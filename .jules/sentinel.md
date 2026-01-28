## 2025-05-22 - SIGBUS on Memory-Mapped File Truncation
**Vulnerability:** A SIGBUS crash occurs when an output file truncates the input file while it is currently memory-mapped.
**Learning:** `File::create` truncates the file immediately. If that file is backed by a `mmap`, any subsequent access to the mapping results in a SIGBUS signal because the mapped pages no longer exist in the file.
**Prevention:** Always use `std::fs::canonicalize` to verify that input and output file paths are distinct before opening any file for writing, especially when using memory mapping.
