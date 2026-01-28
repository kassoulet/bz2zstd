# Sentinel Journal

## 2025-01-27 - [Bus Error via Memory Mapping and File Truncation]
**Vulnerability:** A `Bus error` (SIGBUS) occurred when the input file (memory-mapped) was the same as the output file (opened with `File::create`, which truncates it).
**Learning:** Memory mapping a file and then truncating it via another file handle in the same or another process leads to a crash when the memory-mapped region is accessed. Standard path comparison with `canonicalize` is insufficient if hardlinks are used.
**Prevention:** Check if input and output paths refer to the same file using `std::fs::canonicalize` and, on Unix systems, compare device and inode numbers to catch hardlinks.

## 2025-01-27 - [Decompression Bomb Mitigation]
**Vulnerability:** Lack of size limits during block decompression could lead to resource exhaustion (DoS) if a malicious or malformed bzip2 block is processed.
**Learning:** Even if a format specifies a maximum block size (like 900KB for standard bzip2), a decoder should never trust the input. A 2MB limit was chosen as it is more than double the standard maximum, allowing for safe margins while preventing massive memory allocation from malformed blocks.
**Prevention:** Use `Read::take()` to limit the amount of data decompressed from a single block and return an error if the limit is exceeded.
