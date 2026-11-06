use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use http::Method;
use tokio::sync::Semaphore;

use crate::config::{Backend, LocalConfig};
use crate::errors::LocalError;
use crate::exchange::exchange;
use crate::reply::{ListedModel, LocalAnswer, answer, listed_model, listed_names};
use crate::request::{LocalRequest, build_body, check_request};

const MODEL_LIST_BYTES_CAP: u64 = 1_048_576;

/// The models the Ollama server at `address` has pulled.
pub async fn ollama_models(address: SocketAddr) -> Result<Vec<String>, LocalError> {
    let tags = exchange(
        address,
        Method::GET,
        "/api/tags",
        None,
        MODEL_LIST_BYTES_CAP,
    )
    .await?;
    listed_names(&tags)
}

pub struct LocalModel {
    config: LocalConfig,
    permits: Arc<Semaphore>,
    #[cfg(feature = "gguf")]
    engine: tokio::sync::OnceCell<crate::gguf::GgufEngine>,
}

impl LocalModel {
    pub fn new(config: LocalConfig) -> Self {
        let permits = Arc::new(Semaphore::new(config.max_concurrent_requests as usize));
        Self {
            config,
            permits,
            #[cfg(feature = "gguf")]
            engine: tokio::sync::OnceCell::new(),
        }
    }

    pub fn config(&self) -> &LocalConfig {
        &self.config
    }

    /// Runs the request checks without sending anything.
    pub fn check(&self, request: &LocalRequest) -> Result<(), LocalError> {
        match &self.config.backend {
            Backend::Ollama { .. } => build_body(&self.config, request).map(|_body| ()),
            #[cfg(feature = "gguf")]
            Backend::Gguf { .. } => check_request(&self.config, request),
        }
    }

    /// Makes sure the model can answer before a task spends anything on the
    /// cloud: Ollama is reachable and lists the model, or the GGUF file is
    /// loaded. Returns the model that will answer.
    pub async fn prepare(&self) -> Result<ListedModel, LocalError> {
        match &self.config.backend {
            Backend::Ollama { address } => self.listed_by_ollama(*address).await,
            #[cfg(feature = "gguf")]
            Backend::Gguf { path } => Ok(self.engine(path).await?.listed().clone()),
        }
    }

    pub async fn ask(&self, request: &LocalRequest) -> Result<LocalAnswer, LocalError> {
        check_request(&self.config, request)?;
        let total = Duration::from_secs(self.config.total_timeout_seconds);
        tokio::time::timeout(total, self.ask_within(request))
            .await
            .map_err(|_elapsed| LocalError::Timeout)?
    }

    async fn ask_within(&self, request: &LocalRequest) -> Result<LocalAnswer, LocalError> {
        let _permit = self
            .permits
            .acquire()
            .await
            .map_err(|_closed| LocalError::Http {
                cause: "the request permits were closed".to_string(),
            })?;
        match &self.config.backend {
            Backend::Ollama { address } => self.ask_ollama(*address, request).await,
            #[cfg(feature = "gguf")]
            Backend::Gguf { path } => {
                self.engine(path)
                    .await?
                    .ask(request, self.config.max_output_tokens)
                    .await
            }
        }
    }

    async fn listed_by_ollama(&self, address: SocketAddr) -> Result<ListedModel, LocalError> {
        let tags = exchange(
            address,
            Method::GET,
            "/api/tags",
            None,
            self.config.reply_bytes_cap,
        )
        .await?;
        listed_model(&tags, &self.config.model)
    }

    async fn ask_ollama(
        &self,
        address: SocketAddr,
        request: &LocalRequest,
    ) -> Result<LocalAnswer, LocalError> {
        let body = build_body(&self.config, request)?;
        // The model is looked up before each answer so the digest that is
        // recorded is that of the weights that answered, even after a pull.
        let model = self.listed_by_ollama(address).await?;
        let reply = exchange(
            address,
            Method::POST,
            "/api/chat",
            Some(body),
            self.config.reply_bytes_cap,
        )
        .await?;
        answer(
            &reply,
            &model,
            request.max_output_tokens,
            &request.reply_schema,
        )
    }

    #[cfg(feature = "gguf")]
    async fn engine(&self, path: &std::path::Path) -> Result<&crate::gguf::GgufEngine, LocalError> {
        self.engine
            .get_or_try_init(|| crate::gguf::GgufEngine::load(path, self.config.context_tokens))
            .await
    }
}

#[cfg(test)]
mod tests;
