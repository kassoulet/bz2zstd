use memchr::memmem;

pub fn find_streams(data: &[u8]) -> Vec<(usize, usize)> {
    let mut streams: Vec<(usize, usize)> = Vec::new();
    let finder = memmem::Finder::new(b"BZh");

    // Stronger scanner: BZh[1-9] followed by 0x314159265359 (PI)
    // Total signature length: 4 bytes (header) + 6 bytes (block magic) = 10 bytes
    let magic_block = [0x31, 0x41, 0x59, 0x26, 0x53, 0x59];

    for pos in finder.find_iter(data) {
        // Check if we have enough bytes for the full signature
        if pos + 10 <= data.len() {
            let compression_level = data[pos + 3];
            if (b'1'..=b'9').contains(&compression_level) {
                // Check for Block Magic PI
                if data[pos + 4..pos + 10] == magic_block {
                    if !streams.is_empty() {
                        // Close the previous stream
                        let last_idx = streams.len() - 1;
                        streams[last_idx].1 = pos;
                    }
                    // Start new stream
                    streams.push((pos, data.len())); // Default end to EOF
                }
            }
        }
    }

    streams
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_find_streams_empty() {
        let data = [];
        let streams = find_streams(&data);
        assert!(streams.is_empty());
    }

    #[test]
    fn test_find_streams_single() {
        // BZh9 + PI + some data
        let mut data = b"BZh9".to_vec();
        data.extend_from_slice(&[0x31, 0x41, 0x59, 0x26, 0x53, 0x59]); // PI
        data.extend_from_slice(b"some data");

        let streams = find_streams(&data);
        assert_eq!(streams.len(), 1);
        assert_eq!(streams[0].0, 0);
        assert_eq!(streams[0].1, data.len());
    }

    #[test]
    fn test_find_streams_multiple() {
        let mut data = Vec::new();

        // Stream 1
        let s1_start = 0;
        data.extend_from_slice(b"BZh9");
        data.extend_from_slice(&[0x31, 0x41, 0x59, 0x26, 0x53, 0x59]);
        data.extend_from_slice(b"stream1");
        let _s1_end = data.len();

        // Garbage
        data.extend_from_slice(b"garbage");

        // Stream 2
        let s2_start = data.len();
        data.extend_from_slice(b"BZh5");
        data.extend_from_slice(&[0x31, 0x41, 0x59, 0x26, 0x53, 0x59]);
        data.extend_from_slice(b"stream2");

        let streams = find_streams(&data);
        assert_eq!(streams.len(), 2);
        assert_eq!(streams[0], (s1_start, s2_start)); // First stream ends where second begins
        assert_eq!(streams[1], (s2_start, data.len()));
    }

    #[test]
    fn test_find_streams_incomplete_header() {
        let data = b"BZh";
        let streams = find_streams(data);
        assert!(streams.is_empty());
    }

    #[test]
    fn test_find_streams_invalid_magic() {
        let mut data = b"BZh9".to_vec();
        data.extend_from_slice(&[0x00, 0x00, 0x00, 0x00, 0x00, 0x00]); // Invalid magic
        let streams = find_streams(&data);
        assert!(streams.is_empty());
    }

    #[test]
    fn test_find_streams_sliced() {
        let mut data = Vec::new();

        // Stream 1
        let s1_start = 0;
        data.extend_from_slice(b"BZh9");
        data.extend_from_slice(&[0x31, 0x41, 0x59, 0x26, 0x53, 0x59]);
        data.extend_from_slice(b"stream1");

        // Stream 2
        let s2_start = data.len();
        data.extend_from_slice(b"BZh5");
        data.extend_from_slice(&[0x31, 0x41, 0x59, 0x26, 0x53, 0x59]);
        data.extend_from_slice(b"stream2");

        // Test with full data
        let streams = find_streams(&data);
        assert_eq!(streams.len(), 2);
        assert_eq!(streams[0], (s1_start, s2_start));
        assert_eq!(streams[1], (s2_start, data.len()));

        // Test with slice that cuts off the second stream header
        // Slice ends right before 'B' of second stream
        let slice_len = s2_start;
        let streams_sliced = find_streams(&data[..slice_len]);
        assert_eq!(streams_sliced.len(), 1);
        assert_eq!(streams_sliced[0], (s1_start, slice_len));

        // Test with slice that includes header but not full signature of second stream
        // Slice ends after 'BZh5' but before PI
        let slice_len_partial = s2_start + 4;
        let streams_sliced_partial = find_streams(&data[..slice_len_partial]);
        assert_eq!(streams_sliced_partial.len(), 1);
        assert_eq!(streams_sliced_partial[0], (s1_start, slice_len_partial));
    }
}
