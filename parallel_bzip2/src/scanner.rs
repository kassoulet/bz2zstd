use aho_corasick::AhoCorasick;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum MarkerType {
    Block,
    Eos,
}

const MAGIC_BLOCK: u64 = 0x314159265359;
const MAGIC_EOS: u64 = 0x177245385090;

pub struct Scanner {
    ac: AhoCorasick,
    patterns_info: Vec<(u64, MarkerType, usize)>, // (magic, type, shift)
}

impl Scanner {
    pub fn new() -> Self {
        let mut patterns = Vec::new();
        let mut patterns_info = Vec::new();

        // Generate patterns for Block
        let magic_top = MAGIC_BLOCK << 16;
        for shift in 0..8 {
            let pattern_u64 = magic_top >> shift;
            let pattern_bytes = pattern_u64.to_be_bytes();
            let search_key = pattern_bytes[1..5].to_vec();
            patterns.push(search_key);
            patterns_info.push((MAGIC_BLOCK, MarkerType::Block, shift));
        }

        // Generate patterns for EOS
        let magic_top = MAGIC_EOS << 16;
        for shift in 0..8 {
            let pattern_u64 = magic_top >> shift;
            let pattern_bytes = pattern_u64.to_be_bytes();
            let search_key = pattern_bytes[1..5].to_vec();
            patterns.push(search_key);
            patterns_info.push((MAGIC_EOS, MarkerType::Eos, shift));
        }

        let ac = AhoCorasick::new(patterns).unwrap();

        Self { ac, patterns_info }
    }

    /// Scans a slice in parallel and streams results to a sender.
    /// Results are sent as (chunk_index, markers).
    /// The caller is responsible for reordering.
    pub fn scan_stream(
        &self,
        data: &[u8],
        base_offset_bits: u64,
        sender: crossbeam_channel::Sender<(usize, Vec<(u64, MarkerType)>)>,
    ) {
        let chunk_size = 1024 * 1024; // 1MB chunks for cache locality
        let overlap = 8;
        let len = data.len();
        let num_chunks = len.div_ceil(chunk_size);

        // Create our own thread pool to avoid deadlock with caller's pool
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(rayon::current_num_threads())
            .build()
            .unwrap();

        // Use pool.scope to allow borrowing `data` in the closure.
        // This blocks until all tasks are finished, but since we are in a dedicated
        // scanner thread and sending results via channel, this is the desired behavior.

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

                        if match_start == 0 {
                            continue;
                        }
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

impl Default for Scanner {
    fn default() -> Self {
        Self::new()
    }
}

/// Extracts a range of bits from the byte slice and writes them into the provided buffer.
/// The output is byte-aligned (starts at bit 0 of the first output byte).
/// If the number of bits is not a multiple of 8, the last byte is padded with zeros in the low bits.
pub fn extract_bits(data: &[u8], start_bit: u64, end_bit: u64, out: &mut Vec<u8>) {
    if start_bit >= end_bit {
        return;
    }

    let bit_len = end_bit - start_bit;
    let byte_len = bit_len.div_ceil(8) as usize;
    out.reserve(byte_len);

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
        while bits_left >= 64 {
            if idx + 9 <= data.len() {
                let bytes: [u8; 8] = data[idx..idx + 8].try_into().unwrap();
                let val1 = u64::from_be_bytes(bytes);
                let val2 = data[idx + 8] as u64;

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
            let b2 = if idx + 1 < data.len() {
                data[idx + 1]
            } else {
                0
            };

            let val = (b1 << shift) | (b2 >> (8 - shift));
            out.push(val);

            idx += 1;
            bits_left -= 8;
        }

        // Handle remaining bits (1..7)
        if bits_left > 0 {
            let b1 = data[idx];
            let b2 = if idx + 1 < data.len() {
                data[idx + 1]
            } else {
                0
            };
            let mut val = (b1 << shift) | (b2 >> (8 - shift));

            // Mask the last byte
            let mask = 0xFFu8 << (8 - bits_left);
            val &= mask;
            out.push(val);
        }
    }
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
        let scanner = Scanner::new();
        let (tx, rx) = crossbeam_channel::bounded(100);
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(1)
            .build()
            .unwrap();

        // Run scan_stream in a scope
        std::thread::scope(|s| {
            s.spawn(|| {
                scanner.scan_stream(data, 0, tx);
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
        let mut extracted = Vec::new();
        extract_bits(&data, 8, 16, &mut extracted);
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
        let mut extracted = Vec::new();
        extract_bits(&data, 4, 12, &mut extracted);
        assert_eq!(extracted, vec![0xAB]);
    }

    #[test]
    fn test_extract_bits_partial() {
        // Data: 0xFF
        // Extract 4 bits at 0.
        // Result: 11110000 = 0xF0
        let data = vec![0xFF];
        let mut extracted = Vec::new();
        extract_bits(&data, 0, 4, &mut extracted);
        assert_eq!(extracted, vec![0xF0]);
    }

    #[test]
    fn test_extract_bits_u64_path() {
        // Test the u64 optimized path (more than 8 bytes)
        // 10 bytes of 0xFF
        let data = vec![0xFF; 10];
        // Extract 64 bits at offset 4
        // Should be all 1s
        let mut extracted = Vec::new();
        extract_bits(&data, 4, 68, &mut extracted);
        assert_eq!(extracted.len(), 8);
        assert_eq!(extracted, vec![0xFF; 8]);
    }
}
