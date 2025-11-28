//! Parallel bzip2 decoder with streaming output.
//!
//! This module provides a high-performance decoder that processes bzip2 blocks in parallel
//! while maintaining correct output ordering. It implements the `Read` trait for seamless
//! integration with Rust's I/O ecosystem.
//!
//! # Architecture
//!
//! The decoder uses a three-stage pipeline:
//! 1. **Scanner thread**: Identifies block boundaries in the compressed data
//! 2. **Worker pool**: Decompresses blocks in parallel using Rayon
//! 3. **Reordering**: Ensures decompressed blocks are returned in the correct order
//!
//! # Performance
//!
//! - Parallel decompression scales with available CPU cores
//! - Memory-mapped I/O for efficient file access
//! - Bounded channels prevent excessive memory usage
//! - Zero-copy design where possible
//!
//! # Example
//!
//! ```no_run
//! use parallel_bzip2::Bz2Decoder;
//! use std::io::Read;
//!
//! let mut decoder = Bz2Decoder::open("file.bz2").unwrap();
//! let mut data = Vec::new();
//! decoder.read_to_end(&mut data).unwrap();
//! ```

use crossbeam_channel::{bounded, Receiver};
use std::collections::HashMap;
use std::io::{self, Read};
use std::sync::Arc;

use crate::{decompress_block_into, scan_blocks};

/// Parallel bzip2 decoder implementing the `Read` trait.
///
/// This decoder processes bzip2 blocks in parallel while maintaining correct output
/// ordering. It uses a background thread pool for decompression and buffers results
/// to provide smooth streaming reads.
///
/// # Thread Safety
///
/// The decoder spawns background threads for scanning and decompression. These threads
/// are automatically cleaned up when the decoder is dropped (channels are closed).
///
/// # Memory Management
///
/// - Bounded channels limit memory usage even with fast decompression
/// - The `data` field keeps the source data alive for the lifetime of the decoder
/// - Pending blocks are buffered in a HashMap for reordering
pub struct Bz2Decoder {
    /// Source data kept alive for the decoder's lifetime.
    /// The `#[allow(dead_code)]` is intentional - this field ensures the data
    /// remains valid while background threads access it.
    #[allow(dead_code)]
    data: Arc<dyn AsRef<[u8]> + Send + Sync>,
    /// Channel receiving decompressed blocks: (block_index, decompressed_data)
    receiver: Receiver<(usize, Vec<u8>)>,
    /// Current buffer being read from
    buffer: Vec<u8>,
    /// Position within the current buffer
    buffer_pos: usize,
    /// Index of the next block we expect to read
    next_block_idx: usize,
    /// Out-of-order blocks waiting to be read
    pending_blocks: HashMap<usize, Vec<u8>>,
}

impl Bz2Decoder {
    /// Opens a bzip2 file and creates a decoder using memory-mapped I/O.
    ///
    /// This is the recommended way to create a decoder for files, as it provides
    /// efficient access to the compressed data without loading it entirely into memory.
    ///
    /// # Arguments
    ///
    /// * `path` - Path to the bzip2 file
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The file cannot be opened
    /// - Memory mapping fails (e.g., insufficient address space)
    ///
    /// # Safety
    ///
    /// Uses `unsafe` for memory mapping, but this is safe because:
    /// - The file is opened read-only
    /// - The mmap is kept alive via Arc for the decoder's lifetime
    /// - No concurrent modifications to the file are expected
    pub fn open<P: AsRef<std::path::Path>>(path: P) -> anyhow::Result<Self> {
        let file = std::fs::File::open(path)?;
        let mmap = unsafe { memmap2::MmapOptions::new().map(&file)? };
        Ok(Self::new(Arc::new(mmap)))
    }

