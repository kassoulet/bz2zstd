#!/bin/bash
set -e

# Compile the project
echo "Compiling..."
cargo build --release

# Create a dummy file (10MB)
echo "Creating test data..."
dd if=/dev/urandom of=test_data.bin bs=1M count=10

# Calculate original checksum
ORIG_SUM=$(md5sum test_data.bin | awk '{print $1}')
echo "Original MD5: $ORIG_SUM"

# Test 1: pbzip2 (Multi-stream)
echo "Testing pbzip2 (Multi-stream)..."
pbzip2 -k -f -p4 test_data.bin
./target/release/bz2zstd test_data.bin.bz2 --output test_output_pbzip2.bin.zst
zstd -d -f test_output_pbzip2.bin.zst -o test_output_pbzip2.bin
OUT_SUM=$(md5sum test_output_pbzip2.bin | awk '{print $1}')

if [ "$ORIG_SUM" == "$OUT_SUM" ]; then
    echo "✅ pbzip2 test PASSED"
else
    echo "❌ pbzip2 test FAILED"
    exit 1
fi

# Test 2: Concatenated bzip2 streams
echo "Testing concatenated bzip2 streams..."
split -b 2M test_data.bin chunk_
rm -f concat.bz2
for f in chunk_*; do
    bzip2 -c $f >> concat.bz2
done

./target/release/bz2zstd concat.bz2 --output test_output_concat.bin.zst
zstd -d -f test_output_concat.bin.zst -o test_output_concat.bin
OUT_SUM_CONCAT=$(md5sum test_output_concat.bin | awk '{print $1}')

if [ "$ORIG_SUM" == "$OUT_SUM_CONCAT" ]; then
    echo "✅ Concatenated streams test PASSED"
else
    echo "❌ Concatenated streams test FAILED"
    exit 1
fi

# Test 3: Default output filename
echo "Testing default output filename..."
cp test_data.bin.bz2 test_default.bz2
./target/release/bz2zstd test_default.bz2
if [ -f test_default.zst ]; then
    echo "✅ Default output filename test PASSED"
    rm test_default.zst test_default.bz2
else
    echo "❌ Default output filename test FAILED"
    exit 1
fi

# Cleanup
rm -f test_data.bin test_data.bin.bz2 test_output_pbzip2.bin chunk_* concat.bz2 test_output_concat.bin
echo "All tests passed!"
