use aho_corasick::AhoCorasick;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum MarkerType {
    Block,
    Eos,
}

pub struct Scanner {
    ac: AhoCorasick,
    patterns_info: Vec<(u64, MarkerType, usize)>, // (magic, type, shift)
}

impl Scanner {
    pub fn new(_data: &[u8]) -> Self {
        let magic_block: u64 = 0x314159265359;
        let magic_eos: u64 = 0x177245385090;
        
        let mut patterns = Vec::new();
        let mut patterns_info = Vec::new();

        // Generate patterns for Block
        let magic_top = magic_block << 16;
        for shift in 0..8 {
            let pattern_u64 = magic_top >> shift;
            let pattern_bytes = pattern_u64.to_be_bytes();
            let search_key = pattern_bytes[1..5].to_vec();
            patterns.push(search_key);
            patterns_info.push((magic_block, MarkerType::Block, shift));
        }

        // Generate patterns for EOS
        let magic_top = magic_eos << 16;
        for shift in 0..8 {
            let pattern_u64 = magic_top >> shift;
            let pattern_bytes = pattern_u64.to_be_bytes();
            let search_key = pattern_bytes[1..5].to_vec();
            patterns.push(search_key);
            patterns_info.push((magic_eos, MarkerType::Eos, shift));
        }

        let ac = AhoCorasick::new(patterns).unwrap();

        Self {
            ac,
            patterns_info,
        }
    }

    /// Scans a slice in parallel and streams results to a sender.
    /// Results are sent as (chunk_index, markers).
    /// The caller is responsible for reordering.
    pub fn scan_stream(
        &self,
        data: &[u8],
        base_offset_bits: u64,
        pool: &rayon::ThreadPool,
        sender: crossbeam_channel::Sender<(usize, Vec<(u64, MarkerType)>)>,
    ) {
        let chunk_size = 1 * 1024 * 1024; // 1MB chunks for cache locality
        let overlap = 8;
        let len = data.len();
        let num_chunks = (len + chunk_size - 1) / chunk_size;

        // We spawn tasks into the pool.
        // We don't wait for them here.
        // But we need to make sure `data` lives long enough.
        // `data` is `&[u8]`. `Scanner` is `&Scanner`.
        // The closure passed to `spawn` must be `'static` unless we use `scope`.
        // But `scan_stream` is called inside `scope` in `main.rs`.
        // So we can use `pool.scope`?
        // `pool.scope` blocks until all tasks finish.
        // We want to return immediately?
        // No, `scan_stream` runs in `Scanner Thread`.
        // If it blocks until all chunks are scanned, that's fine, AS LONG AS it sends results AS THEY FINISH.
        // `pool.scope` allows that.
        
        pool.scope(|s| {
            for i in 0..num_chunks {
                let sender = sender.clone();
                let start = i * chunk_size;
                let end = std::cmp::min(start + chunk_size, len);
                let scan_end = std::cmp::min(end + overlap, len);
                let slice = &data[start..scan_end];
                
                s.spawn(move |_| {
                    let mut local_markers = Vec::new();
                    
                    for mat in self.ac.find_iter(slice) {
                        let pattern_id = mat.pattern();
                        let match_start = mat.start();
                        
                        if match_start == 0 { continue; }
                        let start_byte_rel = match_start - 1;
                        
                        if start_byte_rel >= (end - start) {
                            continue;
                        }

                        let (magic, mtype, shift) = self.patterns_info[pattern_id];
                        let rel_bit_offset = (start + start_byte_rel) as u64 * 8 + shift as u64;
                        
                        if verify_magic(data, rel_bit_offset, magic) {
                            local_markers.push((base_offset_bits + rel_bit_offset, mtype));
                        }
                    }
                    
                    let _ = sender.send((i, local_markers));
                });
            }
        });
    }

}

