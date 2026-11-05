use crate::detect::{Detection, Source};

/// A range replaced as one value. `source` is the match that sets
/// its category; `whole` says whether that match covers the span.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Span {
    pub(crate) start: usize,
    pub(crate) end: usize,
    pub(crate) source: Source,
    pub(crate) whole: bool,
}

/// Merges matches that share a character, transitively. Sorting by start,
/// then longest, then rank puts each span's deciding match first.
pub(crate) fn merge(mut detections: Vec<Detection>) -> Vec<Span> {
    detections.sort_by(|left, right| {
        left.start
            .cmp(&right.start)
            .then(right.end.cmp(&left.end))
            .then(left.source.cmp(&right.source))
    });
    let mut spans: Vec<Span> = Vec::new();
    for detection in detections {
        if let Some(last) = spans.last_mut()
            && detection.start < last.end
        {
            if detection.end > last.end {
                last.end = detection.end;
                last.whole = false;
            }
            continue;
        }
        spans.push(Span {
            start: detection.start,
            end: detection.end,
            source: detection.source,
            whole: true,
        });
    }
    spans
}
