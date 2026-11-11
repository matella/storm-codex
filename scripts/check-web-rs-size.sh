#!/bin/sh
# Size budget of a Rust front-end bundle (a contract, see docs/spec/08-frontend.md): the gzip size of every file
# of the Trunk output folder must stay under the limit. Usage: sh scripts/check-web-rs-size.sh <dir> [limit-bytes]
# Default limit: 300000 (OBS sources: <= 300 kB). Prints the size either way.
set -eu
dir="${1:?usage: check-web-rs-size.sh <dir> [limit-bytes]}"
limit="${2:-300000}"
total=0
for f in "$dir"/*; do
  [ -f "$f" ] || continue
  n=$(gzip -9 -c "$f" | wc -c)
  total=$((total + n))
done
echo "$dir: $total bytes gzip (limit $limit)"
[ "$total" -le "$limit" ] || { echo "over the size budget" >&2; exit 1; }
