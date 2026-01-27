# Sentinel Journal

## 2025-01-27 - [Bus Error via Memory Mapping and File Truncation]
**Vulnerability:** A `Bus error` (SIGBUS) occurred when the input file (memory-mapped) was the same as the output file (opened with `File::create`, which truncates it).
**Learning:** Memory mapping a file and then truncating it via another file handle in the same or another process leads to a crash when the memory-mapped region is accessed. This is a common pitfall when using `mmap` for performance.
**Prevention:** Always check if input and output paths refer to the same file (e.g., using `std::fs::canonicalize`) before opening the output file for writing, especially when the input is memory-mapped.

## 2025-01-27 - [Decompression Bomb Mitigation]
**Vulnerability:** Lack of size limits during block decompression could lead to resource exhaustion (DoS) if a malicious or malformed bzip2 block is processed.
**Learning:** Even if a format specifies a maximum block size (like 900KB for bzip2), a decoder should never trust the input and should always enforce a reasonable limit on the uncompressed output.
**Prevention:** Use `Read::take()` to limit the amount of data decompressed from a single block and return an error if the limit is exceeded.