/// Extracts a range of bits from the byte slice and returns them as a byte vector.
/// The output is byte-aligned (starts at bit 0 of the first output byte).
/// If the number of bits is not a multiple of 8, the last byte is padded with zeros in the low bits.
pub fn extract_bits(data: &[u8], start_bit: u64, end_bit: u64) -> Vec<u8> {
    if start_bit >= end_bit {
        return Vec::new();
    }

    let bit_len = end_bit - start_bit;
    let byte_len = ((bit_len + 7) / 8) as usize;
    let mut out = Vec::with_capacity(byte_len);

    let start_byte = (start_bit / 8) as usize;
    let shift = (start_bit % 8) as u8;

    if shift == 0 {
        // Fast path: aligned copy
        out.extend_from_slice(&data[start_byte..start_byte + byte_len]);
        
        // Mask the last byte if needed
        let last_bits = (bit_len % 8) as u8;
        if last_bits > 0 {
            let mask = 0xFFu8 << (8 - last_bits);
            if let Some(last) = out.last_mut() {
                *last &= mask;
            }
        }
    } else {
        // Unaligned copy optimized using u64
        let mut idx = start_byte;
        let mut bits_left = bit_len;
        
        // Process 8 bytes at a time (u64)
        // We need 9 bytes of input to produce 8 bytes of output (due to shift)
        // Actually, we can read u64, shift, and write u64?
        // No, output is Vec<u8>.
        // We can cast out pointer to u64? Unsafe.
        // Let's stick to reading u64 and writing bytes, or writing u64 if we use unsafe.
        // Safe approach: Read u64, write bytes.
        // But writing bytes one by one is slow.
        // We want to write u64.
        // Let's use `out.extend_from_slice(&val.to_be_bytes())`.
        
        while bits_left >= 64 {
            // We need 64 bits.
            // Input: data[idx..idx+9].
            // We read u64 from idx.
            // val1 = u64::from_be_bytes(data[idx..idx+8])
            // val2 = data[idx+8]
            // result = (val1 << shift) | (val2 >> (8-shift)) ??
            // No, shifting u64 left by shift.
            // We need bits from next byte.
            // This is complex for u64.
            
            // Simpler: Read u64 at UNALIGNED address?
            // No, we are reading from `data` which is `&[u8]`.
            // We want to extract 64 bits starting at `start_bit`.
            // `start_bit` is `idx * 8 + shift`.
            // We can read u64 from `data` at `idx` and `idx+1`.
            // Actually, if we read u64 from `data[idx..idx+8]`.
            // And u64 from `data[idx+1..idx+9]`.
            // Then shift?
            
            // Let's use the property:
            // val = (u64_at_idx << shift) | (byte_at_idx_plus_8 >> (8-shift))
            // This gives 64 bits?
            // `u64_at_idx` has 64 bits.
            // `<< shift` loses top `shift` bits.
            // We want those bits?
            // No, we want bits starting at `shift`.
            // So we want `u64_at_idx` shifted LEFT?
            // `data[idx]` is MSB.
            // If shift=1. We want bits 1..7 of byte 0, bits 0..7 of byte 1...
            // `val = (u64_at_idx << shift) | (next_byte >> (8-shift))`
            // This works!
            
            if idx + 9 <= data.len() {
                let bytes: [u8; 8] = data[idx..idx+8].try_into().unwrap();
                let val1 = u64::from_be_bytes(bytes);
                let val2 = data[idx+8] as u64;
                
                let result = (val1 << shift) | (val2 >> (8 - shift));
                out.extend_from_slice(&result.to_be_bytes());
                
                idx += 8;
                bits_left -= 64;
            } else {
                break; // Not enough data for fast path
            }
        }
        
        // Handle remaining bytes one by one
        while bits_left >= 8 {
            let b1 = data[idx];
            let b2 = if idx + 1 < data.len() { data[idx + 1] } else { 0 };
            
            let val = (b1 << shift) | (b2 >> (8 - shift));
            out.push(val);
            
            idx += 1;
            bits_left -= 8;
        }
        
        // Handle remaining bits (1..7)
        if bits_left > 0 {
            let b1 = data[idx];
            let b2 = if idx + 1 < data.len() { data[idx + 1] } else { 0 };
            let mut val = (b1 << shift) | (b2 >> (8 - shift));
            
            // Mask the last byte
            // We only want `bits_left` bits.
            // The `val` contains 8 bits (some might be garbage from b2).
            // We want to keep top `bits_left` bits.
            // mask = 0xFF << (8 - bits_left)
            let mask = 0xFFu8 << (8 - bits_left);
            val &= mask;
            out.push(val);
        }
    }
    
    // The original code handled "Mask the last byte if needed" separately.
    // My new code handles it in the "remaining bits" section.
    // But wait, if `bit_len` is not multiple of 8.
    // The loop `while bits_left >= 8` handles full bytes.
    // The last byte (partial) is handled after.
    // But what if `bit_len` was e.g. 12.
    // Loop runs once (8 bits). `out` has 1 byte.
    // `bits_left` = 4.
    // `idx` incremented.
    // Handle remaining 4 bits.
    // We read `data[idx]` and `data[idx+1]`.
    // Shift.
    // Mask.
    // Push.
    // This is correct.
    
    // Wait, the original code had:
    // let last_bits = (bit_len % 8) as u8;
    // if last_bits > 0 { ... mask last byte ... }
    // My new code does the same.
    
    out
}

