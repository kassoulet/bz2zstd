use std::io::Read;
use std::time::Instant;
use bzip2::read::BzDecoder;
use zstd::stream::encode_all;
use crate::scanner::find_streams;

pub fn tune_threads(data: &[u8], zstd_level: i32) -> (usize, u32) {
    let num_cpus = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1);
    
    if num_cpus <= 1 {
        return (1, 0);
    }

    // Find the first stream to use as a sample
    let streams = find_streams(data);
    if streams.is_empty() {
        // Fallback
        return (num_cpus - 1, 1);
    }

    let (start, end) = streams[0];
    let sample_chunk = &data[start..end];

    // 1. Benchmark Decompression
    let start_decomp = Instant::now();
    let mut decoder = BzDecoder::new(sample_chunk);
    let mut decompressed = Vec::new();
    if decoder.read_to_end(&mut decompressed).is_err() {
        // Fallback on error
        return (num_cpus - 1, 1);
    }
    let t_decomp = start_decomp.elapsed().as_secs_f64();

    // 2. Benchmark Compression
    let start_comp = Instant::now();
    // We use encode_all for a quick benchmark of in-memory compression
    if encode_all(&decompressed[..], zstd_level).is_err() {
         return (num_cpus - 1, 1);
    }
    let t_comp = start_comp.elapsed().as_secs_f64();

    // Avoid division by zero
    if t_decomp < 1e-6 || t_comp < 1e-6 {
         return (num_cpus - 1, 1);
    }

    // 3. Calculate Ratio
    // We want D/t_decomp = C/t_comp  => D/C = t_decomp/t_comp
    // D + C = N
    // D = N * t_decomp / (t_decomp + t_comp)
    
    let ratio = t_decomp / (t_decomp + t_comp);
    let d_threads = (num_cpus as f64 * ratio).round() as usize;

    // Ensure at least 1 thread for each if possible, but prioritize decompression if it's super slow
    let d_threads = d_threads.max(1).min(num_cpus - 1);
    let c_threads = (num_cpus - d_threads) as u32;

    eprintln!("Auto-tuning: Decomp time: {:.4}s, Comp time: {:.4}s. Ratio: {:.2}. Threads: D={}, C={}", 
              t_decomp, t_comp, ratio, d_threads, c_threads);

    (d_threads, c_threads)
}
