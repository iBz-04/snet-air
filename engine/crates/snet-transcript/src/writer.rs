use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use serde::Serialize;

use crate::lines::TaskRecord;

const MAX_TASK_ID_LENGTH: usize = 128;

#[derive(Debug, thiserror::Error)]
pub enum TranscriptError {
    #[error(
        "the task ID `{task_id}` cannot name a transcript file: it must be 1 to 128 ASCII letters, digits, `-` or `_`"
    )]
    TaskIdInvalid { task_id: String },
    #[error("could not {action} `{path}`: {cause}")]
    Io {
        action: &'static str,
        path: PathBuf,
        cause: String,
    },
    #[error("could not serialise a transcript line: {cause}")]
    Serialise { cause: String },
    #[error("an earlier transcript write failed part way, so the transcript is closed")]
    Closed,
}

struct Writer {
    file: File,
    lines_written: u64,
}

/// The transcript of one task: `<home>/transcripts/<task-id>.jsonl`, owner
/// only on Unix. Clones share one file.
#[derive(Clone)]
pub struct Transcript {
    path: PathBuf,
    writer: Arc<Mutex<Writer>>,
}

impl std::fmt::Debug for Transcript {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Transcript")
            .field("path", &self.path)
            .finish()
    }
}

impl Transcript {
    /// Creates the task's transcript file, refusing one that already exists.
    pub fn create(home: &Path, task_id: &str) -> Result<Self, TranscriptError> {
        let id_is_valid = !task_id.is_empty()
            && task_id.len() <= MAX_TASK_ID_LENGTH
            && task_id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_');
        if !id_is_valid {
            return Err(TranscriptError::TaskIdInvalid {
                task_id: task_id.to_string(),
            });
        }
        let directory = home.join("transcripts");
        create_private_directory(&directory)?;
        let path = directory.join(format!("{task_id}.jsonl"));
        let file = open_new_private_file(&path)?;
        Ok(Self {
            path,
            writer: Arc::new(Mutex::new(Writer {
                file,
                lines_written: 0,
            })),
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Appends one task step and returns its line number, counted from 1.
    pub fn record_task(&self, record: &TaskRecord) -> Result<u64, TranscriptError> {
        self.write_line(record)
    }

    /// Writes one JSON line and flushes it to the operating system before
    /// returning, so a request is never sent ahead of its line.
    pub(crate) fn write_line(&self, value: &impl Serialize) -> Result<u64, TranscriptError> {
        let mut line = serde_json::to_vec(value).map_err(|error| TranscriptError::Serialise {
            cause: error.to_string(),
        })?;
        line.push(b'\n');
        // A poisoned lock means a writer panicked part way through a line, so
        // the file may end in a partial line and nothing more may follow it.
        let mut writer = self.writer.lock().map_err(|_| TranscriptError::Closed)?;
        writer
            .file
            .write_all(&line)
            .and_then(|()| writer.file.flush())
            .map_err(|error| TranscriptError::Io {
                action: "write to",
                path: self.path.clone(),
                cause: error.to_string(),
            })?;
        writer.lines_written += 1;
        Ok(writer.lines_written)
    }
}

fn create_private_directory(directory: &Path) -> Result<(), TranscriptError> {
    let mut builder = std::fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    std::os::unix::fs::DirBuilderExt::mode(&mut builder, 0o700);
    builder
        .create(directory)
        .map_err(|error| TranscriptError::Io {
            action: "create the directory",
            path: directory.to_path_buf(),
            cause: error.to_string(),
        })
}

fn open_new_private_file(path: &Path) -> Result<File, TranscriptError> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    options.open(path).map_err(|error| TranscriptError::Io {
        action: "create",
        path: path.to_path_buf(),
        cause: error.to_string(),
    })
}
