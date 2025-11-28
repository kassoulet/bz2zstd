use criterion::{criterion_group, criterion_main, Criterion, Throughput};
use memmap2::MmapOptions;
use parallel_bzip2::Bz2Decoder;
use pprof::criterion::{Output, PProfProfiler};
use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};

/// Get paths to test fixture files
fn get_fixture_files() -> Vec<PathBuf> {
    let fixtures_dir = Path::new("tests/fixtures");
    if !fixtures_dir.exists() {
        return Vec::new();
    }

    std::fs::read_dir(fixtures_dir)
        .expect("Failed to read fixtures directory")
        .filter_map(|entry| {
            let entry = entry.ok()?;
            let path = entry.path();
            if path.extension()? == "bz2" {
                Some(path)
            } else {
                None
            }
        })
        .collect()
}

fn bench_e2e_fixtures(c: &mut Criterion) {
    let fixtures = get_fixture_files();

    if fixtures.is_empty() {
        println!("No fixture files found, skipping fixture benchmarks");
        return;
    }

    let mut group = c.benchmark_group("e2e_fixtures");

    for fixture_path in fixtures.iter().take(5) {
        // Limit to 5 to avoid too many benchmarks
        let file_name = fixture_path.file_name().unwrap().to_string_lossy();

        let data = std::fs::read(fixture_path).expect("Failed to read fixture");
        group.throughput(Throughput::Bytes(data.len() as u64));

        group.bench_function(file_name.as_ref(), |b| {
            b.iter(|| {
                let mut decoder = Bz2Decoder::new(std::sync::Arc::new(data.clone()));
                let mut output = Vec::new();
                decoder.read_to_end(&mut output).unwrap();
                output
            })
        });
    }

    group.finish();
}

fn bench_e2e_memory_usage(c: &mut Criterion) {
    // Generate a 10MB test file
    let filename = "../bench_e2e_memory.bin";
    let bz2_filename = format!("{}.bz2", filename);

    if !Path::new(&bz2_filename).exists() {
        println!("Generating test file for memory benchmark...");

        let status = std::process::Command::new("dd")
            .args(&[
                "if=/dev/urandom",
                &format!("of={}", filename),
                "bs=1M",
                "count=10",
                "status=none",
            ])
            .status();

        if status.is_ok() && status.unwrap().success() {
            std::process::Command::new("bzip2")
                .args(&["-k", "-f", "-9", filename])
                .status()
                .expect("Failed to compress");

            let _ = fs::remove_file(filename);
        }
    }

    if !Path::new(&bz2_filename).exists() {
        println!("Skipping memory benchmark (file generation failed)");
        return;
    }

    let file = File::open(&bz2_filename).expect("Failed to open bench file");
    let mmap = unsafe { MmapOptions::new().map(&file).expect("Failed to mmap") };
    let mmap_arc = std::sync::Arc::new(mmap);

    let mut group = c.benchmark_group("e2e_memory");
    group.throughput(Throughput::Bytes(mmap_arc.len() as u64));

    // Benchmark with different buffer sizes to measure memory impact
    for buffer_size in [1024, 8192, 65536].iter() {
        group.bench_function(format!("buffer_{}", buffer_size), |b| {
            b.iter(|| {
                let mut decoder = Bz2Decoder::new(mmap_arc.clone());
                let mut buffer = vec![0u8; *buffer_size];
                let mut total = 0;
                loop {
                    let n = decoder.read(&mut buffer).unwrap();
                    if n == 0 {
                        break;
                    }
                    total += n;
                }
                total
            })
        });
    }

    group.finish();
}

fn bench_e2e_full_pipeline(c: &mut Criterion) {
    // Test the full pipeline: open file -> decompress -> output
    let filename = "../bench_e2e_pipeline.bin";
    let bz2_filename = format!("{}.bz2", filename);

    if !Path::new(&bz2_filename).exists() {
        println!("Generating test file for pipeline benchmark...");

        let status = std::process::Command::new("dd")
            .args(&[
                "if=/dev/urandom",
                &format!("of={}", filename),
                "bs=1M",
                "count=5",
                "status=none",
            ])
            .status();

        if status.is_ok() && status.unwrap().success() {
            std::process::Command::new("bzip2")
                .args(&["-k", "-f", "-9", filename])
                .status()
                .expect("Failed to compress");

            let _ = fs::remove_file(filename);
        }
    }

    if !Path::new(&bz2_filename).exists() {
        println!("Skipping pipeline benchmark (file generation failed)");
        return;
    }

    let mut group = c.benchmark_group("e2e_pipeline");

    group.bench_function("parallel_bzip2_full", |b| {
        b.iter(|| {
            let mut decoder = Bz2Decoder::open(&bz2_filename).unwrap();
            let mut output = Vec::new();
            decoder.read_to_end(&mut output).unwrap();
            output.len()
        })
    });

    group.bench_function("bzip2_crate_full", |b| {
        b.iter(|| {
            let file = File::open(&bz2_filename).unwrap();
            let mut decoder = bzip2::read::BzDecoder::new(file);
            let mut output = Vec::new();
            decoder.read_to_end(&mut output).unwrap();
            output.len()
        })
    });

    group.finish();
}

criterion_group! {
    name = benches;
    config = Criterion::default().with_profiler(PProfProfiler::new(100, Output::Flamegraph(None)));
    targets = bench_e2e_fixtures, bench_e2e_memory_usage, bench_e2e_full_pipeline
}
criterion_main!(benches);
