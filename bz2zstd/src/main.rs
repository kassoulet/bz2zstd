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
use clap::Parser;
use crossbeam_channel::bounded;
use indicatif::{ProgressBar, ProgressStyle};
use memmap2::MmapOptions;
use rayon::prelude::*;
use std::collections::HashMap;
use std::fs::File;
use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering::Relaxed};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

mod writer;
use parallel_bzip2_decoder::{decompress_block_into, MarkerType, Scanner, Throttle};
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

    /// Zstd compression level (-7 to 22, default = 9)
    /// Higher values provide better compression but are slower.
    /// Negative values provide faster but less efficient compression.
    #[arg(short = 'z', long, default_value_t = 9, value_parser = clap::value_parser!(i32).range(-7..=22))]
    zstd_level: i32,

    /// Number of threads to use (default = number of logical cores)
    #[arg(short = 'j', long)]
    jobs: Option<usize>,

    /// Benchmark mode: Only run the scanner and exit
    /// Useful for measuring scanner performance
    #[arg(long)]
    benchmark_scan: bool,

    /// Print details about the input (bzip2 level, block/stream counts), the parallel
    /// setup, and a summary with throughput and peak memory (to stderr)
    #[arg(short, long)]
    verbose: bool,
}

/// Counters filled in by the pipeline, reported by `--verbose`.
#[derive(Default)]
struct Stats {
    streams: AtomicU64,
    blocks: AtomicU64,
    decompressed: AtomicU64,
    compressed: AtomicU64,
    max_block: AtomicU64,
}

/// Peak resident memory (VmHWM) in MB, from /proc (Linux only).
fn peak_rss_mb() -> Option<u64> {
    let status = std::fs::read_to_string("/proc/self/status").ok()?;
    let kb: u64 = status
        .lines()
        .find_map(|l| l.strip_prefix("VmHWM:"))?
        .trim()
        .trim_end_matches("kB")
        .trim()
        .parse()
        .ok()?;
    Some(kb / 1024)
}

