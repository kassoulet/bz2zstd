//! Output writer wrapper for bz2zstd.
//!
//! This module provides a thin wrapper around the output writer to provide
//! a consistent interface and ensure proper cleanup via the `finish()` method.

use std::io::{self, Write};

/// Wrapper around an output writer.
///
/// This newtype pattern provides:
/// - Explicit `finish()` method for flushing and cleanup
/// - Consistent error handling
/// - Future extensibility (e.g., progress tracking, checksums)
///
/// # Examples
///
/// ```no_run
/// use std::fs::File;
/// use bz2zstd::writer::OutputWriter;
///
/// let file = File::create("output.zst").unwrap();
/// let mut writer = OutputWriter::new(Box::new(file)).unwrap();
/// writer.write_all(b"data").unwrap();
/// writer.finish().unwrap();
/// ```
pub struct OutputWriter(Box<dyn Write + Send>);

impl OutputWriter {
    /// Creates a new output writer.
    pub fn new(writer: Box<dyn Write + Send>) -> io::Result<Self> {
        Ok(OutputWriter(writer))
    }

    /// Flushes and finalizes the output.
    ///
    /// This should be called when writing is complete to ensure all data
    /// is written to the underlying writer.
    pub fn finish(mut self) -> io::Result<()> {
        self.0.flush()?;
        Ok(())
    }
}

impl Write for OutputWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0.write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.0.flush()
    }
}
