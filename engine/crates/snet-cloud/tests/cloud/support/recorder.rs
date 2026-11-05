use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use snet_cloud::{
    CloudSwitch, OutboundRecord, OutboundRecordId, OutboundRecorder, OutcomeRecord, RecorderError,
};

use super::server::Observations;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Succeed,
    Fail,
    Hang,
}

/// Records in memory and can fail, hang, note how many connections a server
/// had seen when the outbound record arrived, or turn the switch off.
pub struct TestRecorder {
    outbound_mode: Mode,
    outcome_mode: Mode,
    watched_server: Option<Arc<Observations>>,
    switch_to_turn_off: Option<CloudSwitch>,
    next_id: AtomicU64,
    pub outbound: Mutex<Vec<OutboundRecord>>,
    pub outcomes: Mutex<Vec<OutcomeRecord>>,
    pub connections_at_outbound: Mutex<Vec<usize>>,
}

impl TestRecorder {
    pub fn new() -> Self {
        Self {
            outbound_mode: Mode::Succeed,
            outcome_mode: Mode::Succeed,
            watched_server: None,
            switch_to_turn_off: None,
            next_id: AtomicU64::new(1),
            outbound: Mutex::new(Vec::new()),
            outcomes: Mutex::new(Vec::new()),
            connections_at_outbound: Mutex::new(Vec::new()),
        }
    }

    pub fn outbound_mode(mut self, mode: Mode) -> Self {
        self.outbound_mode = mode;
        self
    }

    pub fn outcome_mode(mut self, mode: Mode) -> Self {
        self.outcome_mode = mode;
        self
    }

    pub fn watching(mut self, server: &Arc<Observations>) -> Self {
        self.watched_server = Some(Arc::clone(server));
        self
    }

    pub fn turning_off(mut self, switch: &CloudSwitch) -> Self {
        self.switch_to_turn_off = Some(switch.clone());
        self
    }

    pub fn outbound(&self) -> Vec<OutboundRecord> {
        self.outbound.lock().unwrap().clone()
    }

    pub fn outcomes(&self) -> Vec<OutcomeRecord> {
        self.outcomes.lock().unwrap().clone()
    }

    pub async fn wait_for_outcomes(&self, count: usize) -> Vec<OutcomeRecord> {
        for _ in 0..500 {
            let outcomes = self.outcomes();
            if outcomes.len() >= count {
                return outcomes;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        panic!("the recorder did not receive {count} outcomes");
    }
}

impl OutboundRecorder for TestRecorder {
    async fn record_outbound(
        &self,
        record: OutboundRecord,
    ) -> Result<OutboundRecordId, RecorderError> {
        {
            if let Some(server) = &self.watched_server {
                self.connections_at_outbound
                    .lock()
                    .unwrap()
                    .push(server.connections());
            }
            if let Some(switch) = &self.switch_to_turn_off {
                switch.turn_off();
            }
            if self.outbound_mode == Mode::Hang {
                std::future::pending::<()>().await;
            }
            if self.outbound_mode == Mode::Fail {
                return Err(RecorderError::new("the record store is full"));
            }
            self.outbound.lock().unwrap().push(record);
            Ok(OutboundRecordId(
                self.next_id.fetch_add(1, Ordering::SeqCst),
            ))
        }
    }

    async fn record_outcome(&self, outcome: OutcomeRecord) -> Result<(), RecorderError> {
        {
            if self.outcome_mode == Mode::Hang {
                std::future::pending::<()>().await;
            }
            if self.outcome_mode == Mode::Fail {
                return Err(RecorderError::new("the record store is full"));
            }
            self.outcomes.lock().unwrap().push(outcome);
            Ok(())
        }
    }
}
