//! A plain JSON-lines transcript of one task, written before each request
//! leaves the machine so the user can read exactly what was sent.

mod lines;
mod recorder;
mod writer;

pub use lines::{
    ChargeLine, ChunkRunLine, CitationLine, DocumentLine, FanOutLimitsLine, FanOutStartedLine,
    JobRunLine, LocalExchangeLine, QuoteLine, RelayStartedLine, TaskEndedLine, TaskLimitsLine,
    TaskRecord, TaskRefusedLine, TaskStartedLine,
};
pub use writer::{Transcript, TranscriptError};

#[cfg(test)]
mod tests;
