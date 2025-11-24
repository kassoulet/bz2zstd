use std::io::{self, Read};
use std::sync::Arc;
use std::collections::HashMap;
use crossbeam_channel::{bounded, Receiver};
use rayon::ThreadPoolBuilder;

use crate::{scan_blocks, decompress_block_into};

pub struct Bz2Decoder {
    data: Arc<[u8]>,
    receiver: Receiver<(usize, Vec<u8>)>,
    buffer: Vec<u8>,
    buffer_pos: usize,
    next_block_idx: usize,
    pending_blocks: HashMap<usize, Vec<u8>>,
}

impl Bz2Decoder {
    pub fn new(data: Arc<[u8]>) -> Self {
        let (result_sender, result_receiver) = bounded(rayon::current_num_threads() * 2);
        let data_clone = data.clone();

        // We spawn a thread to drive the scanning and decompression.
        // This thread will spawn the scanner and then the workers.
        std::thread::spawn(move || {
            let pool = ThreadPoolBuilder::new()
                .num_threads(rayon::current_num_threads())
                .build()
                .unwrap();

            // We need a scope for scan_blocks, but we are in a static thread now thanks to Arc.
            // However, scan_blocks expects a scope.
            // We can refactor scan_blocks or just use a scope here.
            pool.scope(|s| {
                let task_receiver = scan_blocks(s, &data_clone, &pool);

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
                             decompress_block_into(&data_clone, start_bit, end_bit, &mut decomp_buf, scratch)?;
                             result_sender.send((idx, decomp_buf)).unwrap();
                             Ok(())
                        }
                    );
            });
        });

        Self {
            data,
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
