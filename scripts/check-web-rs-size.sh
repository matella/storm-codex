#!/bin/sh
# Size budget of a Rust front-end bundle (a contract, see docs/spec/08-frontend.md): the gzip size of every file
# of the Trunk output folder must stay under the limit. Usage: sh scripts/check-web-rs-size.sh <dir> [limit-bytes]
# Default limit: 300000 (OBS sources: <= 300 kB). Prints the size either way.
# Set SIZE_CODEC=br to measure brotli (-q 11) instead of gzip: the server negotiates brotli, so it is what is transferred.
set -eu
dir="${1:?usage: check-web-rs-size.sh <dir> [limit-bytes]}"
limit="${2:-300000}"
codec="${SIZE_CODEC:-gzip}"
total=0
for f in "$dir"/*; do
  [ -f "$f" ] || continue
  if [ "$codec" = br ]; then n=$(brotli -q 11 -c "$f" | wc -c); else n=$(gzip -9 -c "$f" | wc -c); fi
  total=$((total + n))
done
echo "$dir: $total bytes $codec (limit $limit)"
[ "$total" -le "$limit" ] || { echo "over the size budget" >&2; exit 1; }
