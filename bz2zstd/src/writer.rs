use std::io::{self, Write};

pub struct OutputWriter(Box<dyn Write + Send>);

impl OutputWriter {
    pub fn new(writer: Box<dyn Write + Send>) -> io::Result<Self> {
        Ok(OutputWriter(writer))
    }

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
