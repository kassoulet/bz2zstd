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

    generate_data(test_file, 5);
    let orig_md5 = calculate_md5(test_file);
    compress_pbzip2(test_file);

    // Convert to zstd
    let status = Command::new(Path::new(BIN_PATH))
        .arg("--input")
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
