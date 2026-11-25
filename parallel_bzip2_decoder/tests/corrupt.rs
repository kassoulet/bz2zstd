use bzip2::{write::BzEncoder, Compression};
use parallel_bzip2_decoder::Bz2Decoder;
use std::io::{Read, Write};
use std::sync::Arc;

#[test]
fn corrupt_block_returns_error_after_valid_prefix() {
    // Pseudo-random data, ~600 KB at level 1 (100 KB blocks) => several blocks
    let mut x = 12345u32;
    let original: Vec<u8> = (0..600_000)
        .map(|_| {
            x = x.wrapping_mul(1664525).wrapping_add(1013904223);
            (x >> 24) as u8 & 0x0f
        })
        .collect();
    let mut enc = BzEncoder::new(Vec::new(), Compression::new(1));
    enc.write_all(&original).unwrap();
    let mut data = enc.finish().unwrap();

    let mid = data.len() * 6 / 10;
    for b in &mut data[mid..mid + 16] {
        *b ^= 0xff;
    }

    let mut dec = Bz2Decoder::new(Arc::new(data));
    let mut out = Vec::new();
    let err = dec
        .read_to_end(&mut out)
        .expect_err("corrupt input must error");
    assert_eq!(err.kind(), std::io::ErrorKind::InvalidData);
    assert!(!out.is_empty() && out.len() < original.len());
    assert_eq!(out, original[..out.len()], "valid prefix must be intact");
    // After the error the decoder must not hang on later reads.
    assert_eq!(dec.read(&mut [0u8; 16]).unwrap(), 0);
}

#[test]
fn truncated_last_block_is_detected() {
    let original: Vec<u8> = (0..300_000u32)
        .map(|i| (i.wrapping_mul(2654435761) >> 24) as u8)
        .collect();
    let mut enc = BzEncoder::new(Vec::new(), Compression::new(1));
    enc.write_all(&original).unwrap();
    let mut data = enc.finish().unwrap();
    data.truncate(data.len() - 2000); // cut into the final block (and its footer)

    let mut out = Vec::new();
    let res = Bz2Decoder::new(Arc::new(data)).read_to_end(&mut out);
    assert!(
        res.is_err(),
        "truncated input must error, got {} bytes",
        out.len()
    );
}

#[test]
fn out_of_range_block_is_an_error_not_a_panic() {
    use parallel_bzip2_decoder::decompress_block;
    let data = vec![0u8; 100];
    assert!(decompress_block(&data, 0, 100 * 8 + 1).is_err()); // past the end
    assert!(decompress_block(&data, 50, 10).is_err()); // reversed
    assert!(decompress_block(&data, u64::MAX - 1, u64::MAX).is_err()); // overflow-ish
}
