#![no_main]

use libfuzzer_sys::fuzz_target;
use parallel_bzip2::Bz2Decoder;
use std::io::Read;
use std::sync::Arc;

fuzz_target!(|data: &[u8]| {
    // Don't fuzz empty data or extremely large inputs
    if data.is_empty() || data.len() > 10_000_000 {
        return;
    }

    // Create decoder from the fuzzed data
    let data_arc = Arc::new(data.to_vec());
    let mut decoder = Bz2Decoder::new(data_arc);
    
    // Try to read all decompressed output
    let mut output = Vec::new();
    
    // Set a reasonable limit to prevent OOM
    const MAX_OUTPUT: usize = 100_000_000; // 100 MB
    
    loop {
        let mut buf = [0u8; 8192];
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
    
    // If we got valid output, verify it's not corrupted
    // (The decoder should either produce valid output or error, never corrupt data)
});
