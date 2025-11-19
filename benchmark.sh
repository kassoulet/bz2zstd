#!/bin/bash
set -e

# Configuration
DATA_SIZE_MB=200
INPUT_FILE="bench_data.bin"
COMPRESSED_FILE="bench_data.bin.bz2"
RESULT_FILE="benchmark_results.txt"

echo "Preparing benchmark..." > $RESULT_FILE

# Compile
echo "Compiling parallel-bz2..."
cargo build --release >> $RESULT_FILE 2>&1

# Generate Data
echo "Generating ${DATA_SIZE_MB}MB of random data..."
dd if=/dev/urandom of=$INPUT_FILE bs=1M count=$DATA_SIZE_MB status=none

# Compress (using pbzip2 to ensure multi-stream)
echo "Compressing with pbzip2..."
pbzip2 -f -k -p$(nproc) $INPUT_FILE

echo "---------------------------------------------------" >> $RESULT_FILE
echo "Benchmark Results (Data Size: ${DATA_SIZE_MB}MB)" >> $RESULT_FILE
echo "System Cores: $(nproc)" >> $RESULT_FILE
echo "---------------------------------------------------" >> $RESULT_FILE

run_bench() {
    TOOL_NAME=$1
    CMD=$2
    
    echo "Running $TOOL_NAME..."
    echo "[$TOOL_NAME]" >> $RESULT_FILE
    
    # Clear cache to be fair (requires sudo, skipping for now as I might not have it, 
    # but we can rely on the file being in cache for all of them if we run them sequentially quickly, 
    # or just accept FS cache impact. For CPU bound tasks, it's less critical).
    
    # We use /usr/bin/time to get detailed stats
    # %P: Percentage of the CPU that this job got
    # %e: Elapsed (wall clock) time
    # %U: User time
    # %S: System time
    /usr/bin/time -f "Real: %e s, User: %U s, Sys: %S s, CPU: %P" $CMD 2>> $RESULT_FILE
    
    echo "" >> $RESULT_FILE
}

# 1. parallel-bz2
run_bench "bz2zstd" "./target/release/bz2zstd --input $COMPRESSED_FILE --output out_parallel.bin"

# 2. pbzip2
run_bench "pbzip2" "pbzip2 -d -k -f -p$(nproc) $COMPRESSED_FILE"

# 3. lbzip2
run_bench "lbzip2" "lbzip2 -d -k -f -n $(nproc) $COMPRESSED_FILE"

# Cleanup
rm -f $INPUT_FILE $COMPRESSED_FILE out_parallel.bin
echo "Benchmark complete. Results saved to $RESULT_FILE"
cat $RESULT_FILE
