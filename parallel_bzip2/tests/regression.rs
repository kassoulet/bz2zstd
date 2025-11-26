use std::fs;
use std::path::Path;
use std::process::Command;

const TEST_DIR: &str = "tests/fixtures";

fn calculate_md5_from_file(path: &Path) -> String {
    let output = Command::new("md5sum")
        .arg(path)
        .output()
        .expect("Failed to run md5sum");
    let output_str = String::from_utf8_lossy(&output.stdout);
    output_str.split_whitespace().next().unwrap().to_string()
}

fn calculate_md5_from_bytes(data: &[u8]) -> String {
    use std::io::Write;
    let mut child = Command::new("md5sum")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .expect("Failed to spawn md5sum");

    {
        let stdin = child.stdin.as_mut().expect("Failed to open stdin");
        stdin.write_all(data).expect("Failed to write to stdin");
    }

    let output = child.wait_with_output().expect("Failed to read stdout");
    let output_str = String::from_utf8_lossy(&output.stdout);
    output_str.split_whitespace().next().unwrap().to_string()
}

#[test]
fn test_regression_lbzip2_samples() {
    let test_dir = Path::new(TEST_DIR);

    // Ensure test directory exists
    if !test_dir.exists() {
        eprintln!(
            "Test directory {} not found. Skipping regression tests.",
            TEST_DIR
        );
        return;
    }

    let entries = fs::read_dir(test_dir).expect("Failed to read test directory");
    let mut failures = Vec::new();

    for entry in entries {
        let entry = entry.expect("Failed to read entry");
        let path = entry.path();
        let file_name = path.file_name().unwrap().to_string_lossy();

        // Process only .bz2 files
        if path.extension().and_then(|s| s.to_str()) == Some("bz2") {
            println!("Testing {:?}", file_name);

            // Test all files - no skipping allowed
            // Edge cases will expose decoder bugs that need to be fixed

            // 1. Try to decompress with lbzip2 to see if it's a valid file and get expected output
            let expected_output_path = path.with_extension("expected");
            let bzip2_status = Command::new("lbzip2")
                .arg("-d")
                .arg("-k")
                .arg("-c")
                .arg(&path)
                .stdout(fs::File::create(&expected_output_path).unwrap())
                .stderr(std::process::Stdio::null()) // Suppress stderr for invalid files
                .status();

            let bzip2_success = bzip2_status.map(|s| s.success()).unwrap_or(false);

            if !bzip2_success {
                println!(
                    "Skipping {:?} (invalid bzip2 file according to lbzip2)",
                    file_name
                );
                let _ = fs::remove_file(&expected_output_path);
                continue;
            }

            // 2. Run parallel_bzip2_cat
            let decoded_result = parallel_bzip2::parallel_bzip2_cat(&path);

            if let Err(e) = decoded_result {
                println!("FAILURE: parallel_bzip2_cat failed on {:?}: {}", path, e);
                failures.push(format!("parallel_bzip2_cat failed on {:?}", file_name));
                let _ = fs::remove_file(&expected_output_path);
                continue;
            }

            let decompressed_data = decoded_result.unwrap();

            // 3. Compare output
            if file_name == "gap.bz2" {
                let expected_data = fs::read(&expected_output_path).unwrap();
                if decompressed_data.starts_with(&expected_data) {
                    println!(
                        "SUCCESS: gap.bz2 matches prefix of lbzip2 output (and recovers more data)"
                    );
                } else {
                    println!("FAILURE: gap.bz2 does not match prefix of lbzip2 output");
                    failures.push(format!("Prefix mismatch on {:?}", file_name));
                }
            } else {
                let expected_md5 = calculate_md5_from_file(&expected_output_path);
                let actual_md5 = calculate_md5_from_bytes(&decompressed_data);

                if expected_md5 != actual_md5 {
                    println!("FAILURE: MD5 mismatch for {:?}", path);
                    println!("Expected: {}", expected_md5);
                    println!("Actual:   {}", actual_md5);
                    failures.push(format!("MD5 mismatch on {:?}", file_name));
                }
            }

            // Cleanup
            let _ = fs::remove_file(expected_output_path);
        }
    }

    if !failures.is_empty() {
        panic!("Regression tests failed:\n{}", failures.join("\n"));
    }
}
