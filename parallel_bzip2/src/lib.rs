pub mod scanner;
pub mod decoder;
pub use scanner::{extract_bits, MarkerType, Scanner};
pub use decoder::Bz2Decoder;

use anyhow::{Context, Result};
use bzip2::read::BzDecoder;
use crossbeam_channel::bounded;
use std::collections::HashMap;
use std::io::Read;
use rayon::Scope;

/// Spawns a scanner thread within the given scope and returns a receiver for block locations (start_bit, end_bit).
pub fn scan_blocks<'scope, 'env: 'scope>(
    scope: &Scope<'scope>,
    data: &'env [u8],
    pool: &'env rayon::ThreadPool,
) -> crossbeam_channel::Receiver<(u64, u64)> {
    let (task_sender, task_receiver) = bounded(100);

    scope.spawn(move |s| {
        let scanner = Scanner::new();
        // Small buffer for chunks to prevent scanning too far ahead (cache thrashing)
        let (chunk_tx, chunk_rx) = bounded(4);

        // Spawn the actual scanning in a background thread
        s.spawn(move |_| {
            scanner.scan_stream(data, 0, pool, chunk_tx);
        });

        // Consume and reorder
        let mut chunk_buffer: HashMap<usize, Vec<(u64, MarkerType)>> = HashMap::new();
        let mut next_chunk_idx = 0;
        let mut current_block_start: Option<u64> = None;

        for (idx, markers) in chunk_rx {
            chunk_buffer.insert(idx, markers);

            while let Some(markers) = chunk_buffer.remove(&next_chunk_idx) {
                for (marker_pos, mtype) in markers {
                    match mtype {
                        MarkerType::Block => {
                            if let Some(start) = current_block_start {
                                if task_sender.send((start, marker_pos)).is_err() {
                                    return;
                                }
                            }
                            current_block_start = Some(marker_pos);
                        }
                        MarkerType::Eos => {
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

        // If we have a dangling block start (EOF reached without EOS marker?)
        if let Some(start) = current_block_start {
            let end = (data.len() as u64) * 8;
            let _ = task_sender.send((start, end));
        }
    });

    task_receiver
}

/// Decompresses a single bzip2 block extracted from the bitstream.
pub fn decompress_block(data: &[u8], start_bit: u64, end_bit: u64) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    let mut scratch = Vec::new();
    decompress_block_into(data, start_bit, end_bit, &mut out, &mut scratch)?;
    Ok(out)
}

/// Decompresses a single bzip2 block into the provided buffer.
/// `scratch` is used for the intermediate compressed data (with header).
pub fn decompress_block_into(
    data: &[u8],
    start_bit: u64,
    end_bit: u64,
    out: &mut Vec<u8>,
    scratch: &mut Vec<u8>,
) -> Result<()> {
    scratch.clear();
    scratch.extend_from_slice(b"BZh9"); // Minimal header
    extract_bits(data, start_bit, end_bit, scratch);

    // Decompress, handling potential UnexpectedEof for the last block
    out.clear();
    let mut decoder = BzDecoder::new(&scratch[..]);
    match decoder.read_to_end(out) {
        Ok(_) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => Ok(()),
        Err(e) => Err(e).context("Failed to decompress block"),
    }
}
