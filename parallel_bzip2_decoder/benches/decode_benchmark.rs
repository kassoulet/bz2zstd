use criterion::{criterion_group, criterion_main, Criterion, Throughput};
use memmap2::MmapOptions;
use parallel_bzip2_decoder::Bz2Decoder;
use pprof::criterion::{Output, PProfProfiler};
use std::fs::{self, File};
use std::io::Read;
use std::path::Path;
use std::process::Command;
use std::sync::Arc;

/// Generate a test file of the specified size in MB
fn generate_test_file(size_mb: usize) -> String {
    let filename = format!("../bench_data_{}_mb.bin", size_mb);
    let bz2_filename = format!("{}.bz2", filename);

    // Skip if already exists
    if Path::new(&bz2_filename).exists() {
        return bz2_filename;
    }

    println!("Generating {}MB test file...", size_mb);

    // Create random data
    let status = Command::new("dd")
        .args([
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
        .args(["-k", "-f", "-9", &filename])
        .status();

    if status.is_err() || !status.unwrap().success() {
        panic!("Failed to compress test data");
    }

    // Remove uncompressed file
    let _ = fs::remove_file(&filename);

    bz2_filename
}

fn bench_decode_size(c: &mut Criterion, size_mb: usize, name: &str) {
    let bz2_file = generate_test_file(size_mb);
    let file = File::open(&bz2_file).expect("Failed to open bench file");
    let mmap = unsafe { MmapOptions::new().map(&file).expect("Failed to mmap") };
    let mmap_arc = Arc::new(mmap);

    let mut group = c.benchmark_group(name);
    group.throughput(Throughput::Bytes(mmap_arc.len() as u64));
    group.sample_size(10); // Reduce sample size for larger files

    group.bench_function("parallel_bzip2_decoder", |b| {
        b.iter(|| {
            let mut decoder = Bz2Decoder::new(mmap_arc.clone());
            let mut buffer = [0u8; 8192];
            while decoder.read(&mut buffer).unwrap() > 0 {}
        })
    });

    group.bench_function("bzip2_crate", |b| {
        b.iter(|| {
            let mut decoder = bzip2::read::BzDecoder::new(&mmap_arc[..]);
            let mut buffer = [0u8; 8192];
            while decoder.read(&mut buffer).unwrap() > 0 {}
        })
    });

    group.finish();
}

fn bench_decode_1mb(c: &mut Criterion) {
    bench_decode_size(c, 1, "decode_1mb");
}

fn bench_decode_10mb(c: &mut Criterion) {
    bench_decode_size(c, 10, "decode_10mb");
}

fn bench_decode_50mb(c: &mut Criterion) {
    bench_decode_size(c, 50, "decode_50mb");
}

fn bench_multistream(c: &mut Criterion) {
    // Generate a multi-stream file using pbzip2 if available
    let filename = "../bench_multistream.bin";
    let bz2_filename = format!("{}.bz2", filename);

    if !Path::new(&bz2_filename).exists() {
        println!("Generating multi-stream test file...");

        // Create 10MB random data
        let status = Command::new("dd")
            .args([
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
                .args(["-k", "-f", "-p4", filename])
                .status();

            if pbzip2_status.is_err() || !pbzip2_status.unwrap().success() {
                // Fallback to regular bzip2
                println!("pbzip2 not available, using bzip2 (single stream)");
                Command::new("bzip2")
                    .args(["-k", "-f", filename])
                    .status()
                    .expect("Failed to compress");
            }

            let _ = fs::remove_file(filename);
        }
    }

    if !Path::new(&bz2_filename).exists() {
        println!("Skipping multistream benchmark (file generation failed)");
        return;
    }

    let file = File::open(&bz2_filename).expect("Failed to open bench file");
    let mmap = unsafe { MmapOptions::new().map(&file).expect("Failed to mmap") };
    let mmap_arc = Arc::new(mmap);

    let mut group = c.benchmark_group("decode_multistream");
    group.throughput(Throughput::Bytes(mmap_arc.len() as u64));
    group.sample_size(10);

    group.bench_function("parallel_bzip2_decoder", |b| {
        b.iter(|| {
            let mut decoder = Bz2Decoder::new(mmap_arc.clone());
            let mut buffer = [0u8; 8192];
            while decoder.read(&mut buffer).unwrap() > 0 {}
        })
    });

    group.finish();
}

criterion_group! {
    name = benches;
    config = Criterion::default().with_profiler(PProfProfiler::new(100, Output::Flamegraph(None)));
    targets = bench_decode_1mb, bench_decode_10mb, bench_decode_50mb, bench_multistream
}
criterion_main!(benches);
