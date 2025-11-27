#![no_main]

use libfuzzer_sys::fuzz_target;
use parallel_bzip2::scan_blocks;
use std::time::{Duration, Instant};

fuzz_target!(|data: &[u8]| {
    // Stricter input size limit to prevent OOM from data.to_vec() in scan_blocks
    if data.is_empty() || data.len() > 1_000_000 {
        return;
    }

    // Call scan_blocks and consume the results with timeout
    let receiver = scan_blocks(data);
    
    // Set a timeout to prevent hanging on pathological inputs
    let start = Instant::now();
    let timeout = Duration::from_secs(1);
    
    // Collect detected blocks with strict limits
    let mut blocks = Vec::new();
    while let Ok((start_bit, end_bit)) = receiver.recv_timeout(Duration::from_millis(100)) {
        // Check timeout
        if start.elapsed() > timeout {
            break;
        }
        
        // Verify that bit positions are sane
        assert!(start_bit <= end_bit, "Invalid bit range: {} > {}", start_bit, end_bit);
        assert!(end_bit <= (data.len() as u64) * 8, "End bit {} exceeds data length", end_bit);
        
        blocks.push((start_bit, end_bit));
        
        // Prevent unbounded memory growth - much stricter limit
        if blocks.len() > 1000 {
            break;
        }
    }
});
