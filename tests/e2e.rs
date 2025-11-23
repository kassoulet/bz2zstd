use std::fs;
use std::path::Path;
use std::process::Command;

const BIN_PATH: &str = "target/release/bz2zstd";

fn compile_binary() {
    let status = Command::new("cargo")
        .arg("build")
        .arg("--release")
        .status()
        .expect("Failed to run cargo build");
    assert!(status.success(), "Cargo build failed");
}

fn generate_data(path: &str, size_mb: usize) {
    let status = Command::new("dd")
        .arg("if=/dev/urandom")
        .arg(format!("of={}", path))
        .arg("bs=1M")
        .arg(format!("count={}", size_mb))
        .arg("status=none")
        .status()
        .expect("Failed to run dd");
    assert!(status.success(), "Failed to generate data");
}

fn compress_pbzip2(input: &str) {
    let status = Command::new("pbzip2")
        .arg("-f")
        .arg("-k")
        .arg("-p4")
        .arg(input)
        .status()
        .expect("Failed to run pbzip2");
    assert!(status.success(), "Failed to compress with pbzip2");
}

fn calculate_md5(path: &str) -> String {
    let output = Command::new("md5sum")
        .arg(path)
        .output()
        .expect("Failed to run md5sum");
    let output_str = String::from_utf8_lossy(&output.stdout);
    output_str.split_whitespace().next().unwrap().to_string()
}

#[test]
fn test_e2e_zstd_conversion() {
    compile_binary();
    let test_file = "test_e2e_zstd.bin";
    let bz2_file = format!("{}.bz2", test_file);
    let zstd_file = "test_e2e_zstd.zst";
    let out_file = "test_e2e_zstd_out.bin";

    // Generate 0.5MB of data (5 * 100KB) to stay within the 1MB hardcoded limit
    let status = Command::new("dd")
        .arg("if=/dev/urandom")
        .arg(format!("of={}", test_file))
        .arg("bs=100k")
        .arg("count=5")
        .arg("status=none")
        .status()
        .expect("Failed to run dd");
    assert!(status.success(), "Failed to generate data");
    let orig_md5 = calculate_md5(test_file);
    let status = Command::new("bzip2")
        .arg("-k")
        .arg("-f")
        .arg(test_file)
        .status()
        .expect("Failed to run bzip2");
    assert!(status.success(), "Failed to compress with bzip2");

    // Convert to zstd
    let status = Command::new(Path::new(BIN_PATH))
        .arg(&bz2_file)
        .arg("--output")
        .arg(zstd_file)
        .status()
        .expect("Failed to run bz2zstd");
    assert!(status.success());

    // Decompress zstd to verify
    let status = Command::new("zstd")
        .arg("-d")
        .arg("-f")
        .arg("-o")
        .arg(out_file)
        .arg(zstd_file)
        .status()
        .expect("Failed to run zstd");
    assert!(status.success());

    let new_md5 = calculate_md5(out_file);
    assert_eq!(orig_md5, new_md5);

    // Cleanup
    let _ = fs::remove_file(test_file);
    let _ = fs::remove_file(bz2_file);
    let _ = fs::remove_file(zstd_file);
    let _ = fs::remove_file(out_file);
}

#[test]
fn test_e2e_large_file() {
    compile_binary();
    let test_file = "test_e2e_large.bin";
    let bz2_file = format!("{}.bz2", test_file);
    let zstd_file = "test_e2e_large.zst";
    let out_file = "test_e2e_large_out.bin";

    // Generate 5MB of data (enough to have multiple blocks)
    generate_data(test_file, 5);

    // Compress with bzip2 (single stream usually, unless pbzip2 used)
    // Use standard bzip2 to ensure single stream if possible, or pbzip2 is fine too.
    // If we use pbzip2, it creates multiple streams.
    // To test block splitting, we need a single large stream.
    // `bzip2` is single threaded and creates one stream.
    // But `bzip2` might not be installed or slow.
    // `pbzip2` with `-p1` creates one stream? No, it still splits.
    // Actually, `pbzip2` creates independent streams.
    // To test our block splitter, we need a file that `find_streams` sees as 1 stream,
    // but `find_blocks` splits.
    // Standard `bzip2` does this.

    let status = Command::new("bzip2")
        .arg("-k")
        .arg("-f")
        .arg(test_file)
        .status();

    if status.is_err() || !status.unwrap().success() {
        // Fallback to pbzip2 if bzip2 not found, but then we might not test block splitting of single stream.
        // But we still test correctness.
        eprintln!("bzip2 not found or failed, trying pbzip2");
        compress_pbzip2(test_file);
    }

    let orig_md5 = calculate_md5(test_file);

    // Run bz2zstd
    let status = Command::new(Path::new(BIN_PATH))
        .arg(&bz2_file)
        .arg("--output")
        .arg(zstd_file)
        .status()
        .expect("Failed to run bz2zstd");

    assert!(status.success(), "bz2zstd failed");

    // Decompress zstd to verify
    let status = Command::new("zstd")
        .arg("-d")
        .arg("-f")
        .arg("-o")
        .arg(out_file)
        .arg(zstd_file)
        .status()
        .expect("Failed to run zstd");
    assert!(status.success(), "zstd decompression failed");

    let new_md5 = calculate_md5(out_file);
    assert_eq!(orig_md5, new_md5, "MD5 mismatch");

    // Cleanup
    let _ = fs::remove_file(test_file);
    let _ = fs::remove_file(bz2_file);
    let _ = fs::remove_file(zstd_file);
    let _ = fs::remove_file(out_file);
}
