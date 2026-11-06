use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use quaynor::chat::{ChatBuilder, ChatHandleAsync, Message};
use quaynor::llm::{Model, get_model};
use quaynor::sampler_config::{SampleStep, SamplerConfig, ShiftStep};
use sha2::{Digest, Sha256};
use snet_wire::ConversationRole;
use tokio::sync::Mutex;

use crate::errors::LocalError;
use crate::reply::{ListedModel, LocalAnswer, parse_answer};
use crate::request::LocalRequest;

const HASH_BUFFER_BYTES: usize = 1024 * 1024;

fn engine_error(cause: impl std::fmt::Display) -> LocalError {
    LocalError::Engine {
        cause: cause.to_string(),
    }
}

/// One GGUF model loaded in this process and the single chat worker that
/// runs it. A worker that fails is replaced on the next request.
pub(crate) struct GgufEngine {
    model: Arc<Model>,
    listed: ListedModel,
    context_tokens: u32,
    chat: Mutex<Option<ChatHandleAsync>>,
}

impl GgufEngine {
    /// Hashes and loads the file on a blocking thread.
    pub(crate) async fn load(path: &Path, context_tokens: u64) -> Result<Self, LocalError> {
        let context_tokens = u32::try_from(context_tokens)
            .map_err(|_| engine_error("the context does not fit 32 bits"))?;
        // llama.cpp writes its loading log to standard error unless told
        // otherwise; routed to tracing, which has no subscriber here, it
        // stays out of the command's output.
        quaynor::send_llamacpp_logs_to_tracing();
        let owned_path = path.to_path_buf();
        let loaded = tokio::task::spawn_blocking(move || load_blocking(&owned_path))
            .await
            .map_err(|error| engine_error(format!("the loading thread stopped: {error}")))??;
        let (model, digest) = loaded;
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| engine_error("the file name is not UTF-8"))?
            .to_string();
        Ok(Self {
            model: Arc::new(model),
            listed: ListedModel { name, digest },
            context_tokens,
            chat: Mutex::new(None),
        })
    }

    pub(crate) fn listed(&self) -> &ListedModel {
        &self.listed
    }

    pub(crate) async fn ask(
        &self,
        request: &LocalRequest,
        configured_cap: u64,
    ) -> Result<LocalAnswer, LocalError> {
        let mut slot = self.chat.lock().await;
        let chat = slot.get_or_insert_with(|| {
            ChatBuilder::new(Arc::clone(&self.model))
                .with_context_size(self.context_tokens)
                .with_template_variable("enable_thinking".to_string(), false)
                .build_async()
        });
        let result = self.ask_on(chat, request, configured_cap).await;
        if matches!(result, Err(LocalError::Engine { .. })) {
            // The worker thread may have ended; the next request builds a
            // new one instead of failing against a dead channel.
            *slot = None;
        }
        result
    }

    async fn ask_on(
        &self,
        chat: &ChatHandleAsync,
        request: &LocalRequest,
        configured_cap: u64,
    ) -> Result<LocalAnswer, LocalError> {
        let output_cap = request.max_output_tokens.min(configured_cap);
        let Some((last, earlier)) = request.conversation.split_last() else {
            return Err(LocalError::RequestInvalid {
                reason: "the conversation has no message".to_string(),
            });
        };
        if last.role != ConversationRole::User {
            return Err(LocalError::RequestInvalid {
                reason: "the last message is not the user's".to_string(),
            });
        }
        let history = earlier
            .iter()
            .map(|message| match message.role {
                ConversationRole::User => Message::new_user(message.text.clone()),
                ConversationRole::Assistant => Message::new_assistant(message.text.clone()),
            })
            .collect();
        chat.reset_chat(Some(request.instructions.clone()), Vec::new())
            .await
            .map_err(engine_error)?;
        chat.set_chat_history(history).await.map_err(engine_error)?;
        let sampler = SamplerConfig::new()
            .shift(ShiftStep::Grammar {
                trigger_on: None,
                root: "root".to_string(),
                grammar: request.reply_schema.grammar(),
            })
            .sample(SampleStep::Greedy);
        chat.set_sampler_config(sampler)
            .await
            .map_err(engine_error)?;

        let mut stream = chat.ask(last.text.clone());
        let mut completion_tokens = 0u64;
        while stream.next_token().await.is_some() {
            completion_tokens += 1;
            if completion_tokens > output_cap {
                chat.stop_generation();
                // Drained so the worker is idle before the next request.
                stream.completed().await.map_err(engine_error)?;
                return Err(LocalError::Truncated);
            }
        }
        let text = stream.completed().await.map_err(engine_error)?;
        let stats = chat.get_stats().await.map_err(engine_error)?;
        let object = parse_answer(&text, &request.reply_schema)?;
        Ok(LocalAnswer {
            object,
            prompt_tokens: u64::from(stats.context_used).saturating_sub(completion_tokens),
            completion_tokens,
            model: self.listed.clone(),
        })
    }
}

fn load_blocking(path: &PathBuf) -> Result<(Model, String), LocalError> {
    let failed = |cause: String| LocalError::ModelLoad {
        path: path.display().to_string(),
        cause,
    };
    let mut file = std::fs::File::open(path).map_err(|error| failed(error.to_string()))?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; HASH_BUFFER_BYTES];
    loop {
        let read = file
            .read(&mut buffer)
            .map_err(|error| failed(error.to_string()))?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    let digest = hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    let text = path
        .to_str()
        .ok_or_else(|| failed("the path is not UTF-8".to_string()))?;
    let model = get_model(text, true, None).map_err(|error| failed(error.to_string()))?;
    Ok((model, digest))
}
