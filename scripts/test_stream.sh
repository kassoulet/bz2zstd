#!/usr/bin/env bash
# End-to-end streaming test:
#   generate data -> (stream) bzip2 -> bz2zstd -> (stream) zstd -d, then check
#   1. data validation: sha256 of original == sha256 of round-tripped output
#   2. memory usage:    bz2zstd peak RSS stays under MAX_RSS_MB
#
# The input is mmap'd and its pages count towards RSS, so the limit is
#   bz2 file size + OVERHEAD_MB   (OVERHEAD_MB covers buffers, thread stacks, zstd contexts).
#
# Usage: scripts/test_stream.sh            (SIZE_MB=300 OVERHEAD_MB=300 by default)
set -euo pipefail

SIZE_MB=${SIZE_MB:-300}
OVERHEAD_MB=${OVERHEAD_MB:-300}
BZ2ZSTD=${BZ2ZSTD:-}
cd "$(dirname "$0")/.."

for t in bzip2 zstd sha256sum base64; do command -v "$t" >/dev/null || { echo "missing: $t"; exit 2; }; done
TIME=/usr/bin/time
[ -x "$TIME" ] || { echo "missing: /usr/bin/time (GNU time)"; exit 2; }

if [ -z "$BZ2ZSTD" ]; then
    cargo build --release -q -p bz2zstd
    BZ2ZSTD=$(cargo metadata --format-version 1 --no-deps | python3 -c 'import json,sys; print(json.load(sys.stdin)["target_directory"])')/release/bz2zstd
fi

WORK=$(mktemp -d)
trap 'rm -rf "$WORK"' EXIT

# 1. Stream generated data through tee (hash) into bzip2; nothing is stored uncompressed.
echo "[1/4] generating ${SIZE_MB} MB and streaming to bzip2"
# base64 expands 3:4, so read 3/4 of the target size from urandom
head -c $((SIZE_MB * 1024 * 1024 * 3 / 4)) /dev/urandom | base64 -w 76 \
    | tee >(sha256sum | cut -d' ' -f1 > "$WORK/orig.sha") \
    | bzip2 -9 > "$WORK/data.bz2"
wait
echo "      bz2 size: $(du -h "$WORK/data.bz2" | cut -f1)"

# 2. Convert, recording peak RSS (KB) of bz2zstd.
echo "[2/4] bz2zstd"
"$TIME" -f '%M' -o "$WORK/rss_kb" "$BZ2ZSTD" "$WORK/data.bz2" -o "$WORK/data.zst"
RSS_MB=$(( $(cat "$WORK/rss_kb") / 1024 ))
BZ2_MB=$(( $(stat -c %s "$WORK/data.bz2") / 1024 / 1024 ))
MAX_RSS_MB=$(( BZ2_MB + OVERHEAD_MB ))

# 3. Stream-decompress zstd straight into the hash.
echo "[3/4] streaming zstd -d | sha256sum"
NEW=$(zstd -dc "$WORK/data.zst" | sha256sum | cut -d' ' -f1)
ORIG=$(cat "$WORK/orig.sha")

# 4. Checks.
echo "[4/4] checks"
fail=0
if [ "$ORIG" = "$NEW" ]; then echo "  OK   data: sha256 match"; else echo "  FAIL data: $ORIG != $NEW"; fail=1; fi
if [ "$RSS_MB" -le "$MAX_RSS_MB" ]; then echo "  OK   memory: peak RSS ${RSS_MB} MB <= ${MAX_RSS_MB} MB (bz2 ${BZ2_MB} + ${OVERHEAD_MB})"; else echo "  FAIL memory: peak RSS ${RSS_MB} MB > ${MAX_RSS_MB} MB (bz2 ${BZ2_MB} + ${OVERHEAD_MB})"; fail=1; fi
exit $fail
