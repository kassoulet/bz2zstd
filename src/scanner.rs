pub fn find_streams(data: &[u8]) -> Vec<(usize, usize)> {
    let mut streams: Vec<(usize, usize)> = Vec::new();
    let mut i = 0;
    
    // Stronger scanner: BZh[1-9] followed by 0x314159265359 (PI)
    // Total signature length: 4 bytes (header) + 6 bytes (block magic) = 10 bytes
    let magic_block = [0x31, 0x41, 0x59, 0x26, 0x53, 0x59];

    while i < data.len() {
        if let Some(pos) = find_subsequence(&data[i..], b"BZh") {
            let absolute_pos = i + pos;
            // Check if we have enough bytes for the full signature
            if absolute_pos + 10 <= data.len() {
                let compression_level = data[absolute_pos + 3];
                if compression_level >= b'1' && compression_level <= b'9' {
                    // Check for Block Magic PI
                    if &data[absolute_pos + 4..absolute_pos + 10] == magic_block {
                        if !streams.is_empty() {
                            // Close the previous stream
                            let last_idx = streams.len() - 1;
                            streams[last_idx].1 = absolute_pos;
                        }
                        // Start new stream
                        streams.push((absolute_pos, data.len())); // Default end to EOF
                        i = absolute_pos + 1; // Advance
                        continue;
                    }
                }
            }
            i = absolute_pos + 1;
        } else {
            break;
        }
    }
    
    streams
}

fn find_subsequence(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|window| window == needle)
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
}
