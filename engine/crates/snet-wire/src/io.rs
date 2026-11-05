use std::io;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::task::{Context, Poll};

use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};

const READ_CHUNK_BYTES: usize = 16 * 1024;
const HEAD_END: &[u8] = b"\r\n\r\n";

#[derive(Debug, thiserror::Error)]
#[error("the reply head exceeds {limit} bytes")]
pub struct ReplyHeadTooLarge {
    limit: usize,
}

/// Connects tokio's streams to hyper's IO traits without hyper-util, which is
/// banned. It counts the request bytes the TLS layer accepted, which
/// the outcome record reports, and enforces the reply head limit of
/// the cloud client exactly.
pub struct HyperIo<T> {
    inner: T,
    read_chunk: Box<[u8]>,
    bytes_written: Arc<AtomicU64>,
    head: HeadLimit,
}

impl<T> HyperIo<T> {
    pub fn new(inner: T, bytes_written: Arc<AtomicU64>, max_head_bytes: usize) -> Self {
        Self {
            inner,
            read_chunk: vec![0; READ_CHUNK_BYTES].into_boxed_slice(),
            bytes_written,
            head: HeadLimit::new(max_head_bytes),
        }
    }
}

/// hyper refuses a reply head over `max_buf_size` only while the head is
/// still incomplete, so one large read can carry a longer head through. This
/// counts the bytes before the first blank line instead.
pub(crate) struct HeadLimit {
    max_head_bytes: usize,
    bytes_before_end: usize,
    recent: Vec<u8>,
    ended: bool,
}

impl HeadLimit {
    pub(crate) fn new(max_head_bytes: usize) -> Self {
        Self {
            max_head_bytes,
            bytes_before_end: 0,
            recent: Vec::with_capacity(HEAD_END.len() * 2),
            ended: false,
        }
    }

    pub(crate) fn observe(&mut self, bytes: &[u8]) -> io::Result<()> {
        if self.ended {
            return Ok(());
        }
        for &byte in bytes {
            self.bytes_before_end += 1;
            if self.recent.len() == HEAD_END.len() {
                self.recent.remove(0);
            }
            self.recent.push(byte);
            if self.recent == HEAD_END {
                self.ended = true;
                break;
            }
        }
        if self.bytes_before_end > self.max_head_bytes {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                ReplyHeadTooLarge {
                    limit: self.max_head_bytes,
                },
            ));
        }
        Ok(())
    }
}

impl<T: AsyncRead + Unpin> hyper::rt::Read for HyperIo<T> {
    fn poll_read(
        self: Pin<&mut Self>,
        context: &mut Context<'_>,
        mut cursor: hyper::rt::ReadBufCursor<'_>,
    ) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        let limit = cursor.remaining().min(this.read_chunk.len());
        let mut read_buffer = ReadBuf::new(&mut this.read_chunk[..limit]);
        match Pin::new(&mut this.inner).poll_read(context, &mut read_buffer) {
            Poll::Ready(Ok(())) => {
                this.head.observe(read_buffer.filled())?;
                cursor.put_slice(read_buffer.filled());
                Poll::Ready(Ok(()))
            }
            Poll::Ready(Err(error)) => Poll::Ready(Err(error)),
            Poll::Pending => Poll::Pending,
        }
    }
}

impl<T: AsyncWrite + Unpin> hyper::rt::Write for HyperIo<T> {
    fn poll_write(
        self: Pin<&mut Self>,
        context: &mut Context<'_>,
        buffer: &[u8],
    ) -> Poll<io::Result<usize>> {
        let this = self.get_mut();
        let result = Pin::new(&mut this.inner).poll_write(context, buffer);
        if let Poll::Ready(Ok(written)) = result {
            this.bytes_written
                .fetch_add(written as u64, Ordering::SeqCst);
        }
        result
    }

    fn poll_flush(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.get_mut().inner).poll_flush(context)
    }

    fn poll_shutdown(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.get_mut().inner).poll_shutdown(context)
    }
}

#[cfg(test)]
mod tests;
