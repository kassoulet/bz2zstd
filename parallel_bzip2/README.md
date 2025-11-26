# parallel_bzip2

A high-performance, parallel bzip2 decoder for Rust.

This crate provides a `Bz2Decoder` that implements `std::io::Read`, allowing you to decompress bzip2 files in parallel using multiple CPU cores. It is designed to work efficiently with both single-stream (standard) and multi-stream (e.g., `pbzip2`) bzip2 files by scanning for block boundaries and decompressing them concurrently.

## Features

- **Parallel Decompression**: Utilizes `rayon` to decompress blocks in parallel.
- **Standard API**: Implements `std::io::Read` for easy integration.
- **Memory Mapped**: Efficiently handles large files using memory mapping.
- **Flexible**: Supports opening files directly or working with in-memory buffers (via `Arc`).

## Usage

Add this to your `Cargo.toml`:

```toml
[dependencies]
parallel_bzip2 = "0.1"
```

### Decompressing a File

The easiest way to use `parallel_bzip2` is to use `Bz2Decoder::open`, which handles memory mapping internally:

```rust
use parallel_bzip2::Bz2Decoder;
use std::io::Read;

fn main() -> anyhow::Result<()> {
    let mut decoder = Bz2Decoder::open("input.bz2")?;
    let mut buffer = Vec::new();
    decoder.read_to_end(&mut buffer)?;
    println!("Decompressed {} bytes", buffer.len());
    Ok(())
}
```

### Decompressing from Memory

If you already have the data in memory (e.g., an `Arc<[u8]>` or `Arc<Mmap>`), you can use `Bz2Decoder::new`:

```rust
use parallel_bzip2::Bz2Decoder;
use std::io::Read;
use std::sync::Arc;

fn main() -> anyhow::Result<()> {
    let data: Vec<u8> = vec![/* ... bzip2 data ... */];
    let data_arc = Arc::new(data);
    let mut decoder = Bz2Decoder::new(data_arc);

    let mut buffer = Vec::new();
    decoder.read_to_end(&mut buffer)?;
    Ok(())
}
```

## Performance

`parallel_bzip2` scales linearly with the number of available CPU cores. It is significantly faster than standard single-threaded decoders for large files.

## License

MIT
