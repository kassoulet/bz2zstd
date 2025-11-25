use criterion::{criterion_group, criterion_main, Criterion, Throughput};
use memmap2::MmapOptions;
use parallel_bzip2::Bz2Decoder;
use std::fs::File;
use std::io::Read;
use std::path::Path;
use std::sync::Arc;

fn bench_decode(c: &mut Criterion) {
    let input_path = Path::new("../test_e2e_limit.bin.bz2"); // Assuming this file exists or we generate it

    // Generate a test file if it doesn't exist
    if !input_path.exists() {
        use std::process::Command;
        // Create 10MB random file
        Command::new("dd")
            .args(&[
                "if=/dev/urandom",
                "of=../test_bench.bin",
                "bs=1M",
                "count=10",
            ])
            .status()
            .unwrap();
        Command::new("bzip2")
            .args(&["-k", "-f", "../test_bench.bin"])
            .status()
            .unwrap();
        // Rename to expected path for simplicity in this script context, or just use it
    }
    let bench_file = if input_path.exists() {
        input_path
    } else {
        Path::new("../test_bench.bin.bz2")
    };

    let file = File::open(bench_file).expect("Failed to open bench file");
    let mmap = unsafe { MmapOptions::new().map(&file).expect("Failed to mmap") };
    let mmap_arc = Arc::new(mmap);

    let mut group = c.benchmark_group("bzip2_decode");
    group.throughput(Throughput::Bytes(mmap_arc.len() as u64));

    group.bench_function("parallel_bzip2", |b| {
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

criterion_group!(benches, bench_decode);
criterion_main!(benches);
