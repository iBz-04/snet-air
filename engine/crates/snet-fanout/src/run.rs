use std::sync::Mutex;

use futures_util::StreamExt;
use snet_local::{LocalAnswer, LocalError, LocalModel, LocalRequest};
use snet_protocol::{FailureKind, ProtocolError};

/// One local request of a round: which job and chunk it serves, whether it
/// has started, and its result once it has ended.
pub(crate) struct Slot {
    pub(crate) job: usize,
    pub(crate) chunk: usize,
    pub(crate) started: bool,
    pub(crate) result: Option<Result<LocalAnswer, LocalError>>,
}

/// An unreachable server or a missing model fails every other request the
/// same way, so it ends the task at once.
fn is_fatal(error: &LocalError) -> bool {
    matches!(
        error,
        LocalError::Connect { .. } | LocalError::ModelNotListed { .. }
    )
}

fn lock(slots: &Mutex<Vec<Slot>>) -> std::sync::MutexGuard<'_, Vec<Slot>> {
    slots
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Runs a round's requests with at most `concurrency` in flight, writing
/// each start and result into `slots`, which outlive a dropped round.
pub(crate) async fn run_round(
    local: &LocalModel,
    requests: Vec<LocalRequest>,
    slots: &Mutex<Vec<Slot>>,
    concurrency: usize,
) -> Result<(), ProtocolError> {
    let asks = requests
        .into_iter()
        .enumerate()
        .map(|(index, request)| async move {
            if let Some(slot) = lock(slots).get_mut(index) {
                slot.started = true;
            }
            let result = local.ask(&request).await;
            (index, result)
        });
    let mut finished = futures_util::stream::iter(asks).buffer_unordered(concurrency.max(1));
    while let Some((index, result)) = finished.next().await {
        let fatal = match &result {
            Err(error) if is_fatal(error) => Some(error.to_string()),
            _ => None,
        };
        if let Some(slot) = lock(slots).get_mut(index) {
            slot.result = Some(result);
        }
        if let Some(detail) = fatal {
            return Err(ProtocolError::new(FailureKind::Local, detail));
        }
    }
    Ok(())
}
