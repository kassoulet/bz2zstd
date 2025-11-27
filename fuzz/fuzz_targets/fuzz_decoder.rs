#![no_main]

use libfuzzer_sys::fuzz_target;
use parallel_bzip2::Bz2Decoder;
use std::io::Read;
use std::sync::Arc;
use std::time::{Duration, Instant};

fuzz_target!(|data: &[u8]| {
    // Much stricter input size limit to prevent OOM from data.to_vec()
    if data.is_empty() || data.len() > 1_000_000 {
        return;
    }

    // Create decoder from the fuzzed data
    let data_arc = Arc::new(data.to_vec());
    let mut decoder = Bz2Decoder::new(data_arc);
    
    // Set a timeout to prevent hanging
    let start = Instant::now();
    let timeout = Duration::from_secs(1);
    
    // Try to read decompressed output with strict limits
    let mut output = Vec::new();
    
    // Much lower limit to prevent OOM - 10 MB instead of 100 MB
    const MAX_OUTPUT: usize = 10_000_000;
    
    loop {
        // Check timeout
        if start.elapsed() > timeout {
            break;
        }
        
        // Use smaller buffer to limit memory growth
        let mut buf = [0u8; 4096];
        match decoder.read(&mut buf) {
            Ok(0) => break, // EOF
            Ok(n) => {
                output.extend_from_slice(&buf[..n]);
                if output.len() > MAX_OUTPUT {
                    break;
                }
            }
            Err(_) => break, // Expected for invalid input
        }
    }
});
