# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Changed
- **Breaking:** `scan_blocks` now takes an `Arc<T: AsRef<[u8]>>` instead of `&[u8]`, avoiding a full copy of the input (e.g. memory-mapped files)

### Added
- Initial release of parallel_bzip2_decoder crate
- Parallel bzip2 decompression functionality
- Bz2Decoder with std::io::Read implementation
- Block scanning and parallel decompression pipeline
- Memory-mapped file support
- Support for both single-stream and multi-stream bzip2 files
- Comprehensive benchmarking suite
- Fuzz testing infrastructure
