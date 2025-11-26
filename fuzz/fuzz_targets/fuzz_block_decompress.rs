#![no_main]

use libfuzzer_sys::fuzz_target;
use parallel_bzip2::decompress_block;
use arbitrary::Arbitrary;

#[derive(Arbitrary, Debug)]
struct FuzzInput {
    data: Vec<u8>,
    start_bit: u64,
    end_bit: u64,
}

fuzz_target!(|input: FuzzInput| {
    // Don't fuzz empty data or extremely large inputs
    if input.data.is_empty() || input.data.len() > 1_000_000 {
        return;
    }

    let max_bits = (input.data.len() as u64) * 8;
    
    // Normalize bit positions to valid ranges
    let start_bit = input.start_bit % (max_bits + 1);
    let end_bit = input.end_bit % (max_bits + 1);
    
    // Ensure start <= end
    let (start_bit, end_bit) = if start_bit <= end_bit {
        (start_bit, end_bit)
    } else {
        (end_bit, start_bit)
    };
    
    // Try to decompress the block
    // This should either succeed, return an error, or panic on bugs
    let _ = decompress_block(&input.data, start_bit, end_bit);
    
    // Test edge cases explicitly
    if start_bit == end_bit {
        // Zero-length range
        let _ = decompress_block(&input.data, start_bit, start_bit);
    }
    
    if end_bit == max_bits {
        // Range extending to the very end
        let _ = decompress_block(&input.data, start_bit, max_bits);
    }
});
