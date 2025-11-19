use anyhow::{Context, Result};
use bzip2::read::BzDecoder;
use clap::Parser;
use crossbeam_channel::bounded;
use memmap2::MmapOptions;
use rayon::prelude::*;
use std::collections::HashMap;
use std::fs::File;
use std::io::{self, Read, Write};
use std::path::PathBuf;
use std::thread;

mod scanner;
mod writer;
mod tuner;

use scanner::find_streams;
use writer::OutputWriter;
use tuner::tune_threads;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Input bzip2 file
    #[arg(short, long)]
    input: PathBuf,

    /// Output file (optional, defaults to stdout)
    #[arg(short, long)]
    output: Option<PathBuf>,

    /// Number of threads for zstd compression (0 = auto)
    #[arg(long, default_value_t = 0)]
    zstd_threads: u32,

    /// Number of threads for decompression (default = num_cpus - 1)
    #[arg(long)]
    decomp_threads: Option<usize>,

    /// Zstd compression level (default = 3)
    #[arg(long, default_value_t = 3)]
    zstd_level: i32,
}

fn main() -> Result<()> {
    let mut args = Args::parse();

    let file = File::open(&args.input).context("Failed to open input file")?;
    let mmap = unsafe { MmapOptions::new().map(&file).context("Failed to mmap input file")? };

    // Auto-tuning
    if args.decomp_threads.is_none() && args.zstd_threads == 0 {
        let (d, c) = tune_threads(&mmap, args.zstd_level);
        args.decomp_threads = Some(d);
        args.zstd_threads = c;
    }

    // Configure rayon
    let num_cpus = std::thread::available_parallelism()?.get();
    let _decomp_threads = args.decomp_threads.unwrap_or_else(|| {
        if num_cpus > 1 { num_cpus - 1 } else { 1 }
    });

    let streams = find_streams(&mmap);
    eprintln!("Found {} streams", streams.len());

    if streams.is_empty() {
        eprintln!("No bzip2 streams found.");
        return Ok(());
    }

    let (sender, receiver) = bounded::<(usize, Vec<u8>)>(rayon::current_num_threads() * 2);

    let writer_handle = thread::spawn(move || -> Result<()> {
        let raw_out: Box<dyn Write + Send> = if let Some(path) = args.output {
            Box::new(File::create(path).context("Failed to create output file")?)
        } else {
            Box::new(io::stdout())
        };

        let mut out = OutputWriter::new(raw_out, args.zstd_level, args.zstd_threads)?;

        let mut buffer: HashMap<usize, Vec<u8>> = HashMap::new();
        let mut next_idx = 0;

        for (idx, data) in receiver {
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

    streams.par_iter().enumerate().try_for_each(|(idx, &(start, end))| -> Result<()> {
        let chunk = &mmap[start..end];
        let mut decoder = BzDecoder::new(chunk);
        let mut decompressed = Vec::new();
        decoder.read_to_end(&mut decompressed).context("Failed to decompress stream")?;
        
        sender.send((idx, decompressed)).context("Failed to send decompressed data")?;
        Ok(())
    })?;

    drop(sender); // Close the channel so the writer knows we're done
    writer_handle.join().unwrap()?;

    Ok(())
}
