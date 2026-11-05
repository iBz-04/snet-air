use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use tokio::sync::watch;

/// Something outside the process that also says whether the cloud may be
/// reached. It is asked wherever the switch is checked.
pub trait SwitchSource: Send + Sync {
    fn reports_on(&self) -> Pin<Box<dyn Future<Output = bool> + Send + '_>>;
}

/// The process-wide cloud switch. While it is off no request leaves the
/// machine, and turning it off aborts every exchange in flight. Clones share one switch.
#[derive(Clone)]
pub struct CloudSwitch {
    state: Arc<watch::Sender<bool>>,
    source: Option<Arc<dyn SwitchSource>>,
}

impl std::fmt::Debug for CloudSwitch {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("CloudSwitch")
            .field("on", &self.is_on())
            .field("has_source", &self.source.is_some())
            .finish()
    }
}

impl CloudSwitch {
    pub fn new(starts_on: bool) -> Self {
        let (state, _initial_receiver) = watch::channel(starts_on);
        Self {
            state: Arc::new(state),
            source: None,
        }
    }

    /// A switch that also asks `source` at every check and stays off from the
    /// first time the source reports off.
    pub fn with_source(starts_on: bool, source: Arc<dyn SwitchSource>) -> Self {
        Self {
            source: Some(source),
            ..Self::new(starts_on)
        }
    }

    pub fn turn_on(&self) {
        self.state.send_replace(true);
    }

    pub fn turn_off(&self) {
        self.state.send_replace(false);
    }

    pub fn is_on(&self) -> bool {
        *self.state.borrow()
    }

    /// The switch, then its source.
    pub(crate) async fn allows_sending(&self) -> bool {
        if !self.is_on() {
            return false;
        }
        if let Some(source) = &self.source
            && !source.reports_on().await
        {
            self.turn_off();
            return false;
        }
        self.is_on()
    }

    pub(crate) fn watch(&self) -> watch::Receiver<bool> {
        self.state.subscribe()
    }
}

/// Resolves once the switch is off. A closed channel counts as off, so the
/// exchange fails closed.
pub(crate) async fn turned_off(receiver: &mut watch::Receiver<bool>) {
    let _off_or_closed = receiver.wait_for(|is_on| !*is_on).await;
}
