# bz2zstd

A high-performance, parallel bzip2 decompressor and bzip2-to-zstd converter written in Rust. It utilizes multiple CPU cores to decompress **both single-stream** (standard) and **multi-stream** (e.g., `pbzip2`) bzip2 files by detecting bzip2 blocks and processing them in parallel.

It also supports direct conversion to Zstandard (`zstd`), allowing for efficient re-compression of large datasets in a single pass.

## Features

-   **Parallel Decompression**: Automatically detects and decompresses multiple bzip2 streams in parallel using `rayon`.
-   **Zstd Conversion**: Decompress bzip2 and compress to zstd in a single pass without intermediate files.
-   **High Performance**: Scales linearly with CPU cores.
-   **Low Memory Footprint**: Uses memory mapping and streaming output to handle large files efficiently.
-   **Robust Detection**: Uses a strong 10-byte signature check to correctly identify bzip2 streams.
-   **Library Support**: Standalone `parallel_bzip2_decoder` crate for integration into other projects
-   **Cross-platform**: Works on Linux, macOS, and Windows

## Installation

### From source

```bash
git clone https://github.com/parallel-bz2/parallel-bz2.git
cd parallel-bz2
cargo build --release
```

The binary will be available at `target/release/bz2zstd`.

### From crates.io (when published)

```bash
cargo install bz2zstd
```

## Library

The core parallel decompression logic is available as a standalone library crate: `parallel_bzip2_decoder`.

```toml
[dependencies]
parallel_bzip2_decoder = { path = "parallel_bzip2_decoder" }
```

See `parallel_bzip2_decoder/README.md` for more details.

## Installation

```bash
git clone <repository_url>
cd parallel-bz2
cargo build --release
```

The binary will be available at `target/release/bz2zstd`.

## Usage

### Convert bzip2 to zstd

```bash
./bz2zstd input.bz2
```

### Configuration

-   `<INPUT>`: Input bzip2 file.
-   `-o, --output <FILE>`: Output file (optional, defaults to input file with .bz2 replaced by .zst).
-   `-z, --zstd-level <LEVEL>`: Set zstd compression level (default: 3, e.g., `-z 9`).
-   `-j, --jobs <N>`: Number of threads to use (default: number of logical cores).
-   `--benchmark-scan`: Benchmark mode: Only run the scanner and exit.

## Contributing

We welcome contributions! Please see our [contributing guidelines](CONTRIBUTING.md) for details on how to get started.

## Development

To run tests:
```bash
cargo test
```

To run benchmarks:
```bash
cargo bench
```

To run with profiling (see `./scripts/` for profiling scripts):
```bash
./scripts/profile_cpu.sh
```

## License

MIT

## Acknowledgments

This project uses and was inspired by various other bzip2 decompression tools and libraries in the ecosystem.
