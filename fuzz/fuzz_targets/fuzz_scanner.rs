#![no_main]

use libfuzzer_sys::fuzz_target;
use parallel_bzip2::scan_blocks;

fuzz_target!(|data: &[u8]| {
    // Don't fuzz empty data or extremely large inputs
    if data.is_empty() || data.len() > 10_000_000 {
        return;
    }

    // Call scan_blocks and consume the results
    let receiver = scan_blocks(data);
    
    // Collect all detected blocks
    let mut blocks = Vec::new();
    while let Ok((start_bit, end_bit)) = receiver.recv() {
        // Verify that bit positions are sane
        assert!(start_bit <= end_bit, "Invalid bit range: {} > {}", start_bit, end_bit);
        assert!(end_bit <= (data.len() as u64) * 8, "End bit {} exceeds data length", end_bit);
        
        blocks.push((start_bit, end_bit));
        
        // Prevent infinite loops in case of bugs
        if blocks.len() > 10000 {
            break;
        }
    }
});
