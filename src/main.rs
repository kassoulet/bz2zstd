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

mod scanner;
mod writer;
use scanner::find_streams;
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
    #[arg(long, default_value_t = 3)]
    zstd_level: i32,

    /// Limit the size of the input file to scan for streams (in bytes).
    /// Useful for huge single-stream files to avoid OOM.
    #[arg(long)]
    scan_limit: Option<usize>,
}

fn main() -> Result<()> {
    let args = Args::parse();

    let file = File::open(&args.input).context("Failed to open input file")?;
    let mmap = unsafe {
        MmapOptions::new()
            .map(&file)
            .context("Failed to mmap input file")?
    };

    let scan_limit = args.scan_limit.unwrap_or(mmap.len());
    let scan_limit = std::cmp::min(scan_limit, mmap.len());
    let streams = find_streams(&mmap[..scan_limit]);
    eprintln!("Found {} streams", streams.len());

    if streams.is_empty() {
        eprintln!("No bzip2 streams found.");
        return Ok(());
    }

    // Channel for sending compressed chunks to the writer
    // We use a bounded channel to avoid using too much memory if the writer is slow
    let (sender, receiver) = bounded::<(usize, Vec<u8>)>(rayon::current_num_threads() * 2);

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

        // Writer now just writes the chunks it receives
        let mut out = OutputWriter::new(raw_out)?;

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

    // Parallel processing: Decompress -> Compress
    use zstd::bulk::Compressor;

    // Parallel processing: Decompress -> Compress
    streams.par_iter().enumerate().try_for_each_init(
        || (Vec::new(), Compressor::new(args.zstd_level).unwrap()),
        |(decomp_buf, compressor), (idx, &(start, end))| -> Result<()> {
            let chunk = &mmap[start..end];

            // 1. Decompress
            decomp_buf.clear();
            let mut decoder = BzDecoder::new(chunk);
            decoder
                .read_to_end(decomp_buf)
                .context("Failed to decompress stream")?;

            // 2. Compress (Independent Zstd Frame)
            // Reuse the compressor context.
            // Note: This creates a full Zstd frame for each chunk.
            let compressed = compressor
                .compress(decomp_buf)
                .context("Failed to compress chunk")?;

            sender
                .send((idx, compressed))
                .context("Failed to send compressed data")?;
            Ok(())
        },
    )?;

    drop(sender); // Close the channel so the writer knows we're done
    writer_handle.join().unwrap()?;

    Ok(())
}
