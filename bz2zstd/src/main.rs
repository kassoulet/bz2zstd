use anyhow::{Context, Result};
use clap::Parser;
use crossbeam_channel::bounded;
use indicatif::{ProgressBar, ProgressDrawTarget, ProgressStyle};
use memmap2::MmapOptions;
use rayon::prelude::*;
use std::collections::HashMap;
use std::fs::File;
use std::io::Write;
use std::path::PathBuf;
use std::thread;

mod writer;
use parallel_bzip2::{decompress_block_into, scan_blocks, Scanner};
use writer::OutputWriter;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Input bzip2 file
    input: PathBuf,

    /// Output file (optional, defaults to input file with .bz2 replaced by .zst)
    #[arg(short, long)]
    output: Option<PathBuf>,

    /// Zstd compression level (default = 3)
    #[arg(short = 'z', long, default_value_t = 3)]
    zstd_level: i32,

    /// Number of threads to use (default = number of logical cores)
    #[arg(short = 'j', long)]
    jobs: Option<usize>,

    /// Benchmark mode: Only run the scanner and exit
    #[arg(long)]
    benchmark_scan: bool,
}

fn main() -> Result<()> {
    let args = Args::parse();

    if let Some(jobs) = args.jobs {
        rayon::ThreadPoolBuilder::new()
            .num_threads(jobs)
            .build_global()
            .context("Failed to build global thread pool")?;
    }

    let file = File::open(&args.input).context("Failed to open input file")?;
    let mmap = unsafe {
        MmapOptions::new()
            .map(&file)
            .context("Failed to mmap input file")?
    };

    // Create a separate thread pool for the scanner to avoid starvation/deadlock
    // with the worker pool (global) when using par_bridge.
    let scanner_threads = rayon::current_num_threads();
    let scanner_pool = rayon::ThreadPoolBuilder::new()
        .num_threads(scanner_threads)
        .build()
        .context("Failed to create scanner thread pool")?;

    if args.benchmark_scan {
        let start = std::time::Instant::now();
        let scanner = Scanner::new();

        let (tx, rx) = bounded(1000); // Buffer for chunk results
        let pool_ref = &scanner_pool;
        let mmap_ref = &mmap;

        // Spawn scanner in background
        rayon::scope(|s| {
            s.spawn(move |_| {
                scanner.scan_stream(mmap_ref, 0, pool_ref, tx);
            });

            let mut count = 0;
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

    let (result_sender, result_receiver) =
        bounded::<(usize, Vec<u8>)>(rayon::current_num_threads() * 2);

    let writer_handle = thread::spawn(move || -> Result<()> {
        let output_path = if let Some(path) = args.output {
            path
        } else {
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
        let mut buffer: HashMap<usize, Vec<u8>> = HashMap::new();
        let mut next_idx = 0;

        for (idx, data) in result_receiver {
            if idx == next_idx {
                out.write_all(&data)?;
                next_idx += 1;

                while let Some(next_data) = buffer.remove(&next_idx) {
                    out.write_all(&next_data)?;
                    next_idx += 1;
                }
            } else {
                buffer.insert(idx, data);
            }
        }
        out.finish()?;
        Ok(())
    });

    rayon::scope(|s| {
        let mmap_ref = &mmap;
        let scanner_pool_ref = &scanner_pool;

        // Scanner Thread (managed by pbz2)
        let task_receiver = scan_blocks(s, mmap_ref, scanner_pool_ref);

        // Worker Pool
        use zstd::bulk::Compressor;

        let pb = ProgressBar::new(mmap.len() as u64);
        pb.set_draw_target(ProgressDrawTarget::stderr_with_hz(5));
        pb.set_style(ProgressStyle::with_template("{spinner:.green} [{elapsed_precise}] [{wide_bar:.cyan/blue}] {bytes}/{total_bytes} ({eta})")
            .unwrap());

        let pb_ref = &pb;

        task_receiver
            .into_iter()
            .enumerate()
            .par_bridge()
            .try_for_each_init(
                || {
                    (
                        Vec::new(),
                        Compressor::new(args.zstd_level).unwrap(),
                        Vec::new(),
                    )
                },
                |(decomp_buf, compressor, wrapped_data),
                 (idx, (start_bit, end_bit))|
                 -> Result<()> {
                    decompress_block_into(&mmap, start_bit, end_bit, decomp_buf, wrapped_data)
                        .context("Failed to decompress block")?;

                    // Compress to Zstd
                    let compressed = compressor
                        .compress(decomp_buf)
                        .context("Failed to compress chunk")?;

                    result_sender
                        .send((idx, compressed))
                        .context("Failed to send compressed data")?;

                    // Update progress bar
                    let input_bits = end_bit - start_bit;
                    pb_ref.inc(input_bits / 8);

                    Ok(())
                },
            )?;

        pb.finish_with_message("Done!");

        Ok::<(), anyhow::Error>(())
    })?;

    drop(result_sender);
    writer_handle.join().unwrap()?;

    Ok(())
}
