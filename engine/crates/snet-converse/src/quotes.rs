use snet_protocol::{Citations, Document, quote_range};

/// The citation of a quote that occurs exactly in its
/// document, at the quote's first occurrence.
pub(crate) fn verify(
    citations: &mut Citations,
    documents: &[Document],
    document: usize,
    text: &str,
) -> Option<u32> {
    let range = quote_range(&documents.get(document)?.text, text)?;
    Some(citations.cite(document, text, range))
}
