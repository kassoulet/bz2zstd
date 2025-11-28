//! High-performance parallel scanner for bzip2 block boundaries.
//!
//! This module provides efficient scanning of bzip2 compressed data to locate block
//! and end-of-stream markers. The scanner uses the Aho-Corasick algorithm for fast
//! pattern matching and processes data in parallel chunks for maximum throughput.
//!
//! # Algorithm
//!
//! The scanner searches for two 48-bit magic numbers defined in the bzip2 specification:
//! - Block marker: 0x314159265359 (π in base 16)
//! - End-of-stream marker: 0x177245385090 (√π in base 16)
//!
//! Since these markers can appear at any bit offset (not just byte boundaries), the
//! scanner generates 8 shifted patterns for each magic number and uses Aho-Corasick
//! for efficient multi-pattern matching. Candidates are then verified by extracting
//! and comparing the full 48-bit value.
//!
//! # Performance
//!
//! - Parallel processing using Rayon for multi-core utilization
//! - 1MB chunks for optimal cache locality
//! - Aho-Corasick automaton for O(n) pattern matching
//! - Minimal memory allocation through buffer reuse

use aho_corasick::AhoCorasick;

/// Marker type found in bzip2 streams.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum MarkerType {
    /// Block start marker (π = 0x314159265359)
    Block,
    /// End-of-stream marker (√π = 0x177245385090)
    Eos,
}

/// Block start magic number from bzip2 specification.
/// This is π represented in hexadecimal: 3.14159265359...
const MAGIC_BLOCK: u64 = 0x314159265359;

/// End-of-stream magic number from bzip2 specification.
/// This is √π represented in hexadecimal: 1.77245385090...
const MAGIC_EOS: u64 = 0x177245385090;

/// Parallel scanner for bzip2 block boundaries.
///
/// The scanner pre-computes 16 search patterns (8 for each magic number, one per
/// bit offset) and uses the Aho-Corasick algorithm for efficient multi-pattern
/// matching. This allows finding markers at any bit position in a single pass.
pub struct Scanner {
    /// Aho-Corasick automaton for fast multi-pattern matching
    ac: AhoCorasick,
    /// Pattern metadata: (magic_number, marker_type, bit_shift)
    /// Used to verify and classify matches from the Aho-Corasick automaton
    patterns_info: Vec<(u64, MarkerType, usize)>,
}

