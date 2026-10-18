//! Reading newline-delimited messages without unbounded buffering.
//!
//! The stdio MCP transport is one JSON-RPC message per line. A peer that never
//! sends a newline must not grow the proxy's memory without limit, so a line
//! is buffered only up to [`MAX_LINE_BYTES`]; past that the caller decides
//! whether to drop the rest or stream it through.

use serde_json::Value;
use std::io::{self, BufRead, Write};

/// The longest message the proxy buffers and inspects: 4 MiB, as in the source rule set.
pub const MAX_LINE_BYTES: usize = 4 * 1024 * 1024;

pub enum Line {
    /// A whole line, including its trailing newline when there was one.
    Complete(Vec<u8>),
    /// The first [`MAX_LINE_BYTES`] of a longer line. The rest is still unread.
    Oversized(Vec<u8>),
}

/// Read one line, or `None` at end of input.
pub fn read_line(reader: &mut impl BufRead, limit: usize) -> io::Result<Option<Line>> {
    let mut line = Vec::new();
    loop {
        let buf = match reader.fill_buf() {
            Ok(buf) => buf,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        };
        if buf.is_empty() {
            return Ok((!line.is_empty()).then_some(Line::Complete(line)));
        }
        let room = limit - line.len();
        match buf.iter().position(|&b| b == b'\n') {
            Some(i) if i < room => {
                line.extend_from_slice(&buf[..=i]);
                reader.consume(i + 1);
                return Ok(Some(Line::Complete(line)));
            },
            _ => {
                let take = buf.len().min(room);
                line.extend_from_slice(&buf[..take]);
                reader.consume(take);
                if line.len() == limit {
                    return Ok(Some(Line::Oversized(line)));
                }
            },
        }
    }
}

/// Consume the remainder of the current line, newline included, writing it to
/// `sink` when one is given. Never buffers more than one read.
pub fn finish_line(reader: &mut impl BufRead, mut sink: Option<&mut dyn Write>) -> io::Result<()> {
    loop {
        let buf = match reader.fill_buf() {
            Ok(buf) => buf,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        };
        if buf.is_empty() {
            return Ok(());
        }
        let (take, done) = match buf.iter().position(|&b| b == b'\n') {
            Some(i) => (i + 1, true),
            None => (buf.len(), false),
        };
        if let Some(out) = sink.as_mut() {
            out.write_all(&buf[..take])?;
        }
        reader.consume(take);
        if done {
            return Ok(());
        }
    }
}

/// The longest id [`IdScanner`] reads; a longer one is not an id worth
/// answering.
const MAX_ID_BYTES: usize = 256;

/// The most messages [`IdScanner`] remembers from one line.
const MAX_MESSAGES: usize = 1024;

/// Finds the ids of the messages in a line too long to buffer, reading it a
/// chunk at a time in constant memory, so a request whose answer is dropped
/// can still be told so. Wherever the id sits in its message, before or after
/// a 4 MiB `result`, it is found.
///
/// It reads only what it needs: the top-level keys of a single message, or
/// of each object in a top-level batch. It does not validate the JSON, and a
/// key spelled with escapes is not recognised; either way the worst outcome
/// is an id it misses, whose request then waits until the client gives up.
#[derive(Default)]
pub struct IdScanner {
    depth: usize,
    /// The container kinds (`{` or `[`) at depths 1 and 2.
    outer: [u8; 2],
    in_string: bool,
    escaped: bool,
    /// The next string at message depth is a key.
    expect_key: bool,
    /// The key being read, while it is read.
    key: Option<Vec<u8>>,
    /// The last key read at message depth, until its value ends.
    last_key: Vec<u8>,
    /// The raw bytes of an `id` value, while it is read.
    capture: Option<Vec<u8>>,
    current: Scanned,
    messages: Vec<Scanned>,
}

#[derive(Default, Clone)]
struct Scanned {
    id: Option<Value>,
    has_method: bool,
}

