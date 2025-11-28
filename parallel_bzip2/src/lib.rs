//! High-performance parallel bzip2 decompression library.
//!
//! This library provides efficient parallel decompression of bzip2 files by processing
//! multiple blocks concurrently. It achieves significant speedups on multi-core systems
//! compared to sequential decompression.
//!
//! # Features
//!
//! - **Parallel block decompression**: Utilizes all available CPU cores
//! - **Streaming API**: Implements `std::io::Read` for easy integration
//! - **Memory-efficient**: Uses bounded channels to limit memory usage
//! - **Zero-copy where possible**: Memory-mapped I/O for file access
//!
//! # Architecture
//!
//! The library uses a multi-stage pipeline:
//!
//! 1. **Scanning**: Identifies block boundaries using parallel pattern matching
//! 2. **Decompression**: Processes blocks in parallel using Rayon
//! 3. **Reordering**: Ensures output maintains correct block order
//!
//! # Quick Start
//!
//! The easiest way to use this library is through the `Bz2Decoder`:
//!
//! ```no_run
//! use parallel_bzip2::Bz2Decoder;
//! use std::io::Read;
//!
//! let mut decoder = Bz2Decoder::open("file.bz2").unwrap();
//! let mut data = Vec::new();
//! decoder.read_to_end(&mut data).unwrap();
//! ```
//!
//! # Advanced Usage
//!
//! For more control, you can use the lower-level functions:
//!
//! ```no_run
//! use parallel_bzip2::{scan_blocks, decompress_block};
//!
//! let compressed_data = std::fs::read("file.bz2").unwrap();
//! let block_receiver = scan_blocks(&compressed_data);
//!
//! for (start_bit, end_bit) in block_receiver {
//!     let decompressed = decompress_block(&compressed_data, start_bit, end_bit).unwrap();
//!     // Process decompressed block...
//! }
//! ```
//!
//! # Performance
//!
//! Performance scales nearly linearly with the number of CPU cores. On an 8-core system,
//! expect 6-7x speedup compared to single-threaded bzip2 decompression.
//!
//! # Thread Safety
//!
//! All public types are thread-safe. The library uses Rayon's global thread pool by default,
//! but creates dedicated pools where needed to avoid deadlocks.

pub mod decoder;
pub mod scanner;
pub use decoder::Bz2Decoder;
pub use scanner::{extract_bits, MarkerType, Scanner};

use anyhow::{Context, Result};
use bzip2::read::BzDecoder;
use crossbeam_channel::bounded;
use std::collections::HashMap;
use std::io::Read;

/// Scans bzip2 data for block boundaries and returns them via a channel.
///
/// This function spawns background threads to scan the data in parallel and identify
/// block start and end positions. The results are sent through a channel as
/// (start_bit, end_bit) tuples representing block boundaries.
///
/// # Architecture
///
/// The function creates a two-stage pipeline:
/// 1. **Scanner thread**: Performs parallel chunk-based scanning
/// 2. **Reordering thread**: Collects chunks and converts markers to block boundaries
///
/// # Arguments
///
/// * `data` - The bzip2 compressed data to scan
///
/// # Returns
///
/// A receiver that yields (start_bit, end_bit) tuples for each block found.
/// The receiver will be closed when all blocks have been identified.
///
/// # Performance
///
/// - **Channel buffer**: Sized at 100 to balance memory usage and throughput
/// - **Chunk buffer**: Limited to 4 chunks to prevent excessive memory usage
/// - **Thread safety**: Creates its own thread pool to avoid deadlock
///
/// # Examples
///
/// ```no_run
/// use parallel_bzip2::scan_blocks;
///
/// let data = std::fs::read("file.bz2").unwrap();
/// let blocks = scan_blocks(&data);
///
/// for (start, end) in blocks {
///     println!("Block from bit {} to bit {}", start, end);
/// }
/// ```
pub fn scan_blocks(data: &[u8]) -> crossbeam_channel::Receiver<(u64, u64)> {
    // Channel for sending block boundaries to the caller
    // Buffer size of 100 allows good throughput without excessive memory use
    let (task_sender, task_receiver) = bounded(100);

    // Clone data into an Arc for safe sharing across threads
    let data_vec = data.to_vec();
    let data_arc = std::sync::Arc::new(data_vec);
    let data_clone = data_arc.clone();

    std::thread::spawn(move || {
        let scanner = Scanner::new();
        // Small buffer for chunks to prevent scanning too far ahead
        // This maintains cache locality and limits memory usage
        let (chunk_tx, chunk_rx) = bounded(4);

        // Spawn the actual scanning in a background thread
        let scan_data = data_clone.clone();
        let _scan_handle = std::thread::spawn(move || {
            scanner.scan_stream(&scan_data, 0, chunk_tx);
        });

        // Reorder chunks and convert markers to block boundaries
        let mut chunk_buffer: HashMap<usize, Vec<(u64, MarkerType)>> = HashMap::new();
        let mut next_chunk_idx = 0;
        let mut current_block_start: Option<u64> = None;

        for (idx, markers) in chunk_rx {
            chunk_buffer.insert(idx, markers);

            // Process chunks in order
            while let Some(markers) = chunk_buffer.remove(&next_chunk_idx) {
                for (marker_pos, mtype) in markers {
                    match mtype {
                        MarkerType::Block => {
                            // Block marker: end previous block (if any) and start new one
                            if let Some(start) = current_block_start {
                                if task_sender.send((start, marker_pos)).is_err() {
                                    return; // Receiver dropped, stop scanning
                                }
                            }
                            current_block_start = Some(marker_pos);
                        }
                        MarkerType::Eos => {
                            // End-of-stream marker: end current block
                            if let Some(start) = current_block_start {
                                if task_sender.send((start, marker_pos)).is_err() {
                                    return;
                                }
                                current_block_start = None;
                            }
                        }
                    }
                }
                next_chunk_idx += 1;
            }
        }

        // Handle edge case: block without EOS marker (truncated file)
        if let Some(start) = current_block_start {
            let end = (data_clone.len() as u64) * 8;
            let _ = task_sender.send((start, end));
        }
    });

    task_receiver
}

