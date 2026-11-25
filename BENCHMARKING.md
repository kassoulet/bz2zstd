# Benchmarking and Profiling Guide

This guide explains how to run performance benchmarks and profiling for the `parallel_bzip2` crate.

## Quick Start

### Running All Benchmarks

```bash
cd parallel_bzip2
cargo bench
```

This will run all benchmarks and generate an HTML report at `target/criterion/report/index.html`.

### Running Specific Benchmarks

```bash
# Decode benchmarks (1MB, 10MB, 50MB, multistream)
cargo bench --bench decode_benchmark

# Scanner benchmarks
cargo bench --bench scanner_benchmark

# End-to-end benchmarks
cargo bench --bench e2e_benchmark
```

## Benchmark Suites

### Decode Benchmark

Tests decompression performance with various file sizes:

- **decode_1mb**: Small file decompression (1MB)
- **decode_10mb**: Medium file decompression (10MB)
- **decode_50mb**: Large file decompression (50MB)
- **decode_multistream**: Multi-stream file decompression

Each test compares `parallel_bzip2` against the reference `bzip2` crate.

### Scanner Benchmark

Tests block scanning performance:

- Scans files of different sizes (1MB, 10MB, 50MB)
- Measures throughput in MB/s
- Tests both single-stream and multi-stream files

### End-to-End Benchmark

Tests the full decompression pipeline:

- **e2e_fixtures**: Tests with real fixture files from the test suite
- **e2e_memory**: Tests memory usage with different buffer sizes (1KB, 8KB, 64KB)
- **e2e_pipeline**: Full pipeline from file open to decompression

## CPU Profiling

### Using the Profiling Script

```bash
cd scripts
./profile_cpu.sh
```

This will:
1. Run all benchmarks with CPU profiling enabled
2. Generate flamegraphs for each benchmark
3. Create an HTML report with detailed performance metrics

### Viewing Flamegraphs

Flamegraphs are saved in `target/criterion/<benchmark>/<test>/profile/flamegraph.svg`.

To view:
```bash
# Open in browser
firefox target/criterion/decode_1mb/parallel_bzip2/profile/flamegraph.svg

# Or use any web browser
open target/criterion/decode_1mb/parallel_bzip2/profile/flamegraph.svg
```

### Interpreting Flamegraphs

- **Width**: Time spent in a function (wider = more time)
- **Height**: Call stack depth
- **Color**: Random (for visual distinction)
- **Hot paths**: Look for wide bars at the top of the stack

### Alternative: cargo-flamegraph

Install and use `cargo-flamegraph` for interactive profiling:

```bash
# Install
cargo install flamegraph

# Profile a specific benchmark
cargo flamegraph --bench decode_benchmark -- --bench
```

## Memory Profiling

### Using the Memory Profiling Script

```bash
cd scripts
./profile_memory.sh
```

This requires `valgrind` to be installed:
```bash
# Debian/Ubuntu
sudo apt-get install valgrind

# RHEL/CentOS
sudo yum install valgrind
```

### Viewing Memory Profiles

Memory profiles are saved in the `profiling/` directory.

#### Text Output
```bash
ms_print profiling/massif.decode.out
ms_print profiling/massif.scanner.out
ms_print profiling/massif.e2e.out
```

#### Graphical Viewer
```bash
# Install massif-visualizer
sudo apt-get install massif-visualizer

# View profile
massif-visualizer profiling/massif.decode.out
```

### Memory Leak Detection

Check the memory leak report:
```bash
cat profiling/memcheck.log
```

Look for:
- **Definitely lost**: Memory leaks that need fixing
- **Indirectly lost**: Memory leaks from lost parent blocks
- **Possibly lost**: Potential leaks (may be false positives)

## Performance Regression Testing

### Baseline Metrics

Save baseline performance metrics:
```bash
cargo bench -- --save-baseline main
```

### Compare Against Baseline

After making changes:
```bash
cargo bench -- --baseline main
```

This will show performance differences compared to the baseline.

### CI Integration

Add to your CI pipeline:
```yaml
- name: Run benchmarks
  run: |
    cd parallel_bzip2
    cargo bench --bench decode_benchmark -- --test
    cargo bench --bench scanner_benchmark -- --test
    cargo bench --bench e2e_benchmark -- --test
```

The `--test` flag runs a quick validation without full benchmarking.

## Benchmark Data

Benchmarks automatically generate test files:
- `bench_data_*_mb.bin.bz2`: Single-stream compressed files
- `bench_scan_*_mb.bin.bz2`: Files for scanner benchmarks
- `bench_multistream.bin.bz2`: Multi-stream compressed file
- `bench_e2e_*.bin.bz2`: End-to-end test files

These files are cached and reused across benchmark runs. They are gitignored.

## Tips and Best Practices

### Reducing Noise

1. **Close other applications**: Minimize background processes
2. **Disable CPU frequency scaling**: Use performance governor
   ```bash
   sudo cpupower frequency-set --governor performance
   ```
3. **Run multiple iterations**: Criterion runs multiple iterations by default

### Benchmark Configuration

Modify benchmark configuration in the benchmark files:

```rust
let mut group = c.benchmark_group("my_benchmark");
group.sample_size(100);           // Number of samples
group.measurement_time(Duration::from_secs(10));  // Time per benchmark
group.warm_up_time(Duration::from_secs(3));       // Warm-up time
```

### Profiling Specific Functions

To profile specific code paths, use criterion's `iter_batched`:

```rust
group.bench_function("my_test", |b| {
    b.iter_batched(
        || setup_data(),           // Setup (not measured)
        |data| process(data),      // Measured code
        BatchSize::SmallInput,
    )
});
```

## Troubleshooting

### Benchmark Fails to Generate Test Files

Ensure `dd` and `bzip2` are installed:
```bash
which dd bzip2
```

### Flamegraphs Not Generated

Check that `pprof` feature is enabled in `Cargo.toml`:
```toml
pprof = { version = "0.13", features = ["flamegraph", "criterion"] }
```

### Memory Profiling Too Slow

Use `--test` flag for quick validation:
```bash
valgrind --tool=massif ../target/release/deps/decode_benchmark-* --bench --test
```

### Permission Denied on Scripts

Make scripts executable:
```bash
chmod +x scripts/profile_cpu.sh scripts/profile_memory.sh
```

## Further Reading

- [Criterion.rs Documentation](https://bheisler.github.io/criterion.rs/book/)
- [Flamegraph Interpretation](http://www.brendangregg.com/flamegraphs.html)
- [Valgrind Massif Manual](https://valgrind.org/docs/manual/ms-manual.html)
- [Rust Performance Book](https://nnethercote.github.io/perf-book/)
