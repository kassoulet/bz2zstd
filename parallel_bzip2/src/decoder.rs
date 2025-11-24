use std::io::{self, Read};
use std::sync::Arc;
use std::collections::HashMap;
use crossbeam_channel::{bounded, Receiver};
use rayon::ThreadPoolBuilder;

use crate::{scan_blocks, decompress_block_into};

pub struct Bz2Decoder {
    data: Arc<dyn AsRef<[u8]> + Send + Sync>,
    receiver: Receiver<(usize, Vec<u8>)>,
    buffer: Vec<u8>,
    buffer_pos: usize,
    next_block_idx: usize,
    pending_blocks: HashMap<usize, Vec<u8>>,
}

impl Bz2Decoder {
    pub fn open<P: AsRef<std::path::Path>>(path: P) -> anyhow::Result<Self> {
        let file = std::fs::File::open(path)?;
        let mmap = unsafe { memmap2::MmapOptions::new().map(&file)? };
        Ok(Self::new(Arc::new(mmap)))
    }

    pub fn new<T>(data: Arc<T>) -> Self 
    where T: AsRef<[u8]> + Send + Sync + 'static 
    {
        let (result_sender, result_receiver) = bounded(rayon::current_num_threads() * 2);
        let data_ref: Arc<dyn AsRef<[u8]> + Send + Sync> = data;
        let data_clone = data_ref.clone();

        // We spawn a thread to drive the scanning and decompression.
        // This thread will spawn the scanner and then the workers.
        std::thread::spawn(move || {
            let pool = ThreadPoolBuilder::new()
                .num_threads(rayon::current_num_threads())
                .build()
                .unwrap();

            pool.scope(|s| {
                let slice = data_clone.as_ref().as_ref();
                let task_receiver = scan_blocks(s, slice, &pool);

                // Worker loop
                use rayon::prelude::*;
                task_receiver
                    .into_iter()
                    .enumerate()
                    .par_bridge()
                    .try_for_each_init(
                        || Vec::new(), // scratch buffer
                        |scratch, (idx, (start_bit, end_bit))| -> anyhow::Result<()> {
                             let mut decomp_buf = Vec::new();
                             decompress_block_into(slice, start_bit, end_bit, &mut decomp_buf, scratch)?;
                             result_sender.send((idx, decomp_buf)).unwrap();
                             Ok(())
                        }
                    );
            });
        });

        Self {
            data: data_ref,
            receiver: result_receiver,
            buffer: Vec::new(),
            buffer_pos: 0,
            next_block_idx: 0,
            pending_blocks: HashMap::new(),
        }
    }
}

impl Read for Bz2Decoder {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.buffer_pos < self.buffer.len() {
            let len = std::cmp::min(buf.len(), self.buffer.len() - self.buffer_pos);
            buf[..len].copy_from_slice(&self.buffer[self.buffer_pos..self.buffer_pos + len]);
            self.buffer_pos += len;
            return Ok(len);
        }

        // Buffer empty, get next block
        loop {
            if let Some(block) = self.pending_blocks.remove(&self.next_block_idx) {
                self.buffer = block;
                self.buffer_pos = 0;
                self.next_block_idx += 1;
                return self.read(buf);
            }

            match self.receiver.recv() {
                Ok((idx, block)) => {
                    if idx == self.next_block_idx {
                        self.buffer = block;
                        self.buffer_pos = 0;
                        self.next_block_idx += 1;
                        return self.read(buf);
                    } else {
                        self.pending_blocks.insert(idx, block);
                    }
                }
                Err(_) => {
                    // Channel closed, EOF
                    return Ok(0);
                }
            }
        }
    }
}
