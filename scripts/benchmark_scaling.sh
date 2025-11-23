#!/bin/bash
set -e

# Configuration
DATA_SIZE_MB=100
INPUT_FILE="bench_scaling.bin"
COMPRESSED_FILE="bench_scaling.bin.bz2"
RESULT_FILE="benchmark_scaling_results.txt"

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
check_dependency "nproc"
check_dependency "/usr/bin/time"

echo "Preparing scaling benchmark..." > $RESULT_FILE

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
echo "Scaling Benchmark Results (Data Size: ${DATA_SIZE_MB}MB, Single Stream)" >> $RESULT_FILE
echo "System Cores: $(nproc)" >> $RESULT_FILE
echo "---------------------------------------------------" >> $RESULT_FILE

run_bench() {
    CORES=$1
    CMD=$2

    echo "Running with $CORES cores..."
    echo "[Cores: $CORES]" >> $RESULT_FILE

    # Force thread count
    export RAYON_NUM_THREADS=$CORES

    /usr/bin/time -f "Real: %e s, User: %U s, Sys: %S s, CPU: %P" $CMD 2>> $RESULT_FILE

    echo "" >> $RESULT_FILE
}

# Run with 1, 2, 4, 8 cores (up to nproc)
MAX_CORES=$(nproc)
for CORES in 1 2 4 8 16; do
    if [ $CORES -le $MAX_CORES ]; then
        run_bench $CORES "./target/release/bz2zstd $COMPRESSED_FILE --output out_scaling_${CORES}.zst"
    fi
done

# Cleanup
rm -f out_scaling_*.zst
# Keep input/compressed for re-runs if needed, or delete?
# rm -f $INPUT_FILE $COMPRESSED_FILE

echo "Benchmark complete. Results saved to $RESULT_FILE"
cat $RESULT_FILE
