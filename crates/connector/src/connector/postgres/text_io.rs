//! Requests text results for marked native bindings, without changing parameter bytes.
//!
//! tokio-postgres requests binary results for Bind. This adapter changes only the
//! final result-format word of bindings whose Parse starts with MARKER. Internal
//! driver queries keep their binary format. The adapter buffers at most 8 KiB.

use std::future::Future;
use std::io;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::task::{Context, Poll, ready};

use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio_postgres::tls::{ChannelBinding, MakeTlsConnect, TlsConnect, TlsStream};

pub(super) const MARKER: &str = "/* sqln native text results */\n";

#[derive(Default)]
struct Formats {
    header: Vec<u8>,
    kind: u8,
    remaining: usize,
    prefix: Vec<u8>,
    statement: Option<Vec<u8>>,
}

impl Formats {
    fn byte(&mut self, byte: u8) -> io::Result<u8> {
        if self.remaining == 0 {
            self.header.push(byte);
            if self.header.len() == 5 {
                self.kind = self.header[0];
                let length = u32::from_be_bytes(self.header[1..5].try_into().unwrap());
                self.remaining = length
                    .checked_sub(4)
                    .ok_or_else(|| io::Error::other("Invalid PostgreSQL frame length."))?
                    as usize;
                self.header.clear();
                self.prefix.clear();
            }
            return Ok(byte);
        }
        if self.prefix.len() < 128 {
            self.prefix.push(byte);
        }
        let mut output = byte;
        if self.kind == b'B' && self.remaining <= 4 {
            let name = self.prefix.split(|b| *b == 0).nth(1).unwrap_or_default();
            if self.statement.as_deref() == Some(name) {
                let expected = [0, 1, 0, 1][4 - self.remaining];
                if byte != expected {
                    return Err(io::Error::other(
                        "Unsupported PostgreSQL Bind result format.",
                    ));
                }
                if self.remaining <= 2 {
                    output = 0;
                }
            }
        }
        self.remaining -= 1;
        if self.remaining == 0 {
            match self.kind {
                b'P' => {
                    let mut parts = self.prefix.split(|b| *b == 0);
                    let name = parts.next().unwrap_or_default();
                    if parts
                        .next()
                        .is_some_and(|sql| sql.starts_with(MARKER.as_bytes()))
                    {
                        self.statement = Some(name.to_vec());
                    }
                }
                b'C' if self.prefix.first() == Some(&b'S') => {
                    let name = self.prefix[1..]
                        .split(|b| *b == 0)
                        .next()
                        .unwrap_or_default();
                    if self.statement.as_deref() == Some(name) {
                        self.statement = None;
                    }
                }
                _ => {}
            }
        }
        Ok(output)
    }
}

pub(super) struct TextIo<S> {
    inner: S,
    enabled: Arc<AtomicBool>,
    formats: Formats,
    pending: Vec<u8>,
    sent: usize,
}

impl<S> TextIo<S> {
    pub(super) fn new(inner: S, enabled: Arc<AtomicBool>) -> Self {
        Self {
            inner,
            enabled,
            formats: Formats::default(),
            pending: Vec::new(),
            sent: 0,
        }
    }
}

impl<S: AsyncWrite + Unpin> TextIo<S> {
    fn flush_pending(&mut self, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        while self.sent < self.pending.len() {
            let count =
                ready!(Pin::new(&mut self.inner).poll_write(cx, &self.pending[self.sent..]))?;
            if count == 0 {
                return Poll::Ready(Err(io::ErrorKind::WriteZero.into()));
            }
            self.sent += count;
        }
        self.pending.clear();
        self.sent = 0;
        Poll::Ready(Ok(()))
    }
}

impl<S: AsyncRead + Unpin> AsyncRead for TextIo<S> {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        Pin::new(&mut self.inner).poll_read(cx, buf)
    }
}

impl<S: AsyncWrite + Unpin> AsyncWrite for TextIo<S> {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        ready!(self.flush_pending(cx))?;
        if !self.enabled.load(Ordering::SeqCst) {
            return Pin::new(&mut self.inner).poll_write(cx, buf);
        }
        let count = buf.len().min(8192);
        for byte in &buf[..count] {
            let byte = self.formats.byte(*byte)?;
            self.pending.push(byte);
        }
        Poll::Ready(Ok(count))
    }
    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        ready!(self.flush_pending(cx))?;
        Pin::new(&mut self.inner).poll_flush(cx)
    }
    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        ready!(self.flush_pending(cx))?;
        Pin::new(&mut self.inner).poll_shutdown(cx)
    }
}

impl<S: TlsStream + Unpin> TlsStream for TextIo<S> {
    fn channel_binding(&self) -> ChannelBinding {
        self.inner.channel_binding()
    }
}

pub(super) struct TextTls<T> {
    pub inner: T,
    pub enabled: Arc<AtomicBool>,
}

impl<S, T> MakeTlsConnect<S> for TextTls<T>
where
    S: Send + 'static,
    T: MakeTlsConnect<S>,
    <T::TlsConnect as TlsConnect<S>>::Future: Send + 'static,
    T::Stream: 'static,
    <T::TlsConnect as TlsConnect<S>>::Error: 'static,
{
    type Stream = TextIo<T::Stream>;
    type TlsConnect = TextTls<T::TlsConnect>;
    type Error = T::Error;
    fn make_tls_connect(&mut self, domain: &str) -> Result<Self::TlsConnect, Self::Error> {
        Ok(TextTls {
            inner: self.inner.make_tls_connect(domain)?,
            enabled: self.enabled.clone(),
        })
    }
}

impl<S, T> TlsConnect<S> for TextTls<T>
where
    S: Send + 'static,
    T: TlsConnect<S>,
    T::Future: Send + 'static,
    T::Stream: 'static,
    T::Error: 'static,
{
    type Stream = TextIo<T::Stream>;
    type Error = T::Error;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Stream, Self::Error>> + Send>>;
    fn connect(self, stream: S) -> Self::Future {
        let future = self.inner.connect(stream);
        Box::pin(async move { Ok(TextIo::new(future.await?, self.enabled)) })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn frame(tag: u8, body: &[u8]) -> Vec<u8> {
        let mut bytes = vec![tag];
        bytes.extend_from_slice(&((body.len() + 4) as u32).to_be_bytes());
        bytes.extend_from_slice(body);
        bytes
    }
    #[test]
    fn only_marked_bind_results_change() {
        let mut formats = Formats::default();
        let parse = frame(
            b'P',
            format!("s1\0{MARKER}SELECT $1\0\0\x01\0\0\0\x19").as_bytes(),
        );
        for byte in parse {
            assert_eq!(formats.byte(byte).unwrap(), byte);
        }
        let mut body = b"\0s1\0\0\x01\0\0\0\x01\0\0\0\x04a\0b'\0\x01\0\x01".to_vec();
        let original = frame(b'B', &body);
        let output: Vec<_> = original.iter().map(|b| formats.byte(*b).unwrap()).collect();
        body.pop();
        body.push(0);
        assert_eq!(output, frame(b'B', &body));
        let internal = frame(b'B', b"\0s2\0\0\0\0\0\0\x01\0\x01");
        for byte in internal {
            assert_eq!(formats.byte(byte).unwrap(), byte);
        }
        for byte in frame(b'C', b"Ss1\0") {
            formats.byte(byte).unwrap();
        }
        assert!(formats.statement.is_none());
    }
}