impl IdScanner {
    pub fn feed(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.byte(b);
        }
    }

    /// The ids of the responses (messages with an id and no method) read so
    /// far, in order.
    pub fn response_ids(&self) -> impl Iterator<Item = &Value> {
        self.messages
            .iter()
            .filter(|m| !m.has_method)
            .filter_map(|m| m.id.as_ref())
    }

    /// The depth whose objects are messages: 1 for a single message, 2 for
    /// the objects of a batch.
    fn message_depth(&self) -> Option<usize> {
        match self.outer {
            [b'{', _] => Some(1),
            [b'[', b'{'] => Some(2),
            _ => None,
        }
    }

    fn byte(&mut self, b: u8) {
        if self.in_string {
            self.string_byte(b);
            return;
        }
        let at_message = Some(self.depth) == self.message_depth();
        match b {
            b'"' => {
                self.in_string = true;
                if at_message && self.expect_key {
                    self.expect_key = false;
                    self.key = Some(Vec::new());
                } else {
                    self.push_capture(b);
                }
            },
            b'{' | b'[' => {
                if at_message && self.capture.take().is_some() {
                    // An id that is an object or array: not one to answer.
                    self.current.id = None;
                }
                self.depth += 1;
                if self.depth <= 2 {
                    self.outer[self.depth - 1] = b;
                }
                if Some(self.depth) == self.message_depth() {
                    self.current = Scanned::default();
                    self.expect_key = true;
                    self.last_key.clear();
                }
            },
            b'}' | b']' => {
                if at_message {
                    self.end_value();
                    if b == b'}' && self.messages.len() < MAX_MESSAGES {
                        self.messages.push(std::mem::take(&mut self.current));
                    }
                }
                self.depth = self.depth.saturating_sub(1);
                if self.depth < 2 {
                    self.outer[1] = 0;
                }
                if self.depth == 0 {
                    self.outer[0] = 0;
                }
            },
            b':' if at_message => match self.last_key.as_slice() {
                b"id" => self.capture = Some(Vec::new()),
                b"method" => self.current.has_method = true,
                _ => {},
            },
            b',' if at_message => {
                self.end_value();
                self.expect_key = true;
            },
            _ if b.is_ascii_whitespace() => {},
            _ => self.push_capture(b),
        }
    }

    fn string_byte(&mut self, b: u8) {
        let closing = !self.escaped && b == b'"';
        self.escaped = !self.escaped && b == b'\\';
        if closing {
            self.in_string = false;
            if let Some(key) = self.key.take() {
                self.last_key = key;
                return;
            }
        } else if let Some(key) = self.key.as_mut() {
            // Longer than any key it looks for: kept unequal to them.
            if key.len() <= 8 {
                key.push(b);
            }
            return;
        }
        self.push_capture(b);
    }

    fn push_capture(&mut self, b: u8) {
        if let Some(capture) = self.capture.as_mut() {
            if capture.len() <= MAX_ID_BYTES {
                capture.push(b);
            }
        }
    }

    fn end_value(&mut self) {
        if let Some(raw) = self.capture.take() {
            self.current.id = (raw.len() <= MAX_ID_BYTES)
                .then(|| serde_json::from_slice::<Value>(&raw).ok())
                .flatten()
                .filter(|id| id.is_number() || id.is_string());
        }
        self.last_key.clear();
    }
}

impl Write for IdScanner {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.feed(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic)]
mod tests {
    use super::*;
    use std::io::BufReader;

    fn complete(line: Option<Line>) -> Vec<u8> {
        match line {
            Some(Line::Complete(l)) => l,
            Some(Line::Oversized(_)) => panic!("unexpectedly oversized"),
            None => panic!("unexpected end of input"),
        }
    }

    #[test]
    fn lines_are_returned_byte_for_byte() {
        let mut r = BufReader::with_capacity(3, &b"ab\r\nnot json\nlast"[..]);
        assert_eq!(complete(read_line(&mut r, 100).unwrap()), b"ab\r\n");
        assert_eq!(complete(read_line(&mut r, 100).unwrap()), b"not json\n");
        assert_eq!(complete(read_line(&mut r, 100).unwrap()), b"last");
        assert!(read_line(&mut r, 100).unwrap().is_none());
    }

    #[test]
    fn a_long_line_stops_at_the_limit_and_can_be_skipped_or_streamed() {
        let input = b"0123456789\nnext\n";
        let mut r = BufReader::with_capacity(4, &input[..]);
        match read_line(&mut r, 6).unwrap() {
            Some(Line::Oversized(prefix)) => assert_eq!(prefix, b"012345"),
            _ => panic!("expected an oversized line"),
        }
        let mut rest = Vec::new();
        finish_line(&mut r, Some(&mut rest)).unwrap();
        assert_eq!(rest, b"6789\n");
        assert_eq!(complete(read_line(&mut r, 6).unwrap()), b"next\n");

        let mut r = BufReader::new(&input[..]);
        assert!(matches!(
            read_line(&mut r, 6).unwrap(),
            Some(Line::Oversized(_))
        ));
        finish_line(&mut r, None).unwrap();
        assert_eq!(complete(read_line(&mut r, 6).unwrap()), b"next\n");
    }

    #[test]
    fn a_line_exactly_at_the_limit_with_its_newline_beyond_it_is_oversized() {
        let mut r = BufReader::new(&b"abcdef\n"[..]);
        assert!(matches!(
            read_line(&mut r, 6).unwrap(),
            Some(Line::Oversized(_))
        ));
        let mut r = BufReader::new(&b"abcde\n"[..]);
        assert_eq!(complete(read_line(&mut r, 6).unwrap()), b"abcde\n");
    }

    fn ids_in(line: &str, chunk: usize) -> Vec<Value> {
        let mut scanner = IdScanner::default();
        for part in line.as_bytes().chunks(chunk) {
            scanner.feed(part);
        }
        scanner.response_ids().cloned().collect()
    }

    #[test]
    fn the_id_scanner_finds_response_ids_wherever_they_are() {
        let big = "x".repeat(10_000);
        for chunk in [1, 7, 4096] {
            let first = format!(r#"{{"jsonrpc":"2.0","id":7,"result":{{"text":"{big}"}}}}"#);
            assert_eq!(ids_in(&first, chunk), [serde_json::json!(7)]);
            let last = format!(
                r#"{{"result":{{"content":[{{"id":1,"text":"a\"id\":2 {big}"}}]}}, "id" : "abc"}}"#
            );
            assert_eq!(ids_in(&last, chunk), [serde_json::json!("abc")]);
            let batch = format!(
                r#"[{{"id":1,"result":"{big}"}},{{"id":2,"method":"x"}},{{"result":[],"id":3.0}}]"#
            );
            assert_eq!(
                ids_in(&batch, chunk),
                [serde_json::json!(1), serde_json::json!(3.0)]
            );
            let nested = r#"{"id":{"a":1},"result":{}}"#;
            assert_eq!(ids_in(nested, chunk), Vec::<Value>::new());
            let request = r#"{"method":"m","id":4,"params":{}}"#;
            assert_eq!(ids_in(request, chunk), Vec::<Value>::new());
        }
    }
}
