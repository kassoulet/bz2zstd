#!/bin/bash
set -e

# Configuration
DATA_SIZE_MB=100
INPUT_FILE="bench_comparison.bin"
COMPRESSED_FILE="bench_comparison.bin.bz2"
RESULT_FILE="benchmark_comparison_results.txt"

# Check dependencies
check_dependency() {
    if ! command -v $1 &> /dev/null; then
        echo "Error: Required command '$1' not found. Please install it."
        exit 1
    fi
}

check_dependency "cargo"
check_dependency "dd"
check_dependency "bzip2"
check_dependency "lbzip2"
check_dependency "zstd"
check_dependency "/usr/bin/time"

echo "Preparing comparison benchmark..." > $RESULT_FILE

# Compile
echo "Compiling parallel-bz2..."
cargo build --release >> $RESULT_FILE 2>&1

# Generate Data
if [ ! -f "$COMPRESSED_FILE" ]; then
    echo "Generating ${DATA_SIZE_MB}MB of random data..."
    dd if=/dev/urandom of=$INPUT_FILE bs=1M count=$DATA_SIZE_MB status=none

    # Compress using standard bzip2 to ensure SINGLE STREAM
    echo "Compressing with bzip2 (single stream)..."
    bzip2 -k -f $INPUT_FILE
fi

echo "---------------------------------------------------" >> $RESULT_FILE
echo "Comparison Benchmark Results (Data Size: ${DATA_SIZE_MB}MB, Single Stream)" >> $RESULT_FILE
echo "System Cores: $(nproc)" >> $RESULT_FILE
echo "---------------------------------------------------" >> $RESULT_FILE

run_bench() {
    NAME=$1
    CMD=$2

    echo "Running $NAME..."
    echo "[$NAME]" >> $RESULT_FILE

    # Run and capture time. We use a subshell to avoid variable pollution if needed,
    # but mainly to capture the time output specifically.
    /usr/bin/time -f "Real: %e s" bash -c "$CMD" 2>> $RESULT_FILE

    echo "" >> $RESULT_FILE
}

# 1. bz2zstd (8 cores)
# We use -j 8 to force 8 threads for fair comparison if the machine has more,
# or to match the requested 8-core test.
run_bench "bz2zstd (8 threads)" "./target/release/bz2zstd -j 8 $COMPRESSED_FILE -o out_bz2zstd.zst"

# 2. bzip2 + zstd (Baseline)
run_bench "bzip2 + zstd (1 thread)" "bzip2 -d -c $COMPRESSED_FILE | zstd -3 > out_bzip2.zst"

# 3. lbzip2 (8 cores) + zstd
# lbzip2 -n 8 forces 8 threads
run_bench "lbzip2 (8 threads) + zstd" "lbzip2 -n 8 -d -c $COMPRESSED_FILE | zstd -3 > out_lbzip2.zst"

# Cleanup outputs
rm -f out_bz2zstd.zst out_bzip2.zst out_lbzip2.zst

echo "Benchmark complete. Results saved to $RESULT_FILE"
cat $RESULT_FILE
