//! The settings that depend on the wire format: the OpenAI-style key header,
//! instruction role and `store`, the Anthropic workspace, and the effort
//!.

use super::{Effort, InstructionRole, KeyHeader, OpenaiChatSettings, WireFormat, WireSettings};
use crate::errors::ConfigError;

const WORKSPACE_PREFIX: &str = "wrkspc_";
const MAX_WORKSPACE_SUFFIX_LENGTH: usize = 64;
pub(super) const KEY_SCOPED: &str = "key_scoped";

/// The settings an endpoint file gives that belong to one wire format.
pub(super) struct FormatFields {
    pub(super) wire_format: WireFormat,
    pub(super) key_header: Option<KeyHeader>,
    pub(super) instruction_role: Option<InstructionRole>,
    pub(super) send_store_false: Option<bool>,
    pub(super) anthropic_workspace: Option<String>,
}

/// The wire settings and the Anthropic workspace to name, if any.
pub(super) fn wire_settings(
    fields: &FormatFields,
) -> Result<(WireSettings, Option<String>), ConfigError> {
    match fields.wire_format {
        WireFormat::OpenaiChat => {
            if fields.anthropic_workspace.is_some() {
                return Err(ConfigError::invalid(
                    "anthropic_workspace",
                    "only anthropic_messages endpoints take it",
                ));
            }
            let key_header = fields.key_header.ok_or_else(|| {
                ConfigError::invalid("key_header", "openai_chat endpoints must set it")
            })?;
            let instruction_role = fields.instruction_role.ok_or_else(|| {
                ConfigError::invalid("instruction_role", "openai_chat endpoints must set it")
            })?;
            let send_store_false = fields.send_store_false.ok_or_else(|| {
                ConfigError::invalid("send_store_false", "openai_chat endpoints must set it")
            })?;
            let settings = OpenaiChatSettings {
                key_header,
                instruction_role,
                send_store_false,
            };
            Ok((WireSettings::OpenaiChat(settings), None))
        }
        WireFormat::AnthropicMessages => {
            let only_for_openai_chat = "only openai_chat endpoints take it";
            if fields.key_header.is_some() {
                return Err(ConfigError::invalid("key_header", only_for_openai_chat));
            }
            if fields.instruction_role.is_some() {
                return Err(ConfigError::invalid(
                    "instruction_role",
                    only_for_openai_chat,
                ));
            }
            if fields.send_store_false.is_some() {
                return Err(ConfigError::invalid(
                    "send_store_false",
                    only_for_openai_chat,
                ));
            }
            let workspace = fields.anthropic_workspace.as_deref().ok_or_else(|| {
                ConfigError::invalid(
                    "anthropic_workspace",
                    "anthropic_messages endpoints must set it to `key_scoped` or a workspace ID",
                )
            })?;
            Ok((
                WireSettings::AnthropicMessages,
                anthropic_workspace(workspace)?,
            ))
        }
    }
}

/// `key_scoped`, or `wrkspc_` and 1 to 64 ASCII letters and digits.
fn anthropic_workspace(text: &str) -> Result<Option<String>, ConfigError> {
    if text == KEY_SCOPED {
        return Ok(None);
    }
    let suffix = text.strip_prefix(WORKSPACE_PREFIX).unwrap_or("");
    let is_workspace_id = !suffix.is_empty()
        && suffix.len() <= MAX_WORKSPACE_SUFFIX_LENGTH
        && suffix.bytes().all(|byte| byte.is_ascii_alphanumeric());
    if !is_workspace_id {
        return Err(ConfigError::invalid(
            "anthropic_workspace",
            "it is neither `key_scoped` nor `wrkspc_` followed by 1 to 64 letters and digits",
        ));
    }
    Ok(Some(text.to_string()))
}

/// The effort, checked against the values the wire format takes.
pub(super) fn effort(text: &str, format: WireFormat) -> Result<Effort, ConfigError> {
    let effort = match text {
        "model_default" => Effort::ModelDefault,
        "none" => Effort::None,
        "minimal" => Effort::Minimal,
        "low" => Effort::Low,
        "medium" => Effort::Medium,
        "high" => Effort::High,
        "xhigh" => Effort::Xhigh,
        "max" => Effort::Max,
        _ => return Err(ConfigError::invalid("effort", "it is not a known effort")),
    };
    let anthropic_refuses = matches!(effort, Effort::None | Effort::Minimal);
    if format == WireFormat::AnthropicMessages && anthropic_refuses {
        return Err(ConfigError::invalid(
            "effort",
            "anthropic_messages endpoints take model_default, low, medium, high, xhigh or max",
        ));
    }
    Ok(effort)
}
