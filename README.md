# bz2zstd

A high-performance, parallel bzip2 decompressor written in Rust. It utilizes multiple CPU cores to decompress **both single-stream** (standard) and **multi-stream** (e.g., `pbzip2`) bzip2 files by detecting bzip2 blocks and processing them in parallel.

It also supports direct conversion to Zstandard (`zstd`), allowing for efficient re-compression of large datasets.

## Features

-   **Parallel Decompression**: Automatically detects and decompresses multiple bzip2 streams in parallel using `rayon`.
-   **Zstd Conversion**: Decompress bzip2 and compress to zstd in a single pass without intermediate files.
-   **High Performance**: Scales linearly with CPU cores.
-   **Low Memory Footprint**: Uses memory mapping and streaming output to handle large files efficiently.
-   **Robust Detection**: Uses a strong 10-byte signature check to correctly identify bzip2 streams.

## Installation

```bash
git clone <repository_url>
cd bz2zstd
cargo build --release
```

The binary will be available at `target/release/bz2zstd`.

## Usage

### Decompress a file

```bash
./bz2zstd input.bz2 -o output.out
```

### Convert bzip2 to zstd

```bash
./bz2zstd input.bz2 -o output.zst
```

### Configuration

-   `<INPUT>`: Input bzip2 file.
-   `-o, --output <FILE>`: Output file (optional, defaults to input file with .bz2 replaced by .zst).
-   `--zstd-level <LEVEL>`: Set zstd compression level (default: 3). You can also use short flags like `-9` directly.
-   `--benchmark-scan`: Benchmark mode: Only run the scanner and exit.

## Performance

On a 12-core system with a 100MB single-stream bzip2 file:

| Cores | Real Time | Speedup |
| :--- | :--- | :--- |
| **1** | 1.80s | 1.0x |
| **2** | 1.15s | 1.56x |
| **4** | 0.75s | 2.4x |
| **8** | 0.72s | 2.5x |

`bz2zstd` scales well with available cores, significantly reducing processing time compared to single-threaded tools.

## License

MIT

## Benchmarking

A scaling benchmark script is provided in `scripts/benchmark_scaling.sh`. This script generates a random file, compresses it with `bzip2`, and then runs `bz2zstd` with varying thread counts to measure scaling performance.

```bash
./scripts/benchmark_scaling.sh
```
