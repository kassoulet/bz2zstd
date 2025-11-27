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

### Recommended Settings to Prevent OOM

The fuzz targets have built-in protections against OOM:
- **Input size limit**: 1 MB maximum
- **Timeout**: 1 second per test case
- **Output limit**: 10 MB for decoder
- **Block limit**: 1000 blocks for scanner

For additional safety, set an RSS memory limit:

```bash
# Limit memory usage to 1 GB
cargo +nightly fuzz run fuzz_scanner -- -rss_limit_mb=1024
cargo +nightly fuzz run fuzz_decoder -- -rss_limit_mb=1024
```

### Quick Test (30 seconds each)

```bash
cargo +nightly fuzz run fuzz_scanner -- -max_total_time=30 -rss_limit_mb=1024
cargo +nightly fuzz run fuzz_decoder -- -max_total_time=30 -rss_limit_mb=1024
cargo +nightly fuzz run fuzz_block_decompress -- -max_total_time=30 -rss_limit_mb=1024
```

### Extended Fuzzing Session

For thorough testing, run for at least 1 hour with memory limits:

```bash
# Run scanner fuzzing for 1 hour with 1GB memory limit
cargo +nightly fuzz run fuzz_scanner -- -max_total_time=3600 -rss_limit_mb=1024

# Run decoder fuzzing for 1 hour with 1GB memory limit
cargo +nightly fuzz run fuzz_decoder -- -max_total_time=3600 -rss_limit_mb=1024

# Run block decompression fuzzing for 1 hour with 1GB memory limit
cargo +nightly fuzz run fuzz_block_decompress -- -max_total_time=3600 -rss_limit_mb=1024
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

## Memory Leak Detection

LibFuzzer includes LeakSanitizer (LSan) by default, which detects memory leaks at the end of each fuzzing run. However, you may see messages like "libFuzzer disabled leak detection after every mutation" - this is normal when the target accumulates memory in global state.

### Using trace_malloc

To get detailed malloc/free traces and identify memory accumulation:

```bash
# Run with malloc tracing (level 1 = basic, 2 = detailed)
cargo +nightly fuzz run fuzz_decoder -- -trace_malloc=2 -runs=100
```

This will show every allocation and deallocation, helping you identify:
- Memory that's allocated but never freed
- Gradual memory accumulation over iterations
- Unexpected allocation patterns

### LeakSanitizer at Shutdown

Even with per-mutation leak detection disabled, LSan still runs at process shutdown:

```bash
# Run for a fixed number of iterations to trigger shutdown leak check
cargo +nightly fuzz run fuzz_decoder -- -runs=1000
```

If there are leaks, you'll see a report like:
```
=================================================================
==12345==ERROR: LeakSanitizer: detected memory leaks

Direct leak of 1024 bytes in 1 object(s) allocated from:
    #0 0x... in malloc
    #1 0x... in your_function
    ...
```

### Using Valgrind for Leak Detection

For more detailed leak analysis, use Valgrind (slower but more thorough):

```bash
# Build without sanitizers for Valgrind compatibility
cd fuzz
cargo build --release --bin fuzz_decoder

# Run with Valgrind
valgrind --leak-check=full --show-leak-kinds=all \
  ./target/release/fuzz_decoder \
  corpus/fuzz_decoder/some_test_case
```

### Monitoring Memory Growth

Watch for gradual memory growth during fuzzing:

```bash
# In one terminal, start fuzzing
cargo +nightly fuzz run fuzz_decoder -- -rss_limit_mb=1024

# In another terminal, monitor memory usage
watch -n 1 'ps aux | grep fuzz_decoder | grep -v grep'
```

Look for:
- **Steady RSS growth**: Indicates a memory leak
- **Stable RSS**: Normal behavior (memory is being reused)
- **Periodic spikes**: May indicate temporary allocations that are cleaned up

### Using heaptrack (Linux)

For detailed heap profiling:

```bash
# Install heaptrack
sudo apt-get install heaptrack

# Profile a fuzz target
cd fuzz
cargo build --release --bin fuzz_decoder
heaptrack ./target/release/fuzz_decoder corpus/fuzz_decoder/some_test_case

# Analyze results
heaptrack_gui heaptrack.fuzz_decoder.*.gz
```

### Interpreting Results

**Normal behavior:**
- Memory usage stabilizes after initial corpus loading
- RSS stays relatively constant during fuzzing
- Small fluctuations are expected

**Memory leak indicators:**
- Continuous RSS growth over time
- RSS approaching the `-rss_limit_mb` limit
- "Out of memory" errors
- Slowdown over time as memory fills up

### Fixing Memory Leaks

Common causes in fuzz targets:
1. **Thread accumulation**: Spawned threads not being joined
2. **Channel accumulation**: Unbounded channel buffers
3. **Global state**: Data structures that grow indefinitely
4. **Forgotten cleanup**: Resources not being dropped

Example fix for thread accumulation:
```rust
// Before (leaks threads)
std::thread::spawn(|| { /* work */ });

// After (joins threads)
let handle = std::thread::spawn(|| { /* work */ });
handle.join().ok();
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

The fuzz targets have built-in OOM protections:
- Input size limited to 1 MB
- 1-second timeout per test case
- Output limited to 10 MB for decoder
- Block collection limited to 1000 for scanner

If you still encounter OOM:
1. Set a stricter RSS limit: `-- -rss_limit_mb=512`
2. Reduce number of workers: `-- -workers=1`
3. Monitor memory usage with `htop` or similar tools

## Further Reading

- [cargo-fuzz documentation](https://rust-fuzz.github.io/book/cargo-fuzz.html)
- [libFuzzer documentation](https://llvm.org/docs/LibFuzzer.html)
- [Rust Fuzz Book](https://rust-fuzz.github.io/book/)
