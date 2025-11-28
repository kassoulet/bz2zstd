//! bz2zstd - High-performance bzip2 to zstd converter.
//!
//! This application converts bzip2 compressed files to zstd format using parallel
//! decompression and compression. It achieves significant speedups on multi-core
//! systems by processing multiple blocks concurrently.
//!
//! # Architecture
//!
//! The converter uses a three-stage pipeline:
//!
//! 1. **Scanner thread**: Identifies bzip2 block boundaries
//! 2. **Worker pool**: Decompresses bzip2 blocks and compresses to zstd in parallel
//! 3. **Writer thread**: Reorders and writes compressed blocks to output file
//!
//! # Performance
//!
//! - Memory-mapped I/O for efficient file access
//! - Bounded channels prevent excessive memory usage
//! - Per-thread zstd compressors avoid lock contention
//! - Scales linearly with CPU core count
//!
//! # Usage
//!
//! ```bash
//! # Convert with default settings
//! bz2zstd input.bz2
//!
//! # Specify output file and compression level
//! bz2zstd input.bz2 -o output.zst -z 10
//!
//! # Limit thread count
//! bz2zstd input.bz2 -j 4
//! ```

use anyhow::{Context, Result};
use bzip2::read::BzDecoder;
use clap::Parser;
use crossbeam_channel::bounded;
use memmap2::MmapOptions;
use rayon::prelude::*;
use std::collections::HashMap;
use std::fs::File;
use std::io::{Read, Write};
use std::path::PathBuf;
use std::thread;

mod writer;
use parallel_bzip2::{extract_bits, MarkerType, Scanner};
use writer::OutputWriter;

/// Command-line arguments for bz2zstd.
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Input bzip2 file
    input: PathBuf,

    /// Output file (optional, defaults to input file with .bz2 replaced by .zst)
    #[arg(short, long)]
    output: Option<PathBuf>,

    /// Zstd compression level (1-22, default = 3)
    /// Higher values provide better compression but are slower
    #[arg(short = 'z', long, default_value_t = 3)]
    zstd_level: i32,

    /// Number of threads to use (default = number of logical cores)
    #[arg(short = 'j', long)]
    jobs: Option<usize>,

    /// Benchmark mode: Only run the scanner and exit
    /// Useful for measuring scanner performance
    #[arg(long)]
    benchmark_scan: bool,
}

