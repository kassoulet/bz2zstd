use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use parallel_bzip2::scan_blocks;
use pprof::criterion::{Output, PProfProfiler};
use std::fs;
use std::path::Path;
use std::process::Command;

/// Generate a test file of the specified size in MB
fn generate_test_file(size_mb: usize) -> String {
    let filename = format!("../bench_scan_{}_mb.bin", size_mb);
    let bz2_filename = format!("{}.bz2", filename);

    // Skip if already exists
    if Path::new(&bz2_filename).exists() {
        return bz2_filename;
    }

    println!("Generating {}MB test file for scanner...", size_mb);

    // Create random data
    let status = Command::new("dd")
        .args(&[
            "if=/dev/urandom",
            &format!("of={}", filename),
            "bs=1M",
            &format!("count={}", size_mb),
            "status=none",
        ])
        .status();

    if status.is_err() || !status.unwrap().success() {
        panic!("Failed to generate test data");
    }

    // Compress with bzip2
    let status = Command::new("bzip2")
        .args(&["-k", "-f", "-9", &filename])
        .status();

    if status.is_err() || !status.unwrap().success() {
        panic!("Failed to compress test data");
    }

    // Remove uncompressed file
    let _ = fs::remove_file(&filename);

    bz2_filename
}

fn bench_scanner(c: &mut Criterion) {
    let mut group = c.benchmark_group("scanner");

    for size_mb in [1, 10, 50].iter() {
        let bz2_file = generate_test_file(*size_mb);
        let data = std::fs::read(&bz2_file).expect("Failed to read test file");

        group.throughput(Throughput::Bytes(data.len() as u64));
        group.bench_with_input(
            BenchmarkId::from_parameter(format!("{}MB", size_mb)),
            &data,
            |b, data| {
                b.iter(|| {
                    let receiver = scan_blocks(data);
                    let mut count = 0;
                    while receiver.recv().is_ok() {
                        count += 1;
                    }
                    count
                })
            },
        );
    }

    group.finish();
}

fn bench_scanner_multistream(c: &mut Criterion) {
    // Generate a multi-stream file using pbzip2 if available
    let filename = "../bench_scan_multistream.bin";
    let bz2_filename = format!("{}.bz2", filename);

    if !Path::new(&bz2_filename).exists() {
        println!("Generating multi-stream test file for scanner...");

        // Create 10MB random data
        let status = Command::new("dd")
            .args(&[
                "if=/dev/urandom",
                &format!("of={}", filename),
                "bs=1M",
                "count=10",
                "status=none",
            ])
            .status();

        if status.is_ok() && status.unwrap().success() {
            // Try pbzip2 for multi-stream
            let pbzip2_status = Command::new("pbzip2")
                .args(&["-k", "-f", "-p4", filename])
                .status();

            if pbzip2_status.is_err() || !pbzip2_status.unwrap().success() {
                // Fallback to regular bzip2
                println!("pbzip2 not available, using bzip2 (single stream)");
                Command::new("bzip2")
                    .args(&["-k", "-f", filename])
                    .status()
                    .expect("Failed to compress");
            }

            let _ = fs::remove_file(filename);
        }
    }

    if !Path::new(&bz2_filename).exists() {
        println!("Skipping multistream scanner benchmark (file generation failed)");
        return;
    }

    let data = std::fs::read(&bz2_filename).expect("Failed to read test file");

    let mut group = c.benchmark_group("scanner_multistream");
    group.throughput(Throughput::Bytes(data.len() as u64));

    group.bench_function("scan_multistream", |b| {
        b.iter(|| {
            let receiver = scan_blocks(&data);
            let mut count = 0;
            while receiver.recv().is_ok() {
                count += 1;
            }
            count
        })
    });

    group.finish();
}

criterion_group! {
    name = benches;
    config = Criterion::default().with_profiler(PProfProfiler::new(100, Output::Flamegraph(None)));
    targets = bench_scanner, bench_scanner_multistream
}
criterion_main!(benches);
