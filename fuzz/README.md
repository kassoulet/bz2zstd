# Fuzz Testing for parallel_bzip2

This directory contains fuzz testing infrastructure for the `parallel_bzip2` crate using `cargo-fuzz`.

## Prerequisites

1. **Nightly Rust toolchain**:
   ```bash
   rustup install nightly
   ```

2. **cargo-fuzz** (already installed if you're reading this):
   ```bash
   cargo install cargo-fuzz
   ```

## Fuzz Targets

### 1. `fuzz_scanner` - Block Scanner Fuzzing

Tests the block marker detection logic with arbitrary byte inputs.

**What it tests:**
- Scanner doesn't panic on malformed data
- Detected block markers are within valid bit ranges
- No infinite loops or hangs

**Run it:**
```bash
cargo +nightly fuzz run fuzz_scanner
```

### 2. `fuzz_decoder` - Full Decoder Fuzzing

Tests the complete decompression pipeline with arbitrary inputs.

**What it tests:**
- Decoder handles invalid bz2 data gracefully
- No panics, deadlocks, or memory issues
- Output limits prevent OOM conditions

**Run it:**
```bash
cargo +nightly fuzz run fuzz_decoder
```

### 3. `fuzz_block_decompress` - Block Decompression Fuzzing

Tests individual block decompression with arbitrary bit ranges.

**What it tests:**
- Edge cases like zero-length ranges
- Bit ranges beyond data length
- Unaligned bit positions
- Overlapping ranges

**Run it:**
```bash
cargo +nightly fuzz run fuzz_block_decompress
```

## Running Fuzz Tests

### Quick Test (30 seconds each)

```bash
cargo +nightly fuzz run fuzz_scanner -- -max_total_time=30
cargo +nightly fuzz run fuzz_decoder -- -max_total_time=30
cargo +nightly fuzz run fuzz_block_decompress -- -max_total_time=30
```

### Extended Fuzzing Session

For thorough testing, run for at least 1 hour:

```bash
# Run scanner fuzzing for 1 hour
cargo +nightly fuzz run fuzz_scanner -- -max_total_time=3600

# Run decoder fuzzing for 1 hour
cargo +nightly fuzz run fuzz_decoder -- -max_total_time=3600

# Run block decompression fuzzing for 1 hour
cargo +nightly fuzz run fuzz_block_decompress -- -max_total_time=3600
```

### Parallel Fuzzing

Run multiple jobs in parallel for faster coverage:

```bash
cargo +nightly fuzz run fuzz_decoder -- -workers=4
```

## Corpus Management

The corpus (interesting test cases) is stored in:
- `corpus/fuzz_scanner/`
- `corpus/fuzz_decoder/`
- `corpus/fuzz_block_decompress/`

These directories are seeded with test fixtures from `parallel_bzip2/tests/fixtures/`.

### Minimizing Corpus

Reduce corpus size while maintaining coverage:

```bash
cargo +nightly fuzz cmin fuzz_decoder
cargo +nightly fuzz cmin fuzz_scanner
cargo +nightly fuzz cmin fuzz_block_decompress
```

## Handling Crashes

### Reproducing a Crash

When a crash is found, it's saved in `artifacts/fuzz_<target>/`:

```bash
# Reproduce the crash
cargo +nightly fuzz run fuzz_decoder artifacts/fuzz_decoder/crash-<hash>
```

### Minimizing a Crash

Reduce the crashing input to the smallest possible size:

```bash
cargo +nightly fuzz tmin fuzz_decoder artifacts/fuzz_decoder/crash-<hash>
```

### Debugging a Crash

View the debug output of the crashing input:

```bash
cargo +nightly fuzz fmt fuzz_decoder artifacts/fuzz_decoder/crash-<hash>
```

## Coverage Analysis

Generate coverage information:

```bash
cargo +nightly fuzz coverage fuzz_decoder
```

## Performance Tips

### Disable Sanitizers for Safe Rust

If your code is 100% safe Rust (no `unsafe` blocks, no C/C++ FFI), you can significantly boost performance:

```bash
cargo +nightly fuzz run fuzz_decoder -- -sanitizer=none
```

**Note:** The `parallel_bzip2` crate uses the `bzip2` crate which contains unsafe code, so sanitizers should generally be enabled.

### Adjust Memory Limits

Limit memory usage to prevent OOM:

```bash
cargo +nightly fuzz run fuzz_decoder -- -rss_limit_mb=2048
```

## Continuous Fuzzing

For CI/CD integration, you can run fuzzing for a fixed duration:

```bash
#!/bin/bash
# Run each fuzz target for 5 minutes
for target in fuzz_scanner fuzz_decoder fuzz_block_decompress; do
    cargo +nightly fuzz run $target -- -max_total_time=300 || exit 1
done
```

## Troubleshooting

### "error: no such subcommand: `fuzz`"

Make sure you're using the nightly toolchain:
```bash
cargo +nightly fuzz --version
```

### Slow Fuzzing Performance

1. Build in release mode (default for cargo-fuzz)
2. Use multiple workers: `-- -workers=N`
3. Consider disabling sanitizers if appropriate

### Out of Memory Errors

1. Reduce input size limits in fuzz targets
2. Set RSS limit: `-- -rss_limit_mb=2048`
3. Reduce number of workers

## Further Reading

- [cargo-fuzz documentation](https://rust-fuzz.github.io/book/cargo-fuzz.html)
- [libFuzzer documentation](https://llvm.org/docs/LibFuzzer.html)
- [Rust Fuzz Book](https://rust-fuzz.github.io/book/)