fn main() -> Result<()> {
    let args = Args::parse();

    // Configure global thread pool if user specified thread count
    // This affects all Rayon parallel iterators in the application
    if let Some(jobs) = args.jobs {
        rayon::ThreadPoolBuilder::new()
            .num_threads(jobs)
            .build_global()
            .context("Failed to build global thread pool")?;
    }

    // Memory-map the input file for efficient random access
    // Benefits:
    // - No need to load entire file into memory
    // - OS handles paging and caching
    // - Multiple threads can access without copying
    let file = File::open(&args.input).context("Failed to open input file")?;
    let mmap = unsafe {
        MmapOptions::new()
            .map(&file)
            .context("Failed to mmap input file")?
    };

    // Benchmark mode: measure scanner performance only
    if args.benchmark_scan {
        let start = std::time::Instant::now();
        let scanner = Scanner::new();

        let (tx, rx) = bounded(1000); // Large buffer for benchmark
        let mmap_ref = &mmap;

        // Run scanner and count markers
        thread::scope(|s| {
            s.spawn(move || {
                scanner.scan_stream(mmap_ref, 0, tx);
            });

            let mut count = 0;
            // We don't need to reorder for benchmark, just count
            for (_, markers) in rx {
                count += markers.len();
            }

            let elapsed = start.elapsed();
            println!("Scanned {} markers in {:.2?}", count, elapsed);
            let mb = mmap.len() as f64 / 1024.0 / 1024.0;
            println!("Throughput: {:.2} MB/s", mb / elapsed.as_secs_f64());
        });
        return Ok(());
    }

    // === MAIN PIPELINE SETUP ===
    //
    // Three-stage pipeline:
    // 1. Scanner thread: Finds block boundaries
    // 2. Worker pool: Decompresses bzip2 → compresses zstd
    // 3. Writer thread: Reorders and writes output

    // Channel for block boundaries (start_bit, end_bit)
    // Bounded to prevent scanner from running too far ahead
    // Small buffer maintains cache locality
    let (task_sender, task_receiver) = bounded::<(u64, u64)>(100);

    // Channel for compressed results (block_index, compressed_data)
    // Sized at 2x thread count to allow buffering without excessive memory use
    let (result_sender, result_receiver) =
        bounded::<(usize, Vec<u8>)>(rayon::current_num_threads() * 2);

    // === STAGE 3: WRITER THREAD ===
    //
    // Receives compressed blocks from workers and writes them in order.
    // Uses a HashMap to buffer out-of-order blocks.
    let writer_handle = thread::spawn(move || -> Result<()> {
        // Determine output file path
        let output_path = if let Some(path) = args.output {
            path
        } else {
            // Auto-generate output filename by replacing .bz2 with .zst
            let input_str = args.input.to_string_lossy();
            if input_str.ends_with("bz2") {
                PathBuf::from(input_str.replace("bz2", "zst"))
            } else {
                let mut path = args.input.clone();
                path.set_extension("zst");
                path
            }
        };

        let raw_out: Box<dyn Write + Send> =
            Box::new(File::create(output_path).context("Failed to create output file")?);

        let mut out = OutputWriter::new(raw_out)?;
        // Buffer for out-of-order blocks
        let mut buffer: HashMap<usize, Vec<u8>> = HashMap::new();
        let mut next_idx = 0;

        // Reordering loop: ensure blocks are written in correct order
        for (idx, data) in result_receiver {
            if idx == next_idx {
                // This is the next expected block, write it immediately
                out.write_all(&data)?;
                next_idx += 1;

                // Check if we have subsequent blocks buffered
                while let Some(next_data) = buffer.remove(&next_idx) {
                    out.write_all(&next_data)?;
                    next_idx += 1;
                }
            } else {
                // Out-of-order block, buffer it for later
                buffer.insert(idx, data);
            }
        }
        out.finish()?;
        Ok(())
    });

    // === STAGE 1: SCANNER THREAD ===
    //
    // Scans the bzip2 file for block boundaries and converts markers to block ranges.
    std::thread::scope(|s| {
        let mmap_ref = &mmap;

        s.spawn(move || {
            let scanner = Scanner::new();
            // Small buffer for chunks to prevent scanning too far ahead
            // This maintains cache locality
            let (chunk_tx, chunk_rx) = bounded(4);

            // Spawn the actual scanning in a background thread
            s.spawn(move || {
                scanner.scan_stream(mmap_ref, 0, chunk_tx);
            });

            // Convert markers to block boundaries
            // Markers come as (position, type) where type is Block or Eos
            // We convert these to (start_bit, end_bit) ranges
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
                                        return; // Workers stopped, exit
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
                let end = (mmap_ref.len() as u64) * 8;
                let _ = task_sender.send((start, end));
            }
        });

        // === STAGE 2: WORKER POOL ===
        //
        // Parallel workers that decompress bzip2 blocks and compress to zstd.
        // Each worker has its own decompression buffer and zstd compressor to avoid contention.
        use zstd::bulk::Compressor;
        task_receiver
            .into_iter()
            .enumerate() // Add block index for reordering
            .par_bridge() // Convert to parallel iterator using Rayon
            .try_for_each_init(
                // Per-thread initialization: create buffers and compressor once per thread
                // This avoids lock contention and repeated allocations
                || (Vec::new(), Compressor::new(args.zstd_level).unwrap()),
                |(decomp_buf, compressor), (idx, (start_bit, end_bit))| -> Result<()> {
                    // Extract the compressed bzip2 block bits
                    let mut block_data = Vec::new();
                    extract_bits(&mmap, start_bit, end_bit, &mut block_data);

                    // Wrap with bzip2 header (BZh9 = highest compression level)
                    let mut wrapped_data = Vec::with_capacity(4 + block_data.len());
                    wrapped_data.extend_from_slice(b"BZh9");
                    wrapped_data.append(&mut block_data);

                    // Decompress the bzip2 block
                    // Note: Last block may not have EOS marker, causing UnexpectedEof
                    decomp_buf.clear();
                    let mut decoder = BzDecoder::new(&wrapped_data[..]);
                    match decoder.read_to_end(decomp_buf) {
                        Ok(_) => {}
                        // Expected for last block without EOS marker
                        Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => {}
                        Err(e) => return Err(e).context("Failed to decompress block"),
                    }

                    // Compress to zstd using per-thread compressor
                    let compressed = compressor
                        .compress(decomp_buf)
                        .context("Failed to compress chunk")?;

                    // Send to writer thread with block index for reordering
                    result_sender
                        .send((idx, compressed))
                        .context("Failed to send compressed data")?;
                    Ok(())
                },
            )?;

        Ok::<(), anyhow::Error>(())
    })?;

    drop(result_sender);
    writer_handle.join().unwrap()?;

    Ok(())
}
