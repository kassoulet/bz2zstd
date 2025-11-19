#!/bin/bash
set -e

# Compile
echo "Compiling..."
cargo build --release

# Create test data
echo "Creating test data..."
dd if=/dev/urandom of=test_tune.bin bs=1M count=10

# Compress with pbzip2
echo "Compressing with pbzip2..."
pbzip2 -f -k -p4 test_tune.bin

# Run with auto-tuning (no thread args)
echo "Running with auto-tuning..."
./target/release/bz2zstd --input test_tune.bin.bz2 --output test_tune.bin.zst 2> tune_log.txt

# Check for tuning message
if grep -q "Auto-tuning" tune_log.txt; then
    echo "✅ Auto-tuning triggered"
    cat tune_log.txt
else
    echo "❌ Auto-tuning NOT triggered"
    cat tune_log.txt
    exit 1
fi

# Verify output exists
if [ -f test_tune.bin.zst ]; then
    echo "✅ Output file created"
else
    echo "❌ Output file missing"
    exit 1
fi

# Cleanup
rm -f test_tune.bin test_tune.bin.bz2 test_tune.bin.zst tune_log.txt
