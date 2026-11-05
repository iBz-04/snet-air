use std::sync::Arc;
use std::time::Duration;

use http::Method;
use tokio::sync::Semaphore;

use crate::config::LocalConfig;
use crate::errors::LocalError;
use crate::exchange::exchange;
use crate::reply::{LocalAnswer, answer, listed_model};
use crate::request::{LocalRequest, build_body};

pub struct LocalModel {
    config: LocalConfig,
    permits: Arc<Semaphore>,
}

impl LocalModel {
    pub fn new(config: LocalConfig) -> Self {
        let permits = Arc::new(Semaphore::new(config.max_concurrent_requests as usize));
        Self { config, permits }
    }

    pub fn config(&self) -> &LocalConfig {
        &self.config
    }

    /// Runs the request checks without sending anything.
    pub fn check(&self, request: &LocalRequest) -> Result<(), LocalError> {
        build_body(&self.config, request).map(|_body| ())
    }

    pub async fn ask(&self, request: &LocalRequest) -> Result<LocalAnswer, LocalError> {
        let body = build_body(&self.config, request)?;
        let total = Duration::from_secs(self.config.total_timeout_seconds);
        tokio::time::timeout(total, self.ask_within(request, body))
            .await
            .map_err(|_elapsed| LocalError::Timeout)?
    }

    async fn ask_within(
        &self,
        request: &LocalRequest,
        body: Vec<u8>,
    ) -> Result<LocalAnswer, LocalError> {
        let _permit = self
            .permits
            .acquire()
            .await
            .map_err(|_closed| LocalError::Http {
                cause: "the request permits were closed".to_string(),
            })?;
        let config = &self.config;
        // The model is looked up before each answer so the digest that is
        // recorded is that of the weights that answered, even after a pull.
        let tags = exchange(
            config.address,
            Method::GET,
            "/api/tags",
            None,
            config.reply_bytes_cap,
        )
        .await?;
        let model = listed_model(&tags, &config.model)?;
        let reply = exchange(
            config.address,
            Method::POST,
            "/api/chat",
            Some(body),
            config.reply_bytes_cap,
        )
        .await?;
        answer(
            &reply,
            &model,
            request.max_output_tokens,
            &request.reply_schema,
        )
    }
}

#[cfg(test)]
mod tests;
