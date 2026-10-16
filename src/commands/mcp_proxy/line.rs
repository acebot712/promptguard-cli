//! Reading newline-delimited messages without unbounded buffering.
//!
//! The stdio MCP transport is one JSON-RPC message per line. A peer that never
//! sends a newline must not grow the proxy's memory without limit, so a line
//! is buffered only up to [`MAX_LINE_BYTES`]; past that the caller decides
//! whether to drop the rest or stream it through.

use std::io::{self, BufRead, Write};

/// The longest message the proxy buffers and inspects: 4 MiB, as in the earlier rule set.
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
}