impl Scanner {
    /// Creates a new scanner with pre-computed search patterns.
    ///
    /// This generates 16 patterns total: 8 shifted variants of the block marker
    /// and 8 shifted variants of the end-of-stream marker. Each variant corresponds
    /// to a different bit alignment (0-7 bits offset).
    ///
    /// # Pattern Generation
    ///
    /// For each magic number:
    /// 1. Shift left by 16 bits to make room for verification
    /// 2. Generate 8 variants by shifting right 0-7 bits
    /// 3. Extract middle 4 bytes as the search pattern
    /// 4. Store metadata for later verification
    ///
    /// # Performance
    ///
    /// The Aho-Corasick automaton is built once at construction time,
    /// enabling O(n) scanning regardless of the number of patterns.
    pub fn new() -> Self {
        let mut patterns = Vec::new();
        let mut patterns_info = Vec::new();

        // Generate patterns for Block marker (π)
        // We shift left by 16 bits to create space for bit-level alignment
        let magic_top = MAGIC_BLOCK << 16;
        for shift in 0..8 {
            let pattern_u64 = magic_top >> shift;
            let pattern_bytes = pattern_u64.to_be_bytes();
            // Use middle 4 bytes as search key (most distinctive part)
            let search_key = pattern_bytes[1..5].to_vec();
            patterns.push(search_key);
            patterns_info.push((MAGIC_BLOCK, MarkerType::Block, shift));
        }

        // Generate patterns for EOS marker (√π)
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

    /// Scans data in parallel and streams marker locations to a channel.
    ///
    /// This method divides the input into 1MB chunks and processes them in parallel
    /// using a dedicated thread pool. Results are sent as (chunk_index, markers)
    /// tuples, allowing the caller to reorder them if needed.
    ///
    /// # Arguments
    ///
    /// * `data` - The bzip2 compressed data to scan
    /// * `base_offset_bits` - Bit offset to add to all marker positions (for multi-file scanning)
    /// * `sender` - Channel to send results: (chunk_index, Vec<(bit_position, marker_type)>)
    ///
    /// # Performance
    ///
    /// - **Chunk size**: 1MB for optimal cache locality
    /// - **Overlap**: 8 bytes between chunks to catch markers at boundaries
    /// - **Thread pool**: Creates a dedicated pool to avoid deadlock with caller's pool
    /// - **Blocking**: This method blocks until all chunks are processed
    ///
    /// # Algorithm
    ///
    /// For each chunk:
    /// 1. Run Aho-Corasick pattern matching to find candidates
    /// 2. Filter out matches at chunk boundaries (handled by overlap)
    /// 3. Verify each candidate by extracting and comparing the full 48-bit magic number
    /// 4. Send verified markers with their bit positions to the channel
    pub fn scan_stream(
        &self,
        data: &[u8],
        base_offset_bits: u64,
        sender: crossbeam_channel::Sender<(usize, Vec<(u64, MarkerType)>)>,
    ) {
        // Performance: 1MB chunks provide good balance between:
        // - Cache locality (fits in L3 cache on most CPUs)
        // - Parallelism (enough chunks to keep all cores busy)
        // - Overhead (not too many small tasks)
        let chunk_size = 1024 * 1024;
        // Overlap ensures we don't miss markers that span chunk boundaries
        let overlap = 8;
        let len = data.len();
        let num_chunks = len.div_ceil(chunk_size);

        // Create a dedicated thread pool to prevent deadlock:
        // If we used the global pool and the caller is also using it (e.g., via par_bridge),
        // we could deadlock when all threads are waiting for scanner results but the scanner
        // can't make progress because all threads are blocked.
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(rayon::current_num_threads())
            .build()
            .unwrap();

        // Use pool.scope to allow borrowing `data` in the closure.
        // This blocks until all tasks are finished, which is desired behavior since
        // we're in a dedicated scanner thread sending results via channel.

        pool.scope(|s| {
            for i in 0..num_chunks {
                let sender = sender.clone();
                let start = i * chunk_size;
                let end = std::cmp::min(start + chunk_size, len);
                // Extend scan region to catch markers at chunk boundary
                let scan_end = std::cmp::min(end + overlap, len);
                let slice = &data[start..scan_end];

                s.spawn(move |_| {
                    let mut local_markers = Vec::new();

                    // Aho-Corasick finds all pattern matches in O(n) time
                    for mat in self.ac.find_iter(slice) {
                        let pattern_id = mat.pattern();
                        let match_start = mat.start();

                        // Skip matches at position 0 (we need the byte before for verification)
                        if match_start == 0 {
                            continue;
                        }
                        let start_byte_rel = match_start - 1;

                        // Skip matches in the overlap region (will be handled by next chunk)
                        if start_byte_rel >= (end - start) {
                            continue;
                        }

                        // Verify the match by extracting and comparing the full 48-bit magic
                        let (magic, mtype, shift) = self.patterns_info[pattern_id];
                        let rel_bit_offset = (start + start_byte_rel) as u64 * 8 + shift as u64;

                        if verify_magic(data, rel_bit_offset, magic) {
                            local_markers.push((base_offset_bits + rel_bit_offset, mtype));
                        }
                    }

                    // Send results for this chunk (ignore errors if receiver dropped)
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

/// Extracts a range of bits from a byte slice and appends them to the output buffer.
///
/// This function handles bit-level extraction, which is necessary because bzip2 blocks
/// can start and end at any bit position, not just byte boundaries.
///
/// # Arguments
///
/// * `data` - Source byte slice
/// * `start_bit` - Starting bit position (0-indexed, bit 0 is MSB of byte 0)
/// * `end_bit` - Ending bit position (exclusive)
/// * `out` - Output buffer to append extracted bits to
///
/// # Output Format
///
/// The extracted bits are written byte-aligned to the output buffer:
/// - The first output byte contains bits [start_bit..start_bit+8)
/// - If the bit range is not a multiple of 8, the last byte is padded with zeros
///
/// # Performance
///
/// This function has three code paths optimized for different scenarios:
/// 1. **Aligned fast path**: When start_bit is byte-aligned, uses memcpy-like operation
/// 2. **u64 SIMD path**: Processes 8 bytes at a time for better throughput
/// 3. **Byte-by-byte path**: Handles remaining bytes and edge cases
///
/// # Examples
///
/// ```
/// # use parallel_bzip2::extract_bits;
/// let data = vec![0xAA, 0xBB]; // 10101010 10111011
/// let mut out = Vec::new();
/// extract_bits(&data, 4, 12, &mut out);
/// // Extracts bits 4-11: 1010 1011 = 0xAB
/// assert_eq!(out, vec![0xAB]);
/// ```
pub fn extract_bits(data: &[u8], start_bit: u64, end_bit: u64, out: &mut Vec<u8>) {
    if start_bit >= end_bit {
        return;
    }

    let bit_len = end_bit - start_bit;
    let byte_len = bit_len.div_ceil(8) as usize;
    // Pre-allocate to avoid reallocations during extraction
    out.reserve(byte_len);

    let start_byte = (start_bit / 8) as usize;
    let shift = (start_bit % 8) as u8;

    if shift == 0 {
        // Fast path: byte-aligned extraction
        // This is essentially a memcpy, which is very fast
        out.extend_from_slice(&data[start_byte..start_byte + byte_len]);

        // Mask the last byte if we're extracting a partial byte
        let last_bits = (bit_len % 8) as u8;
        if last_bits > 0 {
            // Keep only the top `last_bits` bits
            let mask = 0xFFu8 << (8 - last_bits);
            if let Some(last) = out.last_mut() {
                *last &= mask;
            }
        }
    } else {
        // Unaligned extraction: bits don't start on a byte boundary
        // We need to shift and combine bytes to extract the bit range
        let mut idx = start_byte;
        let mut bits_left = bit_len;

        // Performance optimization: Process 8 bytes at a time using u64
        // This is SIMD-friendly and reduces loop overhead
        while bits_left >= 64 {
            if idx + 9 <= data.len() {
                // Read 8 bytes as u64, plus one extra byte for the shift
                let bytes: [u8; 8] = data[idx..idx + 8].try_into().unwrap();
                let val1 = u64::from_be_bytes(bytes);
                let val2 = data[idx + 8] as u64;

                // Shift and combine to extract the desired bits
                // val1 << shift: shift left to align the start
                // val2 >> (8 - shift): bring in bits from the next byte
                let result = (val1 << shift) | (val2 >> (8 - shift));
                out.extend_from_slice(&result.to_be_bytes());

                idx += 8;
                bits_left -= 64;
            } else {
                break; // Not enough data for u64 fast path
            }
        }

        // Handle remaining bytes one by one
        while bits_left >= 8 {
            let b1 = data[idx];
            let b2 = if idx + 1 < data.len() {
                data[idx + 1]
            } else {
                0 // Pad with zeros if at end of data
            };

            // Combine two bytes with appropriate shift
            let val = (b1 << shift) | (b2 >> (8 - shift));
            out.push(val);

            idx += 1;
            bits_left -= 8;
        }

        // Handle remaining bits (1-7 bits)
        if bits_left > 0 {
            let b1 = data[idx];
            let b2 = if idx + 1 < data.len() {
                data[idx + 1]
            } else {
                0
            };
            let mut val = (b1 << shift) | (b2 >> (8 - shift));

            // Mask to keep only the bits we need
            let mask = 0xFFu8 << (8 - bits_left);
            val &= mask;
            out.push(val);
        }
    }
}

/// Verifies that a 48-bit magic number exists at the specified bit offset.
///
/// This function is used to confirm candidates found by the Aho-Corasick pattern
/// matcher. Since the pattern matcher only looks at 4 bytes, we need to verify
/// the full 48-bit magic number.
///
/// # Arguments
///
/// * `data` - Source byte slice
/// * `bit_offset` - Bit position where the magic number should start
/// * `expected_magic` - The 48-bit magic number to verify (MAGIC_BLOCK or MAGIC_EOS)
///
/// # Algorithm
///
/// 1. Calculate byte position and bit shift from bit_offset
/// 2. Read 8 bytes (u64) starting at that position
/// 3. Shift the u64 to align the magic number
/// 4. Mask and compare with the expected value
///
/// # Returns
///
/// `true` if the magic number matches, `false` otherwise
fn verify_magic(data: &[u8], bit_offset: u64, expected_magic: u64) -> bool {
    let byte_idx = (bit_offset / 8) as usize;
    let shift = (bit_offset % 8) as u8;

    // We need to read 48 bits from `data` starting at `bit_offset`.
    // This spans 6 or 7 bytes depending on alignment.
    if byte_idx + 6 > data.len() {
        return false;
    }

    // Read 8 bytes (u64) to handle the shift easily
    let mut buf = [0u8; 8];
    let len_to_read = std::cmp::min(8, data.len() - byte_idx);
    buf[..len_to_read].copy_from_slice(&data[byte_idx..byte_idx + len_to_read]);

    let val = u64::from_be_bytes(buf);

    // Shift the expected magic to match the bit alignment in the data
    // The magic is 48 bits, so we shift it left by 16 to fill the top 48 bits of a u64
    let magic_top = expected_magic << 16;
    let expected = magic_top >> shift;
    // Create a mask for the top 48 bits (adjusted for shift)
    let mask = 0xFFFFFFFFFFFF0000 >> shift;

    (val & mask) == expected
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scan_to_vec(data: &[u8]) -> Vec<(u64, MarkerType)> {
        let scanner = Scanner::new();
        let (tx, rx) = crossbeam_channel::bounded(100);
        let _pool = rayon::ThreadPoolBuilder::new()
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
