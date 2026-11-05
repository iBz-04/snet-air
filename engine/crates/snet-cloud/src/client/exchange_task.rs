use std::sync::Arc;
use std::time::SystemTime;

use bytes::Bytes;
use sha2::{Digest, Sha256};
use tokio::sync::oneshot;

use super::{CloudReply, RECORDER_TIME_LIMIT, Shared};
use crate::budget::{Charge, Reservation, TaskBudget};
use crate::errors::{CloudError, Failure};
use crate::record::{OutboundRecordId, OutboundRecorder, OutcomeRecord, RecordStage};
use crate::request::AcceptedTurn;
use crate::schema::ReplySchema;
use crate::transport::{self, Exchange, ExchangeProgress, PreparedRequest};
use crate::wire;
use snet_pseudonym::Destination;

/// Everything a recorded exchange needs, owned, so the exchange can run as
/// its own tokio task that the caller cannot cancel.
pub(super) struct ExchangeTask<R> {
    pub(super) shared: Arc<Shared<R>>,
    pub(super) budget: TaskBudget,
    pub(super) reservation: Reservation,
    pub(super) worst_case: Charge,
    pub(super) outbound_id: OutboundRecordId,
    pub(super) prepared: PreparedRequest,
    pub(super) reply_schema: ReplySchema,
    pub(super) output_cap: u64,
    pub(super) body_size: u64,
    pub(super) result_sender: oneshot::Sender<Result<CloudReply, CloudError>>,
}

impl<R: OutboundRecorder> ExchangeTask<R> {
    pub(super) async fn run(self) {
        let Self {
            shared,
            budget,
            reservation,
            worst_case,
            outbound_id,
            prepared,
            reply_schema,
            output_cap,
            body_size,
            result_sender,
        } = self;
        let config = &shared.config;
        let format = config.wire.format();
        let mut progress = ExchangeProgress::new(&prepared);
        let exchange = Exchange {
            config,
            tls: Arc::clone(&shared.tls),
            switch: &shared.switch,
            request: prepared,
            request_id_header: wire::request_id_header(format),
        };
        let transport_result = transport::run(exchange, &mut progress).await;

        let settlement = Settlement {
            shared: &shared,
            budget: &budget,
            worst_case,
            reply_schema: &reply_schema,
            output_cap,
            body_size,
            caller_abandoned: result_sender.is_closed(),
        };
        let (result, usage_charged) = settlement.conclude(reservation, transport_result, &progress);

        let bytes_written = progress.bytes_written();
        let reply_body = Bytes::from(progress.reply_body);
        let outcome = OutcomeRecord {
            outbound: outbound_id,
            time: SystemTime::now(),
            bytes_written,
            http_status: progress.status,
            provider_request_id: progress.request_id,
            reply_size: reply_body.len() as u64,
            reply_sha256: Sha256::digest(&reply_body).into(),
            reply_body,
            usage_charged,
            error_kind: result.as_ref().err().map(Failure::kind),
        };
        let fail = |failure| CloudError {
            endpoint: config.name.clone(),
            failure,
        };
        let recorded =
            tokio::time::timeout(RECORDER_TIME_LIMIT, shared.recorder.record_outcome(outcome))
                .await;
        let result = match recorded {
            Ok(Ok(())) => result.map_err(fail),
            Ok(Err(source)) => Err(fail(Failure::RecorderFailed {
                stage: RecordStage::Outcome,
                source,
            })),
            Err(_elapsed) => Err(fail(Failure::RecorderTimedOut {
                stage: RecordStage::Outcome,
            })),
        };
        // A caller that stopped waiting has dropped the receiver; the outcome
        // is already recorded, so the undelivered result is discarded.
        let _ = result_sender.send(result);
    }
}

struct Settlement<'a, R> {
    shared: &'a Shared<R>,
    budget: &'a TaskBudget,
    worst_case: Charge,
    reply_schema: &'a ReplySchema,
    output_cap: u64,
    body_size: u64,
    caller_abandoned: bool,
}

impl<R> Settlement<'_, R> {
    /// Interprets the exchange and settles the budget.
    /// Returns the result for the caller and what the task is now charged for
    /// this request. Only an accepted reply delivered to a waiting caller
    /// replaces the reservation; every other ending, including a caller that
    /// stopped waiting, keeps it in full.
    fn conclude(
        &self,
        reservation: Reservation,
        transport_result: Result<(), Failure>,
        progress: &ExchangeProgress,
    ) -> (Result<CloudReply, Failure>, Charge) {
        let config = &self.shared.config;
        let budget = self.budget;
        let worst_case = self.worst_case;
        let reply_schema = self.reply_schema;
        let body_size = self.body_size;
        let format = config.wire.format();
        let kept = reservation.amount();

        if let Err(failure) = transport_result {
            return (Err(failure), kept);
        }
        if self.caller_abandoned {
            return (Err(Failure::Abandoned), kept);
        }
        let Some(status) = progress.status else {
            return (
                Err(Failure::Http {
                    cause: "the exchange ended without a status".to_string(),
                }),
                kept,
            );
        };
        if !(200..300).contains(&status) {
            let failure = wire::provider_failure(
                format,
                status,
                &progress.reply_body,
                progress.request_id.clone(),
            );
            return (Err(failure), kept);
        }

        let parsed = match wire::parse_success(
            format,
            &progress.reply_body,
            reply_schema,
            self.output_cap,
        ) {
            Ok(parsed) => parsed,
            Err(failure) => return (Err(failure), kept),
        };
        let usage_charge = match wire::usage_charge(parsed.usage, u128::from(body_size), config) {
            Ok(charge) => charge,
            Err(_too_large_to_price) => {
                // Usage that cannot even be priced is above any worst case.
                return match budget.close() {
                    Ok(()) => (Err(Failure::UsageAboveWorstCase), kept),
                    Err(error) => (Err(Failure::Budget(error)), kept),
                };
            }
        };

        if usage_charge.exceeds(&worst_case) {
            return match budget.settle_and_close(reservation, usage_charge) {
                Ok(()) => (Err(Failure::UsageAboveWorstCase), usage_charge),
                Err(error) => (Err(Failure::Budget(error)), kept),
            };
        }
        let accepted = match parsed.result {
            Ok(accepted) => accepted,
            Err(failure) => return (Err(failure), kept),
        };
        if let Err(error) = budget.settle(reservation, usage_charge) {
            return (Err(Failure::Budget(error)), kept);
        }
        let config = &self.shared.config;
        let turn = AcceptedTurn::accepted(
            &accepted.object,
            Destination::new(config.name.clone(), config.base_url.host()),
        );
        let reply = CloudReply {
            turn,
            object: accepted.object,
            model: accepted.model,
            usage: parsed.usage,
            charged: usage_charge,
            provider_request_id: progress.request_id.clone(),
        };
        (Ok(reply), usage_charge)
    }
}
