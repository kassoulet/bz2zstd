#!/bin/bash
# CPU Profiling Script for parallel_bzip2
# This script runs benchmarks with CPU profiling enabled and generates flamegraphs

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
PROFILING_DIR="$PROJECT_ROOT/profiling"

echo "=== CPU Profiling for parallel_bzip2 ==="
echo

# Create profiling directory
mkdir -p "$PROFILING_DIR"

cd "$PROJECT_ROOT/parallel_bzip2"

echo "Running benchmarks with CPU profiling enabled..."
echo "This will generate flamegraphs in: $PROFILING_DIR"
echo

# Check if cargo-flamegraph is installed (alternative method)
if command -v cargo-flamegraph &> /dev/null; then
    echo "Note: cargo-flamegraph is available as an alternative profiling method"
    echo "Usage: cargo flamegraph --bench <benchmark_name>"
    echo
fi

# Run benchmarks with profiling
# The pprof profiler is already configured in the benchmark code
echo "Running decode benchmarks..."
cargo bench --bench decode_benchmark

echo
echo "Running scanner benchmarks..."
cargo bench --bench scanner_benchmark

echo
echo "Running e2e benchmarks..."
cargo bench --bench e2e_benchmark

echo
echo "=== Profiling Complete ==="
echo
echo "Flamegraphs have been generated in:"
echo "  $PROJECT_ROOT/parallel_bzip2/target/criterion/"
echo
echo "To view flamegraphs:"
echo "  1. Navigate to target/criterion/<benchmark_name>/<test_name>/profile/"
echo "  2. Open flamegraph.svg in a web browser"
echo
echo "Benchmark results are available at:"
echo "  $PROJECT_ROOT/parallel_bzip2/target/criterion/report/index.html"
echo