fn verify_magic(data: &[u8], bit_offset: u64, expected_magic: u64) -> bool {
    let byte_idx = (bit_offset / 8) as usize;
    let shift = (bit_offset % 8) as u8;

    // We need to read 48 bits from `data` starting at `bit_offset`.
    // This spans 6 or 7 bytes.
    if byte_idx + 6 > data.len() {
        return false;
    }

    // Read 8 bytes (u64) to handle the shift easily, if available.
    let mut buf = [0u8; 8];
    let len_to_read = std::cmp::min(8, data.len() - byte_idx);
    buf[..len_to_read].copy_from_slice(&data[byte_idx..byte_idx + len_to_read]);

    let val = u64::from_be_bytes(buf);

    let magic_top = expected_magic << 16;
    let expected = magic_top >> shift;
    let mask = 0xFFFFFFFFFFFF0000 >> shift;

    (val & mask) == expected
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scan_to_vec(data: &[u8]) -> Vec<(u64, MarkerType)> {
        let scanner = Scanner::new(data);
        let (tx, rx) = crossbeam_channel::bounded(100);
        let pool = rayon::ThreadPoolBuilder::new().num_threads(1).build().unwrap();
        
        // Run scan_stream in a scope
        std::thread::scope(|s| {
            s.spawn(|| {
                scanner.scan_stream(data, 0, &pool, tx);
            });
        });

        let mut results = Vec::new();
        for (_, markers) in rx {
            results.extend(markers);
        }
        results.sort_by_key(|k| k.0);
        results
    }

    #[test]
    fn test_scanner_empty() {
        let data = [];
        let markers = scan_to_vec(&data);
        assert!(markers.is_empty());
    }

    #[test]
    fn test_scanner_single_block() {
        // Block Magic: 0x314159265359
        let mut data = Vec::new();
        data.extend_from_slice(&[0x31, 0x41, 0x59, 0x26, 0x53, 0x59]); // PI
        data.extend_from_slice(b"some data");

        let markers = scan_to_vec(&data);
        assert_eq!(markers.len(), 1);
        assert_eq!(markers[0].0, 0);
        assert!(matches!(markers[0].1, MarkerType::Block));
    }

    #[test]
    fn test_scanner_eos() {
        // EOS Magic: 0x177245385090
        let mut data = Vec::new();
        data.extend_from_slice(&[0x17, 0x72, 0x45, 0x38, 0x50, 0x90]); // sqrt(PI)

        let markers = scan_to_vec(&data);
        assert_eq!(markers.len(), 1);
        assert_eq!(markers[0].0, 0);
        assert!(matches!(markers[0].1, MarkerType::Eos));
    }

    #[test]
    fn test_scanner_multiple_blocks() {
        let mut data = Vec::new();
        
        // Block 1 at 0
        data.extend_from_slice(&[0x31, 0x41, 0x59, 0x26, 0x53, 0x59]);
        data.extend_from_slice(b"data1");
        
        // Block 2 at 6+5 = 11 bytes
        let pos2 = data.len() as u64 * 8;
        data.extend_from_slice(&[0x31, 0x41, 0x59, 0x26, 0x53, 0x59]);
        data.extend_from_slice(b"data2");

        // EOS
        let pos_eos = data.len() as u64 * 8;
        data.extend_from_slice(&[0x17, 0x72, 0x45, 0x38, 0x50, 0x90]);

        let markers = scan_to_vec(&data);
        assert_eq!(markers.len(), 3);
        assert_eq!(markers[0].0, 0);
        assert!(matches!(markers[0].1, MarkerType::Block));
        
        assert_eq!(markers[1].0, pos2);
        assert!(matches!(markers[1].1, MarkerType::Block));
        
        assert_eq!(markers[2].0, pos_eos);
        assert!(matches!(markers[2].1, MarkerType::Eos));
    }

    #[test]
    fn test_scanner_shifted() {
        // Construct a shifted block magic.
        // Magic: 0x314159265359
        // Shift 1 bit right (in stream logic, this means it starts at bit 1)
        let magic: u64 = 0x314159265359;
        let shift = 1;
        let val = (magic << 16) >> shift; 
        let bytes = val.to_be_bytes();

        // We need enough bytes. val is u64 (8 bytes).
        // The magic is 6 bytes.
        // If we write 8 bytes, we cover the magic.
        
        let markers = scan_to_vec(&bytes);
        assert_eq!(markers.len(), 1);
        assert_eq!(markers[0].0, shift as u64);
        assert!(matches!(markers[0].1, MarkerType::Block));
    }

    #[test]
    fn test_extract_bits_aligned() {
        let data = vec![0xAA, 0xBB, 0xCC];
        // Extract 0xBB
        let extracted = extract_bits(&data, 8, 16);
        assert_eq!(extracted, vec![0xBB]);
    }

    #[test]
    fn test_extract_bits_shifted() {
        // Data: 0xAA, 0xBB
        // Binary: 10101010 10111011
        // Extract 8 bits starting at 4.
        // Bits 4..12.
        // Byte 0 (AA): 1010[1010]
        // Byte 1 (BB): [1011]1011
        // Result: 1010 1011 = 0xAB
        let data = vec![0xAA, 0xBB];
        let extracted = extract_bits(&data, 4, 12);
        assert_eq!(extracted, vec![0xAB]);
    }

    #[test]
    fn test_extract_bits_partial() {
        // Data: 0xFF
        // Extract 4 bits at 0.
        // Result: 11110000 = 0xF0
        let data = vec![0xFF];
        let extracted = extract_bits(&data, 0, 4);
        assert_eq!(extracted, vec![0xF0]);
    }
    
    #[test]
    fn test_extract_bits_u64_path() {
        // Test the u64 optimized path (more than 8 bytes)
        // 10 bytes of 0xFF
        let data = vec![0xFF; 10];
        // Extract 64 bits at offset 4
        // Should be all 1s
        let extracted = extract_bits(&data, 4, 68);
        assert_eq!(extracted.len(), 8);
        assert_eq!(extracted, vec![0xFF; 8]);
    }
}
