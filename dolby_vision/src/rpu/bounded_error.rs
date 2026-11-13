//! Fixed-storage diagnostics for the bounded HEVC parser.
use std::fmt::{self, Write};
use std::io;

/// Reason the bounded parser refused an input or could not complete parsing.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum ParseErrorCategory {
    /// Malformed syntax, inconsistent fields, invalid alignment, or CRC mismatch.
    InvalidInput,
    /// Recognized syntax that this implementation does not support.
    Unsupported,
    /// Input limit, checked size conversion, or fallible reservation refused.
    Capacity,
    /// The pinned slice reader rejected a read; inspect `io_kind` for its kind.
    Io,
}

/// Fixed diagnostic tags, recorded from the innermost failing layer outward.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum ParseContext {
    /// HEVC RPU parsing and validation.
    Rpu,
    /// RPU header parsing and validation.
    Header,
    /// Mapping curves and coefficients.
    Mapping,
    /// Display-management payload.
    Dm,
    /// Content mapping version 2.9 extension metadata.
    CmV29,
    /// Content mapping version 4.0 extension metadata.
    CmV40,
}

/// An owned parse error with no heap storage, source error, or captured backtrace.
/// Messages retain the first 192 UTF-8 bytes, truncated at a character boundary.
/// At most four context tags are retained, innermost first; later tags are dropped.
/// `io_kind` retains a reader error's kind; its original message/source is discarded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BoundedParseError {
    message: [u8; 192],
    message_len: u8,
    category: ParseErrorCategory,
    contexts: [Option<ParseContext>; 4],
    io_kind: Option<io::ErrorKind>,
}

const _: () = assert!(std::mem::size_of::<BoundedParseError>() <= 256);
const _: () = assert!(std::mem::size_of::<super::rpu_data_mapping::DoviMappingPiece>() <= 1024);
const _: () =
    assert!(std::mem::size_of::<super::extension_metadata::blocks::ExtMetadataBlock>() <= 256);

pub(crate) type BoundedResult<T> = Result<T, BoundedParseError>;

impl BoundedParseError {
    // Only crate-private macros supply static format strings and audited primitive arguments.
    pub(crate) fn new(category: ParseErrorCategory, args: fmt::Arguments<'_>) -> Self {
        let mut error = Self {
            message: [0; 192],
            message_len: 0,
            category,
            contexts: [None; 4],
            io_kind: None,
        };
        let _ = MessageWriter { error: &mut error }.write_fmt(args);
        error
    }

    /// Return the reason parsing failed, independently of diagnostic wording.
    pub fn category(&self) -> ParseErrorCategory {
        self.category
    }

    /// Return the retained UTF-8 message; formatting this view does not allocate.
    pub fn message(&self) -> &str {
        std::str::from_utf8(&self.message[..usize::from(self.message_len)])
            .expect("fixed diagnostic writer preserves UTF-8")
    }

    /// Iterate retained context tags from the innermost layer outward, at most four.
    pub fn contexts(&self) -> impl Iterator<Item = ParseContext> + '_ {
        self.contexts.iter().flatten().copied()
    }

    /// Return the original slice-reader error kind when the category is `Io`.
    pub fn io_kind(&self) -> Option<io::ErrorKind> {
        self.io_kind
    }

    pub(crate) fn context(mut self, context: ParseContext) -> Self {
        if let Some(slot) = self.contexts.iter_mut().find(|slot| slot.is_none()) {
            *slot = Some(context);
        }
        self
    }

    // Legacy adapters are outside the bounded entry's call graph. Reconstruct an I/O
    // source so existing Anyhow downcasts continue to identify reader errors.
    pub(crate) fn into_anyhow(self) -> anyhow::Error {
        if let Some(kind) = self.io_kind {
            anyhow::Error::new(io::Error::new(kind, "RPU slice reader error")).context(self)
        } else {
            anyhow::Error::new(self)
        }
    }
}

struct MessageWriter<'a> {
    error: &'a mut BoundedParseError,
}

impl Write for MessageWriter<'_> {
    fn write_str(&mut self, value: &str) -> fmt::Result {
        let start = usize::from(self.error.message_len);
        let mut count = (self.error.message.len() - start).min(value.len());
        while !value.is_char_boundary(count) {
            count -= 1;
        }
        self.error.message[start..start + count].copy_from_slice(&value.as_bytes()[..count]);
        self.error.message_len += count as u8;
        if count < value.len() {
            // Stop the whole formatting operation, preserving a prefix even if a
            // multibyte character did not fit into the final available bytes.
            Err(fmt::Error)
        } else {
            Ok(())
        }
    }
}

impl From<io::Error> for BoundedParseError {
    fn from(error: io::Error) -> Self {
        let kind = error.kind();
        drop(error);
        let mut bounded = Self::new(
            ParseErrorCategory::Io,
            format_args!("RPU slice reader error"),
        );
        bounded.io_kind = Some(kind);
        bounded
    }
}

impl fmt::Display for BoundedParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for context in self.contexts() {
            write!(f, "{context:?}: ")?;
        }
        f.write_str(self.message())
    }
}

impl std::error::Error for BoundedParseError {}

macro_rules! bounded_error {
    ($category:ident, $message:literal $(, $arg:expr)* $(,)?) => {
        $crate::rpu::bounded_error::BoundedParseError::new(
            $crate::rpu::bounded_error::ParseErrorCategory::$category,
            format_args!($message $(, $arg)*),
        )
    };
    ($message:literal $(, $arg:expr)* $(,)?) => {
        $crate::rpu::bounded_error::bounded_error!(InvalidInput, $message $(, $arg)*)
    };
}
macro_rules! bbail {
    ($($arg:tt)*) => {
        return Err($crate::rpu::bounded_error::bounded_error!($($arg)*))
    };
}
macro_rules! bensure {
    ($condition:expr $(,)?) => {
        if !$condition {
            return Err($crate::rpu::bounded_error::BoundedParseError::new(
                $crate::rpu::bounded_error::ParseErrorCategory::InvalidInput,
                format_args!(stringify!($condition)),
            ));
        }
    };
    ($condition:expr, $($arg:tt)*) => {
        if !$condition {
            $crate::rpu::bounded_error::bbail!($($arg)*);
        }
    };
}
pub(crate) use {bbail, bensure, bounded_error};

#[cfg(test)]
mod tests;