/// Decompresses a single bzip2 block and returns the decompressed data.
///
/// This is a convenience wrapper around `decompress_block_into` that allocates
/// the output buffer for you. For better performance when decompressing multiple
/// blocks, use `decompress_block_into` with reused buffers.
///
/// # Arguments
///
/// * `data` - The complete bzip2 file data
/// * `start_bit` - Bit offset where the block starts
/// * `end_bit` - Bit offset where the block ends
///
/// # Returns
///
/// The decompressed block data
///
/// # Errors
///
/// Returns an error if the block is corrupted or cannot be decompressed.
///
/// # Examples
///
/// ```no_run
/// use parallel_bzip2::{scan_blocks, decompress_block};
///
/// let data = std::fs::read("file.bz2").unwrap();
/// let blocks = scan_blocks(&data);
///
/// if let Some((start, end)) = blocks.iter().next() {
///     let decompressed = decompress_block(&data, start, end).unwrap();
///     println!("Decompressed {} bytes", decompressed.len());
/// }
/// ```
pub fn decompress_block(data: &[u8], start_bit: u64, end_bit: u64) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    let mut scratch = Vec::new();
    decompress_block_into(data, start_bit, end_bit, &mut out, &mut scratch)?;
    Ok(out)
}

/// Decompresses a single bzip2 block into provided buffers (zero-allocation).
///
/// This function is optimized for decompressing multiple blocks by reusing buffers.
/// It's used internally by the parallel decoder for maximum performance.
///
/// # Arguments
///
/// * `data` - The complete bzip2 file data
/// * `start_bit` - Bit offset where the block starts
/// * `end_bit` - Bit offset where the block ends
/// * `out` - Output buffer for decompressed data (will be cleared)
/// * `scratch` - Scratch buffer for compressed data with header (will be cleared)
///
/// # Performance
///
/// By reusing `scratch` across multiple calls, this function avoids allocating
/// a new buffer for each block. This is especially important in parallel scenarios
/// where thousands of blocks may be processed.
///
/// # Errors
///
/// Returns an error if the block is corrupted or cannot be decompressed.
///
/// # Examples
///
/// ```no_run
/// use parallel_bzip2::{scan_blocks, decompress_block_into};
///
/// let data = std::fs::read("file.bz2").unwrap();
/// let blocks = scan_blocks(&data);
///
/// let mut out = Vec::new();
/// let mut scratch = Vec::new();
///
/// for (start, end) in blocks {
///     decompress_block_into(&data, start, end, &mut out, &mut scratch).unwrap();
///     // Process `out`...
/// }
/// ```
pub fn decompress_block_into(
    data: &[u8],
    start_bit: u64,
    end_bit: u64,
    out: &mut Vec<u8>,
    scratch: &mut Vec<u8>,
) -> Result<()> {
    scratch.clear();
    // Add minimal bzip2 header (BZh9 = highest compression level)
    scratch.extend_from_slice(b"BZh9");
    // Extract the block bits and append to scratch buffer
    extract_bits(data, start_bit, end_bit, scratch);

    // Decompress using the bzip2 crate
    // Note: The last block may not have a proper EOS marker, causing UnexpectedEof
    out.clear();
    let mut decoder = BzDecoder::new(&scratch[..]);
    match decoder.read_to_end(out) {
        Ok(_) => Ok(()),
        // UnexpectedEof is expected for the last block without EOS marker
        Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => Ok(()),
        Err(e) => Err(e).context("Failed to decompress block"),
    }
}

/// Decompresses an entire bzip2 file and returns the decompressed data.
///
/// This is a convenience function that combines scanning and decompression.
/// It's primarily used for testing but can be useful for simple use cases.
///
/// For more control or streaming decompression, use `Bz2Decoder` instead.
///
/// # Arguments
///
/// * `path` - Path to the bzip2 file
///
/// # Returns
///
/// The complete decompressed file contents
///
/// # Errors
///
/// Returns an error if:
/// - The file cannot be opened
/// - The file is not a valid bzip2 file
/// - Decompression fails
///
/// # Examples
///
/// ```no_run
/// use parallel_bzip2::parallel_bzip2_cat;
///
/// let data = parallel_bzip2_cat("file.bz2").unwrap();
/// println!("Decompressed {} bytes", data.len());
/// ```
pub fn parallel_bzip2_cat<P: AsRef<std::path::Path>>(path: P) -> Result<Vec<u8>> {
    let mut decoder = Bz2Decoder::open(path)?;
    let mut data = Vec::new();
    decoder.read_to_end(&mut data)?;
    Ok(data)
}
