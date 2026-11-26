# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Changed
- **Breaking:** `scan_blocks` now takes an `Arc<T: AsRef<[u8]>>` instead of `&[u8]`, avoiding a full copy of the input (e.g. memory-mapped files)

### Fixed
- Block CRC is now verified; corrupt or truncated blocks return an error instead of silently producing bad/short output
- `Bz2Decoder::read` returns an `InvalidData` error (after all preceding valid blocks) instead of a clean EOF when a block fails
- `Bz2Decoder` no longer buffers an unbounded number of out-of-order blocks
- `bz2zstd` writer no longer buffers an unbounded number of out-of-order blocks (new public `Throttle` helper shared with `Bz2Decoder`)
- `decompress_block_into` / `decompress_block` return an error instead of panicking on out-of-range bit offsets
- `Bz2Error` gains a `CrcMismatch` variant (breaking for exhaustive matches)

### Added
- `bz2zstd --verbose`: reports bzip2 level, stream/block counts, parallel setup, throughput and peak memory
- `bz2zstd` shows a progress bar (input read, %, speed, ETA) on interactive terminals
- Initial release of parallel_bzip2_decoder crate
- Parallel bzip2 decompression functionality
- Bz2Decoder with std::io::Read implementation
- Block scanning and parallel decompression pipeline
- Memory-mapped file support
- Support for both single-stream and multi-stream bzip2 files
- Comprehensive benchmarking suite
- Fuzz testing infrastructure
