#!/bin/sh
# Builds the reference-data snapshot `referential.tar.gz` bundled by the standalone desktop installer, from the
# operator's HotsPatchNotes API. Layout = what `dim::ingest_snapshot` (crates/storm-codex-server/src/dim.rs) reads:
#   heroes.json          GET /api/heroes                         (array)
#   hero-details.json    {shortName: GET /api/heroes/<shortName>} (object)
#   patches.json         GET /api/patches?page=1&pageSize=200     (object with `items`, same page as sync_patches)
#   patch-details.json   {internalId: GET /api/patches/<internalId>} (object)
#   images/**            the `/images/...` files named by heroes[].icon and battlegrounds[].imageUrl
#
# Usage: sh scripts/make-referential.sh <hpn-base-url> [out-file]
#    or: HPN_BASE_URL=<hpn-base-url> sh scripts/make-referential.sh [out-file]
# Default out-file: target/referential/referential.tar.gz. Prints the SHA-256 to pin in desktop/referential-pin.txt.
# Needs: curl, jq, tar (gzip), and sha256sum or shasum. No host or secret is built in: the base URL is an argument.
set -eu

if [ $# -ge 1 ] && [ "${1#http}" != "$1" ]; then
  base="$1"
  shift
else
  base="${HPN_BASE_URL:-}"
fi
[ -n "$base" ] || { echo "usage: make-referential.sh <hpn-base-url> [out-file] (or set HPN_BASE_URL)" >&2; exit 2; }
base="${base%/}"
out="${1:-target/referential/referential.tar.gz}"

for tool in curl jq tar; do
  command -v "$tool" >/dev/null 2>&1 || { echo "missing tool: $tool" >&2; exit 2; }
done
if command -v sha256sum >/dev/null 2>&1; then sha() { sha256sum "$1" | cut -d' ' -f1; }
elif command -v shasum >/dev/null 2>&1; then sha() { shasum -a 256 "$1" | cut -d' ' -f1; }
else echo "missing tool: sha256sum or shasum" >&2; exit 2; fi

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT INT TERM
snap="$work/snap"
mkdir -p "$snap/images" "$work/hero" "$work/patch"
: >"$work/hero-details.stream"
: >"$work/patch-details.stream"

# jq -r lines without a CR (a Windows jq build ends lines with CRLF).
jqr() { jq -r "$@" | tr -d '\r'; }

get() { curl -fsS --retry 3 --max-time 60 "$base$1" -o "$2"; }

# A key used in a URL path: plain characters only (no `/`, `..`, `?`, spaces).
safe_key() {
  case "$1" in
    "" | *[!A-Za-z0-9._-]* | *..*) return 1 ;;
    *) return 0 ;;
  esac
}

echo "heroes from $base"
get /api/heroes "$snap/heroes.json"
jq -e 'type == "array" and length > 0' "$snap/heroes.json" >/dev/null \
  || { echo "/api/heroes: expected a non-empty array" >&2; exit 1; }

n=0
for short in $(jqr '.[].shortName // empty' "$snap/heroes.json"); do
  safe_key "$short" || { echo "unexpected hero shortName: $short" >&2; exit 1; }
  get "/api/heroes/$short" "$work/hero/$short.json"
  jq --arg k "$short" '{($k): .}' "$work/hero/$short.json" >>"$work/hero-details.stream"
  n=$((n + 1))
done
jq -s 'add // {}' "$work/hero-details.stream" >"$snap/hero-details.json"
echo "  $n hero details"

echo "patches"
get "/api/patches?page=1&pageSize=200" "$snap/patches.json"
jq -e '.items | type == "array" and length > 0' "$snap/patches.json" >/dev/null \
  || { echo "/api/patches: expected an object with a non-empty items array" >&2; exit 1; }
n=0
for iid in $(jqr '.items[].internalId // empty' "$snap/patches.json"); do
  safe_key "$iid" || { echo "unexpected patch internalId: $iid" >&2; exit 1; }
  get "/api/patches/$iid" "$work/patch/$iid.json"
  jq --arg k "$iid" '{($k): .}' "$work/patch/$iid.json" >>"$work/patch-details.stream"
  n=$((n + 1))
done
jq -s 'add // {}' "$work/patch-details.stream" >"$snap/patch-details.json"
echo "  $n patch details"

echo "images"
get /api/battlegrounds "$work/battlegrounds.json"
n=0
missing=0
for path in $(jqr '.[].icon // empty' "$snap/heroes.json") $(jqr '.[].imageUrl // empty' "$work/battlegrounds.json"); do
  rel="${path#/images/}"
  [ "$rel" != "$path" ] || continue # not under /images/: skipped, like the server
  case "$rel" in
    *..* | *[!A-Za-z0-9._/-]*) echo "unexpected image path: $path" >&2; exit 1 ;;
  esac
  mkdir -p "$snap/images/$(dirname "$rel")"
  # 404 (e.g. an ARAM map without an image) is skipped, as the server's own vendoring does.
  if get "$path" "$snap/images/$rel" && [ -s "$snap/images/$rel" ]; then
    n=$((n + 1))
  else
    rm -f "$snap/images/$rel"
    missing=$((missing + 1))
  fi
done
echo "  $n images ($missing missing)"

mkdir -p "$(dirname "$out")"
tar -czf "$out" -C "$snap" heroes.json hero-details.json patches.json patch-details.json images
echo "wrote $out ($(wc -c <"$out" | tr -d ' ') bytes)"
echo "sha256=$(sha "$out")"
