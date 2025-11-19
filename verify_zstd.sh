#!/bin/bash
set -e

# Compile
echo "Compiling..."
cargo build --release

# Create test data
echo "Creating test data..."
dd if=/dev/urandom of=test_zstd.bin bs=1M count=10
ORIG_SUM=$(md5sum test_zstd.bin | awk '{print $1}')

# Compress with pbzip2
echo "Compressing with pbzip2..."
pbzip2 -f -k -p4 test_zstd.bin

# Convert to zstd with specific level
echo "Converting to zstd (level 1)..."
./target/release/bz2zstd --input test_zstd.bin.bz2 --output test_zstd.bin.zst --zstd-threads 2 --zstd-level 1

# Decompress zstd and verify
echo "Verifying zstd output..."
zstd -d -f test_zstd.bin.zst
NEW_SUM=$(md5sum test_zstd.bin | awk '{print $1}')

if [ "$ORIG_SUM" == "$NEW_SUM" ]; then
    echo "✅ Zstd conversion test PASSED"
else
    echo "❌ Zstd conversion test FAILED"
    exit 1
fi

# Cleanup
rm -f test_zstd.bin test_zstd.bin.bz2 test_zstd.bin.zst
