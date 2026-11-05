mod exchange_task;

use std::sync::Arc;
use std::time::{Duration, SystemTime};

use rustls::ClientConfig;
use sha2::{Digest, Sha256};
use tokio::sync::{oneshot, watch};

use crate::budget::{Charge, TaskBudget};
use crate::config::EndpointConfig;
use crate::errors::{CloudError, Failure};
use crate::json::JsonObject;
use crate::record::{OutboundRecord, OutboundRecorder, RecordStage};
use crate::request::AcceptedTurn;
use crate::secret::ApiKey;
use crate::switch::CloudSwitch;
use crate::tls;
use crate::transport::PreparedRequest;
use crate::wire::{self, CloudRequest, ReportedUsage};

use exchange_task::ExchangeTask;

/// The longest the client waits for the recorder on one call.
pub(crate) const RECORDER_TIME_LIMIT: Duration = Duration::from_secs(30);

/// An accepted reply: the object, already checked against the request's
/// schema, with what the provider reported and what the task was charged.
#[derive(Debug, Clone)]
pub struct CloudReply {
    pub object: JsonObject,
    /// The reply to send back in a later turn of the same conversation.
    pub turn: AcceptedTurn,
    pub model: String,
    pub usage: ReportedUsage,
    pub charged: Charge,
    pub provider_request_id: Option<String>,
}

struct Shared<R> {
    config: EndpointConfig,
    key: ApiKey,
    tls: Arc<ClientConfig>,
    recorder: Arc<R>,
    switch: CloudSwitch,
    /// How many exchange tasks are running.
    exchanges_running: watch::Sender<usize>,
}

/// Sends single structured requests to one configured cloud endpoint.
pub struct CloudClient<R: OutboundRecorder> {
    shared: Arc<Shared<R>>,
}

impl<R: OutboundRecorder> CloudClient<R> {
    pub fn new(
        config: EndpointConfig,
        key: ApiKey,
        recorder: Arc<R>,
        switch: CloudSwitch,
    ) -> Result<Self, CloudError> {
        let tls = tls::client_config(&config).map_err(|error| CloudError {
            endpoint: config.name.clone(),
            failure: Failure::TlsConfiguration {
                cause: error.to_string(),
            },
        })?;
        Ok(Self {
            shared: Arc::new(Shared {
                config,
                key,
                tls,
                recorder,
                switch,
                exchanges_running: watch::channel(0).0,
            }),
        })
    }

    pub fn config(&self) -> &EndpointConfig {
        &self.shared.config
    }

    /// Sends one request. Every check that can refuse it runs before it is
    /// recorded; once recorded, the exchange runs as its own task, so a caller
    /// that stops waiting cannot keep it from its outcome record or its
    /// charge.
    pub async fn send(
        &self,
        budget: &TaskBudget,
        request: CloudRequest,
    ) -> Result<CloudReply, CloudError> {
        let fail = |failure| self.error(failure);
        let config = &self.shared.config;

        if !self.shared.switch.allows_sending().await {
            return Err(fail(Failure::SwitchOff));
        }
        let pseudonymisation = request.reports_for(config).map_err(fail)?;
        let body = wire::build_body(config, &request).map_err(fail)?;
        let body_size = body.len() as u64;
        if body_size > config.request_bytes_cap {
            return Err(fail(Failure::RequestTooLarge {
                size: body_size,
                cap: config.request_bytes_cap,
            }));
        }
        let worst_case = Charge::worst_case(
            body_size,
            config.input_token_allowance,
            request.max_output_tokens,
            &config.prices,
        )
        .map_err(|error| fail(Failure::Budget(error)))?;
        let prepared =
            PreparedRequest::new(config, &self.shared.key, body.clone()).map_err(fail)?;

        let reservation = budget
            .reserve(worst_case)
            .map_err(|error| fail(Failure::Budget(error)))?;

        let outbound = OutboundRecord {
            time: SystemTime::now(),
            task_id: budget.task_id().to_string(),
            endpoint_name: config.name.clone(),
            host: config.base_url.host().to_string(),
            port: config.base_url.port(),
            model: config.model.clone(),
            wire_format: config.wire.format(),
            configuration_digest: config.digest,
            method: "POST",
            path: prepared.path.clone(),
            headers: prepared.recordable_headers(),
            body_size,
            body_sha256: Sha256::digest(&body).into(),
            body,
            pseudonymisation,
        };
        let recorded = tokio::time::timeout(
            RECORDER_TIME_LIMIT,
            self.shared.recorder.record_outbound(outbound),
        )
        .await;
        let outbound_id = match recorded {
            Ok(Ok(outbound_id)) => outbound_id,
            Ok(Err(source)) => {
                budget
                    .release(reservation)
                    .map_err(|error| fail(Failure::Budget(error)))?;
                return Err(fail(Failure::RecorderFailed {
                    stage: RecordStage::Outbound,
                    source,
                }));
            }
            Err(_elapsed) => {
                budget
                    .release(reservation)
                    .map_err(|error| fail(Failure::Budget(error)))?;
                return Err(fail(Failure::RecorderTimedOut {
                    stage: RecordStage::Outbound,
                }));
            }
        };

        let (result_sender, result_receiver) = oneshot::channel();
        let task = ExchangeTask {
            shared: Arc::clone(&self.shared),
            budget: budget.clone(),
            reservation,
            worst_case,
            outbound_id,
            prepared,
            reply_schema: request.reply_schema.checked().clone(),
            output_cap: request.max_output_tokens,
            body_size,
            result_sender,
        };
        // The task is detached: dropping this future stops only the waiting.
        let running = ExchangeRunning::start(&self.shared.exchanges_running);
        tokio::spawn(async move {
            task.run().await;
            drop(running);
        });
        match result_receiver.await {
            Ok(result) => result,
            Err(_task_ended_without_sending) => Err(fail(Failure::ExchangeTaskFailed {
                cause: "the exchange task stopped before sending its result".to_string(),
            })),
        }
    }

    /// Resolves once no exchange task of this client is running, so a caller
    /// that ends can first let every exchange record its outcome.
    pub async fn exchanges_finished(&self) {
        let mut running = self.shared.exchanges_running.subscribe();
        let _none_running_or_closed = running.wait_for(|count| *count == 0).await;
    }

    fn error(&self, failure: Failure) -> CloudError {
        CloudError {
            endpoint: self.shared.config.name.clone(),
            failure,
        }
    }
}

/// Counts one exchange task as running until dropped, even when the task
/// panics.
struct ExchangeRunning {
    counter: watch::Sender<usize>,
}

impl ExchangeRunning {
    fn start(counter: &watch::Sender<usize>) -> Self {
        counter.send_modify(|count| *count += 1);
        Self {
            counter: counter.clone(),
        }
    }
}

impl Drop for ExchangeRunning {
    fn drop(&mut self) {
        self.counter
            .send_modify(|count| *count = count.saturating_sub(1));
    }
}
