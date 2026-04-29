//! Error types for parallel bzip2 decompression.

use std::fmt;

/// Error type for parallel bzip2 decompression operations.
#[derive(Debug)]
pub enum Bz2Error {
    /// Failed to decompress a block at the specified bit offset.
    DecompressionFailed {
        /// Bit offset where the block starts
        offset: u64,
        /// The underlying I/O error
        source: std::io::Error,
    },

    /// Invalid bzip2 format detected.
    InvalidFormat(String),

    /// I/O error occurred.
    Io(std::io::Error),

    /// Memory mapping failed.
    MmapFailed(std::io::Error),

    /// Decompression limit exceeded (possible decompression bomb).
    DecompressionLimitExceeded {
        /// Bit offset where the block starts
        offset: u64,
        /// The limit that was exceeded
        limit: usize,
    },

    /// Compressed block size limit exceeded.
    CompressedBlockLimitExceeded {
        /// Bit offset where the block starts
        offset: u64,
        /// The limit that was exceeded
        limit: usize,
    },
}

impl fmt::Display for Bz2Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Bz2Error::DecompressionFailed { offset, source } => {
                write!(
                    f,
                    "Failed to decompress block at bit offset {}: {}",
                    offset, source
                )
            }
            Bz2Error::InvalidFormat(msg) => write!(f, "Invalid bzip2 format: {}", msg),
            Bz2Error::Io(err) => write!(f, "I/O error: {}", err),
            Bz2Error::MmapFailed(err) => write!(f, "Memory mapping failed: {}", err),
            Bz2Error::DecompressionLimitExceeded { offset, limit } => {
                write!(
                    f,
                    "Decompression limit exceeded ({} bytes) at bit offset {}. Possible decompression bomb.",
                    limit, offset
                )
            }
            Bz2Error::CompressedBlockLimitExceeded { offset, limit } => {
                write!(
                    f,
                    "Compressed block limit exceeded ({} bytes) at bit offset {}. Possible malicious file.",
                    limit, offset
                )
            }
        }
    }
}

impl std::error::Error for Bz2Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Bz2Error::DecompressionFailed { source, .. } => Some(source),
            Bz2Error::Io(err) => Some(err),
            Bz2Error::MmapFailed(err) => Some(err),
            Bz2Error::InvalidFormat(_) => None,
            Bz2Error::DecompressionLimitExceeded { .. } => None,
            Bz2Error::CompressedBlockLimitExceeded { .. } => None,
        }
    }
}

impl From<std::io::Error> for Bz2Error {
    fn from(err: std::io::Error) -> Self {
        Bz2Error::Io(err)
    }
}

/// Result type for parallel bzip2 decompression operations.
pub type Result<T> = std::result::Result<T, Bz2Error>;
