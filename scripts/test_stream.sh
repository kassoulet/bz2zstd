#!/usr/bin/env bash
# End-to-end streaming test:
#   generate data -> (stream) bzip2 -> bz2zstd -> (stream) zstd -d, then check
#   1. data validation: sha256 of original == sha256 of round-tripped output
#   2. memory usage:    bz2zstd peak RSS stays under MAX_RSS_MB
#
# Memory is checked on /proc/<pid>/status, sampled while bz2zstd runs:
#   RssAnon  real working memory; limit = ANON_BASE_MB + threads * ANON_PER_THREAD_MB
#            (dominated by one zstd compression context + block buffers per thread)
#   RssFile  mmap'd input pages (reclaimable); bz2zstd releases finished blocks, so this
#            must stay <= MAX_FILE_MB no matter how large the input is.
#
# Usage: scripts/test_stream.sh   (SIZE_MB=300; override limits via the vars below)
set -euo pipefail

SIZE_MB=${SIZE_MB:-300}
ANON_BASE_MB=${ANON_BASE_MB:-50}
ANON_PER_THREAD_MB=${ANON_PER_THREAD_MB:-20}
MAX_FILE_MB=${MAX_FILE_MB:-150}
THREADS=${THREADS:-$(nproc)}
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

# 2. Convert, sampling peak anonymous / file-backed RSS (MB) of bz2zstd.
echo "[2/4] bz2zstd (-j $THREADS)"
"$BZ2ZSTD" -j "$THREADS" "$WORK/data.bz2" -o "$WORK/data.zst" & PID=$!
ANON_KB=0; FILE_KB=0
while kill -0 "$PID" 2>/dev/null; do
    read -r a f < <(awk '/RssAnon/{a=$2} /RssFile/{f=$2} END{print a+0, f+0}' "/proc/$PID/status" 2>/dev/null || echo "0 0")
    [ "${a:-0}" -gt "$ANON_KB" ] && ANON_KB=$a
    [ "${f:-0}" -gt "$FILE_KB" ] && FILE_KB=$f
    sleep 0.05
done
wait "$PID" || { echo "  FAIL bz2zstd exited with an error"; exit 1; }
ANON_MB=$((ANON_KB / 1024)); FILE_MB=$((FILE_KB / 1024))
MAX_ANON_MB=$((ANON_BASE_MB + THREADS * ANON_PER_THREAD_MB))

# 3. Stream-decompress zstd straight into the hash.
echo "[3/4] streaming zstd -d | sha256sum"
NEW=$(zstd -dc "$WORK/data.zst" | sha256sum | cut -d' ' -f1)
ORIG=$(cat "$WORK/orig.sha")

# 4. Checks.
echo "[4/4] checks"
fail=0
if [ "$ORIG" = "$NEW" ]; then echo "  OK   data: sha256 match"; else echo "  FAIL data: $ORIG != $NEW"; fail=1; fi
if [ "$ANON_MB" -le "$MAX_ANON_MB" ]; then echo "  OK   anon RSS ${ANON_MB} MB <= ${MAX_ANON_MB} MB"; else echo "  FAIL anon RSS ${ANON_MB} MB > ${MAX_ANON_MB} MB"; fail=1; fi
if [ "$FILE_MB" -le "$MAX_FILE_MB" ]; then echo "  OK   file RSS ${FILE_MB} MB <= ${MAX_FILE_MB} MB"; else echo "  FAIL file RSS ${FILE_MB} MB > ${MAX_FILE_MB} MB"; fail=1; fi
exit $fail
