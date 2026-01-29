## 2025-05-15 - [Decompression Bomb and Path Collision Protection]
**Vulnerability:** Resource exhaustion via decompression bombs and application crash (SIGBUS) due to self-overwriting memory-mapped files.
**Learning:** Bzip2 blocks are typically 900KB but can be maliciously crafted. Memory-mapped files in Rust can trigger a SIGBUS if the underlying file is truncated (e.g., via `File::create`).
**Prevention:** Enforce a strict uncompressed size limit per block (2MB) using `Read::take`. Use `std::fs::canonicalize` to ensure input and output file paths are distinct before opening any output file.