fn main() -> Result<()> {
    let args = Args::parse();

    // Pre-validate zstd compression level to ensure it's supported
    // This prevents a panic in the worker threads later
    zstd::bulk::Compressor::new(args.zstd_level).context("Invalid zstd compression level")?;

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

    // Progress over input bytes handed to the workers. Hidden automatically when
    // stderr is not a terminal (pipes, tests).
    let progress = ProgressBar::new(mmap.len() as u64);
    progress.set_style(
        ProgressStyle::with_template(
            "{wide_bar} {percent:>3}% | {bytes}/{total_bytes} | {bytes_per_sec} | ETA {eta_precise}",
        )
        .expect("static progress template is valid"),
    );
    progress.enable_steady_tick(Duration::from_millis(100));

    // Channel for block boundaries (start_bit, end_bit)
    // Bounded to prevent scanner from running too far ahead
    // Small buffer maintains cache locality
    // 2 per thread keeps workers fed while limiting how many input pages the scanner touches ahead.
    let (task_sender, task_receiver) = bounded::<(u64, u64)>(rayon::current_num_threads() * 2);

    // Channel for compressed results (block_index, compressed_data)
    // Sized at 2x thread count to allow buffering without excessive memory use
    let (result_sender, result_receiver) =
        bounded::<(usize, Vec<u8>)>(rayon::current_num_threads() * 2);

    // Determine output file path
    let output_path = if let Some(path) = &args.output {
        path.clone()
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

    // Security Check: Verify that input and output file paths are distinct.
    // This prevents a 'Bus error' (SIGBUS) caused by truncating a file that is
    // currently memory-mapped as input.
    //
    // We check both:
    // 1. Canonical paths (detects same file via different paths/symlinks)
    // 2. Device/Inode (detects same file via hard links on Unix)
    let is_same_file = (|| -> Result<bool> {
        let input_canon = std::fs::canonicalize(&args.input);
        let output_canon = std::fs::canonicalize(&output_path);

        if let (Ok(in_c), Ok(out_c)) = (input_canon, output_canon) {
            if in_c == out_c {
                return Ok(true);
            }
        }

        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            let input_meta = std::fs::metadata(&args.input);
            let output_meta = std::fs::metadata(&output_path);
            if let (Ok(in_m), Ok(out_m)) = (input_meta, output_meta) {
                if in_m.dev() == out_m.dev() && in_m.ino() == out_m.ino() {
                    return Ok(true);
                }
            }
        }

        Ok(false)
    })()?;

    if is_same_file {
        return Err(anyhow::anyhow!(
            "Input and output files must be different: {:?}",
            output_path
        ));
    }

    let started = Instant::now();
    let stats = Stats::default();
    let stats = &stats;
    if args.verbose {
        // The stream header is "BZh" + level '1'..'9'; block size is level * 100 kB.
        let level = match mmap.get(..4) {
            Some([b'B', b'Z', b'h', l @ b'1'..=b'9']) => Some(u32::from(l - b'0')),
            _ => None,
        };
        let threads = rayon::current_num_threads();
        eprintln!(
            "Input:    {} ({:.1} MiB)",
            args.input.display(),
            mmap.len() as f64 / 1048576.0
        );
        match level {
            Some(l) => eprintln!(
                "bzip2:    level {} (block size {} kB, first stream)",
                l,
                l * 100
            ),
            None => eprintln!("bzip2:    no valid 'BZh' header at start of file"),
        }
        eprintln!(
            "Mode:     parallel, {} worker threads + {} scanner threads, memory-mapped input",
            threads, threads
        );
        eprintln!(
            "Queues:   {} pending block ranges, writer window {} blocks",
            threads * 2,
            threads * 4
        );
        eprintln!(
            "Output:   {} (zstd level {})",
            output_path.display(),
            args.zstd_level
        );
    }

    // === STAGE 3: WRITER THREAD ===
    //
    // Receives compressed blocks from workers and writes them in order.
    // Uses a HashMap to buffer out-of-order blocks.
    let writer_output_path = output_path.clone();
    // Keeps workers within a window of the writer so `buffer` below stays bounded.
    let throttle = Arc::new(Throttle::new(rayon::current_num_threads() * 4));
    let writer_throttle = throttle.clone();
    let writer_handle = thread::spawn(move || -> Result<()> {
        let res = (|| -> Result<()> {
            let raw_out: Box<dyn Write + Send> =
                Box::new(File::create(writer_output_path).context("Failed to create output file")?);

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
                    writer_throttle.set_next(next_idx);
                } else {
                    // Out-of-order block, buffer it for later
                    buffer.insert(idx, data);
                }
            }
            out.finish()?;
            Ok(())
        })();
        // Writer is done (or failed): never leave workers blocked on backpressure.
        writer_throttle.release();
        res
    });

    // === STAGE 1: SCANNER THREAD ===
    //
    // Scans the bzip2 file for block boundaries and converts markers to block ranges.
    std::thread::scope(|s| {
        let mmap_ref = &mmap;
        let progress = progress.clone();

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
                                stats.streams.fetch_add(1, Relaxed);
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
                eprintln!("Warning: Input stream appears to be truncated - found block without EOS marker");
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
                || {
                    (
                        Vec::new(),
                        Vec::new(),
                        Compressor::new(args.zstd_level)
                            .expect("Compression level was pre-validated"),
                    )
                },
                |(decomp_buf, scratch, compressor), (idx, (start_bit, end_bit))| -> Result<()> {
                    let res = (|| -> Result<()> {
                        // Shared with the library: enforces the compressed/decompressed size
                        // limits and verifies the block CRC (detects corruption/truncation).
                        decompress_block_into(&mmap, start_bit, end_bit, decomp_buf, scratch)
                            .with_context(|| format!("Failed to decompress block {}", idx))?;

                        stats.blocks.fetch_add(1, Relaxed);
                        stats
                            .decompressed
                            .fetch_add(decomp_buf.len() as u64, Relaxed);
                        stats.max_block.fetch_max(decomp_buf.len() as u64, Relaxed);

                        // Count consumed input bytes once the block decoded successfully.
                        progress.inc((end_bit - start_bit + 7) / 8);

                        // This block's input is no longer needed: drop its pages from the
                        // page cache mapping so RSS doesn't grow to the size of the input.
                        // Only whole pages inside the block are released (edges are shared
                        // with neighbouring blocks); errors are ignored, it's just a hint.
                        // ponytail: assumes 4 KiB pages; on larger pages the call fails harmlessly.
                        #[cfg(unix)]
                        {
                            const PAGE: u64 = 4096;
                            let from = (start_bit / 8 + PAGE - 1) / PAGE * PAGE;
                            let to = (end_bit / 8) / PAGE * PAGE;
                            if to > from {
                                let _ = mmap.advise_range(
                                    memmap2::Advice::DontNeed,
                                    from as usize,
                                    (to - from) as usize,
                                );
                            }
                        }

                        // Compress to zstd using per-thread compressor
                        let compressed = compressor
                            .compress(decomp_buf)
                            .context("Failed to compress chunk")?;

                        stats.compressed.fetch_add(compressed.len() as u64, Relaxed);

                        // Don't run more than a window ahead of the writer
                        throttle.wait(idx);

                        // Send to writer thread with block index for reordering
                        result_sender
                            .send((idx, compressed))
                            .context("Failed to send compressed data")?;
                        Ok(())
                    })();
                    // A failed block will never reach the writer: release workers waiting
                    // on it so the pipeline can shut down instead of deadlocking.
                    if res.is_err() {
                        throttle.release();
                    }
                    res
                },
            )?;

        Ok::<(), anyhow::Error>(())
    })?;

    drop(result_sender);
    writer_handle.join().unwrap()?;
    // Block ranges exclude the stream header/footer, so snap to 100% at the end.
    progress.finish();

    if args.verbose {
        let secs = started.elapsed().as_secs_f64();
        let blocks = stats.blocks.load(Relaxed);
        let dec = stats.decompressed.load(Relaxed);
        let comp = stats.compressed.load(Relaxed);
        let mib = |b: u64| b as f64 / 1048576.0;
        eprintln!("Streams:  {}", stats.streams.load(Relaxed));
        eprintln!(
            "Blocks:   {} (avg {:.0} kB, max {:.0} kB decompressed)",
            blocks,
            dec as f64 / blocks.max(1) as f64 / 1024.0,
            stats.max_block.load(Relaxed) as f64 / 1024.0
        );
        eprintln!(
            "Data:     {:.1} MiB bzip2 -> {:.1} MiB raw -> {:.1} MiB zstd (zstd/raw {:.1}%)",
            mib(mmap.len() as u64),
            mib(dec),
            mib(comp),
            comp as f64 * 100.0 / dec.max(1) as f64
        );
        eprintln!(
            "Time:     {:.1}s ({:.1} MiB/s bzip2 in, {:.1} MiB/s raw out)",
            secs,
            mib(mmap.len() as u64) / secs,
            mib(dec) / secs
        );
        if let Some(mb) = peak_rss_mb() {
            eprintln!("Memory:   peak RSS {} MB", mb);
        }
    }

    Ok(())
}