    /// Creates a new decoder from any data source.
    ///
    /// This constructor spawns background threads for scanning and decompression,
    /// then immediately returns. The decoder will stream decompressed data as it
    /// becomes available.
    ///
    /// # Architecture
    ///
    /// The constructor sets up a three-stage pipeline:
    ///
    /// 1. **Driver thread**: Coordinates scanning and decompression
    ///    - Calls `scan_blocks()` to get block boundaries
    ///    - Feeds blocks to the worker pool via `par_bridge()`
    ///
    /// 2. **Scanner thread** (inside `scan_blocks()`):
    ///    - Scans data in parallel chunks
    ///    - Sends block boundaries to the driver
    ///
    /// 3. **Worker pool** (Rayon global pool):
    ///    - Decompresses blocks in parallel
    ///    - Sends results with block indices for reordering
    ///
    /// # Channel Sizing
    ///
    /// The result channel is sized at `num_threads * 2` to:
    /// - Prevent blocking workers when decompression is faster than reading
    /// - Limit memory usage (each block can be several MB decompressed)
    /// - Maintain good throughput without excessive buffering
    ///
    /// # Arguments
    ///
    /// * `data` - Arc-wrapped data source (e.g., Vec, mmap, etc.)
    ///
    /// # Type Parameters
    ///
    /// * `T` - Any type that can be converted to a byte slice and is thread-safe
    pub fn new<T>(data: Arc<T>) -> Self
    where
        T: AsRef<[u8]> + Send + Sync + 'static,
    {
        // Channel for sending decompressed blocks back to the reader
        // Sized at 2x thread count to allow some buffering without excessive memory use
        let (result_sender, result_receiver) = bounded(rayon::current_num_threads() * 2);
        let data_ref: Arc<dyn AsRef<[u8]> + Send + Sync> = data;
        let data_clone = data_ref.clone();

        // Spawn the driver thread that coordinates scanning and decompression
        std::thread::spawn(move || {
            let slice = data_clone.as_ref().as_ref();
            // Get block boundaries from the scanner
            let task_receiver = scan_blocks(slice);

            // Parallel decompression using Rayon
            // par_bridge() allows us to process an iterator in parallel
            use rayon::prelude::*;
            let _ = task_receiver
                .into_iter()
                .enumerate() // Add block index for reordering
                .par_bridge() // Convert to parallel iterator
                .try_for_each_init(
                    Vec::new, // Thread-local scratch buffer (avoids allocations)
                    |scratch, (idx, (start_bit, end_bit))| -> anyhow::Result<()> {
                        let mut decomp_buf = Vec::new();
                        // Decompress this block
                        decompress_block_into(slice, start_bit, end_bit, &mut decomp_buf, scratch)?;
                        // Send result with index for reordering
                        result_sender.send((idx, decomp_buf)).unwrap();
                        Ok(())
                    },
                );
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
    /// Reads decompressed data into the provided buffer.
    ///
    /// This implementation handles the complexity of parallel decompression while
    /// maintaining correct block ordering. Blocks may arrive out of order from the
    /// worker pool, so we buffer them until we can return them sequentially.
    ///
    /// # Algorithm
    ///
    /// 1. If we have buffered data, return it immediately
    /// 2. Otherwise, try to get the next expected block from pending blocks
    /// 3. If not available, receive blocks from the channel until we get the right one
    /// 4. Buffer out-of-order blocks for later
    /// 5. Recursively call read() to actually copy data to the caller's buffer
    ///
    /// # Returns
    ///
    /// - `Ok(n)` where n > 0: Successfully read n bytes
    /// - `Ok(0)`: End of stream (all blocks decompressed)
    /// - `Err(e)`: I/O error (should not happen in normal operation)
    ///
    /// # Performance
    ///
    /// The recursive call at the end is optimized away by the compiler (tail call).
    /// The HashMap lookup for pending blocks is O(1) average case.
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        // Fast path: return buffered data if available
        if self.buffer_pos < self.buffer.len() {
            let len = std::cmp::min(buf.len(), self.buffer.len() - self.buffer_pos);
            buf[..len].copy_from_slice(&self.buffer[self.buffer_pos..self.buffer_pos + len]);
            self.buffer_pos += len;
            return Ok(len);
        }

        // Buffer empty, need to get the next block
        loop {
            // Check if we have the next expected block in pending blocks
            if let Some(block) = self.pending_blocks.remove(&self.next_block_idx) {
                self.buffer = block;
                self.buffer_pos = 0;
                self.next_block_idx += 1;
                // Tail recursion: actually copy data to caller's buffer
                return self.read(buf);
            }

            // Receive blocks from the channel
            match self.receiver.recv() {
                Ok((idx, block)) => {
                    if idx == self.next_block_idx {
                        // This is the block we're waiting for
                        self.buffer = block;
                        self.buffer_pos = 0;
                        self.next_block_idx += 1;
                        return self.read(buf);
                    } else {
                        // Out-of-order block, buffer it for later
                        self.pending_blocks.insert(idx, block);
                    }
                }
                Err(_) => {
                    // Channel closed, all blocks have been processed
                    return Ok(0);
                }
            }
        }
    }
}
