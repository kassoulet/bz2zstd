# bz2zstd

A high-performance, parallel bzip2 decompressor written in Rust. It is designed to handle "multi-stream" bzip2 files (like those created by `pbzip2` or concatenated streams) by utilizing multiple CPU cores for decompression.

It also supports direct conversion to Zstandard (`zstd`), allowing for efficient re-compression of large datasets.

## Features

-   **Parallel Decompression**: Automatically detects and decompresses multiple bzip2 streams in parallel using `rayon`.
-   **Zstd Conversion**: Decompress bzip2 and compress to zstd in a single pass without intermediate files.
-   **High Performance**: Scales linearly with CPU cores (verified ~933% CPU usage on 12 cores).
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
./bz2zstd --input file.bz2 --output file.out
```

### Convert bzip2 to zstd

```bash
./bz2zstd --input file.bz2 --output file.zst
```

### Configuration

-   `--input <FILE>`: Input bzip2 file.
-   `--output <FILE>`: Output file (optional, defaults to stdout).
-   `--zstd-level <LEVEL>`: Set zstd compression level (default: 3).
-   `--zstd-threads <NUM>`: Number of threads for zstd compression (0 = auto).
-   `--decomp-threads <NUM>`: Number of threads for bzip2 decompression (default: num_cpus - 1).

## Performance

On a 12-core system with a 200MB multi-stream bzip2 file:

| Tool | Real Time | CPU Usage |
| :--- | :--- | :--- |
| **bz2zstd** | 2.23s | 933% |
| **pbzip2** | 2.12s | 996% |
| **lbzip2** | 2.01s | 1041% |

`bz2zstd` offers performance comparable to industry-standard C/C++ tools while providing memory safety and easy zstd integration.

## License

MIT

## Benchmarking

A scaling benchmark script is provided in `scripts/benchmark_scaling.sh`. This script generates a random file, compresses it with `bzip2`, and then runs `bz2zstd` with varying thread counts to measure scaling performance.

```bash
./scripts/benchmark_scaling.sh
```
