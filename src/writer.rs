use std::io::{self, Write};
use zstd::stream::write::Encoder as ZstdEncoder;

pub struct OutputWriter(ZstdEncoder<'static, Box<dyn Write + Send>>);

impl OutputWriter {
    pub fn new(writer: Box<dyn Write + Send>, level: i32, threads: u32) -> io::Result<Self> {
        let mut zstd_out = ZstdEncoder::new(writer, level)?;
        zstd_out.multithread(threads)?;
        zstd_out.include_checksum(true)?;
        Ok(OutputWriter(zstd_out))
    }

    pub fn finish(self) -> io::Result<()> {
        self.0.finish()?;
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
