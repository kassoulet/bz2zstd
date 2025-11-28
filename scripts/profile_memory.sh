#!/bin/bash
# Memory Profiling Script for parallel_bzip2
# This script runs benchmarks with memory profiling using valgrind

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
PROFILING_DIR="$PROJECT_ROOT/profiling"

echo "=== Memory Profiling for parallel_bzip2 ==="
echo

# Check if valgrind is installed
if ! command -v valgrind &> /dev/null; then
    echo "Error: valgrind is not installed"
    echo "Install with: sudo apt-get install valgrind (Debian/Ubuntu)"
    echo "           or: sudo yum install valgrind (RHEL/CentOS)"
    exit 1
fi

# Create profiling directory
mkdir -p "$PROFILING_DIR"

cd "$PROJECT_ROOT/parallel_bzip2"

echo "Building benchmarks in release mode..."
cargo build --release --benches

echo
echo "Running memory profiling..."
echo "This may take several minutes..."
echo

# Run decode benchmark with valgrind
echo "Profiling decode benchmark..."
valgrind --tool=massif \
    --massif-out-file="$PROFILING_DIR/massif.decode.out" \
    --time-unit=B \
    ../target/release/deps/decode_benchmark-* --bench --test 2>&1 | head -n 50

# Run scanner benchmark with valgrind
echo
echo "Profiling scanner benchmark..."
valgrind --tool=massif \
    --massif-out-file="$PROFILING_DIR/massif.scanner.out" \
    --time-unit=B \
    ../target/release/deps/scanner_benchmark-* --bench --test 2>&1 | head -n 50

# Run e2e benchmark with valgrind
echo
echo "Profiling e2e benchmark..."
valgrind --tool=massif \
    --massif-out-file="$PROFILING_DIR/massif.e2e.out" \
    --time-unit=B \
    ../target/release/deps/e2e_benchmark-* --bench --test 2>&1 | head -n 50

echo
echo "=== Memory Profiling Complete ==="
echo
echo "Memory profiles have been saved to: $PROFILING_DIR"
echo
echo "To view memory profiles:"
echo "  ms_print $PROFILING_DIR/massif.decode.out"
echo "  ms_print $PROFILING_DIR/massif.scanner.out"
echo "  ms_print $PROFILING_DIR/massif.e2e.out"
echo
echo "Or use a graphical viewer:"
echo "  massif-visualizer $PROFILING_DIR/massif.decode.out"
echo

# Check for memory leaks with memcheck
echo "Running memory leak detection..."
echo "This will only run a quick test to check for leaks..."
echo

valgrind --tool=memcheck \
    --leak-check=full \
    --show-leak-kinds=all \
    --track-origins=yes \
    --log-file="$PROFILING_DIR/memcheck.log" \
    ../target/release/deps/decode_benchmark-* --bench --test 2>&1 | head -n 20

echo
echo "Memory leak report saved to: $PROFILING_DIR/memcheck.log"
echo
