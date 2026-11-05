/// How the command ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Exit {
    Success,
    /// The task ended without an answer: not found in the files, or failed.
    NoAnswer,
    Usage,
    Configuration,
    Failed,
}

impl Exit {
    pub fn code(self) -> u8 {
        match self {
            Exit::Success => 0,
            Exit::NoAnswer => 1,
            Exit::Usage => 2,
            Exit::Configuration => 3,
            Exit::Failed => 4,
        }
    }
}

/// A failure the command reports on standard error. The message never holds
/// the API key, document text or an answer.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{message}")]
pub struct CommandError {
    pub exit: Exit,
    pub message: String,
}

impl CommandError {
    pub fn new(exit: Exit, message: impl Into<String>) -> Self {
        Self {
            exit,
            message: message.into(),
        }
    }

    pub fn usage(message: impl Into<String>) -> Self {
        Self::new(Exit::Usage, message)
    }

    pub fn configuration(message: impl Into<String>) -> Self {
        Self::new(Exit::Configuration, message)
    }

    pub fn failed(message: impl Into<String>) -> Self {
        Self::new(Exit::Failed, message)
    }
}
