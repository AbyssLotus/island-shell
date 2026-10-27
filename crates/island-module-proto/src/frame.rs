//! One JSON message per line, at most [`MAX_FRAME`] bytes. A line that's too long or isn't a valid
//! message is reported and skipped; the stream carries on with the next line.

use std::io::{self, BufRead, Write};

use crate::msg::Message;

/// 4 MiB. Anything bigger goes through the module's cache folder instead.
pub const MAX_FRAME: usize = 4 << 20;

#[derive(Debug)]
pub enum FrameError {
    /// The line was longer than [`MAX_FRAME`]; it was skipped.
    TooBig,
    /// Not JSON, or not a JSON-RPC message.
    Malformed(String),
    Io(io::Error),
}

impl std::fmt::Display for FrameError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FrameError::TooBig => write!(f, "message over {MAX_FRAME} bytes"),
            FrameError::Malformed(why) => write!(f, "malformed message: {why}"),
            FrameError::Io(e) => write!(f, "{e}"),
        }
    }
}

pub struct FrameReader<R> {
    inner: R,
    max: usize,
}

impl<R: BufRead> FrameReader<R> {
    pub fn new(inner: R) -> Self {
        FrameReader { inner, max: MAX_FRAME }
    }

    /// A smaller limit, for tests.
    pub fn with_limit(inner: R, max: usize) -> Self {
        FrameReader { inner, max }
    }

    /// The next message. `None` at the end of the stream. Blank lines are skipped.
    pub fn next_message(&mut self) -> Option<Result<Message, FrameError>> {
        loop {
            let mut line = Vec::new();
            let mut too_big = false;
            // Read up to the newline without ever holding more than `max` bytes.
            loop {
                let buf = match self.inner.fill_buf() {
                    Ok(b) => b,
                    Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                    Err(e) => return Some(Err(FrameError::Io(e))),
                };
                if buf.is_empty() {
                    if line.is_empty() && !too_big {
                        return None;
                    }
                    break;
                }
                let (chunk, done) = match buf.iter().position(|&b| b == b'\n') {
                    Some(i) => (&buf[..i], i + 1),
                    None => (buf, buf.len()),
                };
                if !too_big {
                    if line.len() + chunk.len() > self.max {
                        too_big = true;
                        line = Vec::new();
                    } else {
                        line.extend_from_slice(chunk);
                    }
                }
                let ended = done <= buf.len() && buf.get(done - 1) == Some(&b'\n');
                self.inner.consume(done);
                if ended {
                    break;
                }
            }
            if too_big {
                return Some(Err(FrameError::TooBig));
            }
            if line.last() == Some(&b'\r') {
                line.pop();
            }
            if line.iter().all(|b| b.is_ascii_whitespace()) {
                continue;
            }
            let parsed = serde_json::from_slice(&line).map_err(|e| FrameError::Malformed(e.to_string())).and_then(|v| Message::from_json(v).map_err(FrameError::Malformed));
            return Some(parsed);
        }
    }
}

impl<R: BufRead> Iterator for FrameReader<R> {
    type Item = Result<Message, FrameError>;
    fn next(&mut self) -> Option<Self::Item> {
        self.next_message()
    }
}

/// Writes one message and its newline, then flushes. Refuses messages over [`MAX_FRAME`].
pub fn write_message<W: Write + ?Sized>(w: &mut W, m: &Message) -> io::Result<()> {
    let mut bytes = serde_json::to_vec(&m.to_json()).map_err(io::Error::other)?;
    if bytes.len() > MAX_FRAME {
        return Err(io::Error::new(io::ErrorKind::InvalidData, format!("message over {MAX_FRAME} bytes")));
    }
    bytes.push(b'\n');
    w.write_all(&bytes)?;
    w.flush()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::io::{BufReader, Cursor};

    fn read_all(input: &[u8], max: usize, cap: usize) -> Vec<Result<Message, String>> {
        // A tiny BufReader capacity exercises lines that span several reads.
        let r = BufReader::with_capacity(cap, Cursor::new(input.to_vec()));
        FrameReader::with_limit(r, max).map(|m| m.map_err(|e| e.to_string())).collect()
    }

    #[test]
    fn writes_and_reads_back() {
        let mut out = Vec::new();
        let msgs = [Message::request(1, "a", json!({ "x": "é ✓" })), Message::notification("b", json!(null)), Message::ok(1, json!([1, 2, 3]))];
        for m in &msgs {
            write_message(&mut out, m).unwrap();
        }
        assert_eq!(out.iter().filter(|&&b| b == b'\n').count(), 3);
        for cap in [1, 3, 64, 8192] {
            let got: Vec<_> = read_all(&out, MAX_FRAME, cap).into_iter().map(Result::unwrap).collect();
            assert_eq!(got, msgs);
        }
    }

    #[test]
    fn oversize_lines_are_skipped() {
        let big = format!("{{\"jsonrpc\":\"2.0\",\"method\":\"x\",\"params\":\"{}\"}}\n", "a".repeat(200));
        let input = [big.as_bytes(), b"{\"jsonrpc\":\"2.0\",\"method\":\"after\"}\n"].concat();
        for cap in [1, 7, 4096] {
            let got = read_all(&input, 100, cap);
            assert_eq!(got.len(), 2, "cap {cap}");
            assert!(got[0].as_ref().unwrap_err().contains("over"));
            assert_eq!(got[1], Ok(Message::Notification { method: "after".into(), params: serde_json::Value::Null }));
        }
    }

    #[test]
    fn broken_frames_are_reported_and_skipped() {
        let input = b"not json\n\n{\"jsonrpc\":\"2.0\",\"id\":\"x\",\"method\":\"m\"}\r\n{\"jsonrpc\":\"2.0\",\"method\":\"ok\"}\r\n{\"jsonrpc\":\"2.0\",\"method\":\"tail\"}";
        let got = read_all(input, MAX_FRAME, 5);
        assert_eq!(got.len(), 4);
        assert!(got[0].as_ref().unwrap_err().contains("malformed"));
        assert!(got[1].as_ref().unwrap_err().contains("unsigned"));
        assert!(matches!(&got[2], Ok(Message::Notification { method, .. }) if method == "ok"));
        // The last line has no newline: the stream ended, the message still counts.
        assert!(matches!(&got[3], Ok(Message::Notification { method, .. }) if method == "tail"));
    }

    #[test]
    fn writer_refuses_oversize() {
        let mut out = Vec::new();
        let m = Message::notification("host.emit", json!({ "name": "x", "data": "a".repeat(MAX_FRAME) }));
        assert!(write_message(&mut out, &m).is_err());
        assert!(out.is_empty());
    }

    #[test]
    fn empty_stream() {
        assert!(read_all(b"", MAX_FRAME, 8).is_empty());
        assert!(read_all(b"\n\n  \n", MAX_FRAME, 8).is_empty());
    }
}
