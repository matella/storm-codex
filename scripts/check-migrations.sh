#!/usr/bin/env sh
# Rejects any modification, deletion or rename of an sqlx migration already present in the
# comparison base. sqlx checks each applied migration by checksum: rewriting
# one makes startup fail (VersionMismatch) on any database that already applied it. A
# schema change goes through a NEW migration.
# Usage: scripts/check-migrations.sh <base-commit>   (e.g. origin/main, or the SHA before a push)
set -eu
base="${1:?usage: check-migrations.sh <base-commit>}"
dir="crates/storm-codex-server/migrations"
case "$base" in
  ""|0000000000000000000000000000000000000000) echo "no comparison base: check skipped"; exit 0 ;;
esac
if ! git cat-file -e "$base^{commit}" 2>/dev/null; then
  echo "base $base not found (truncated history?): check skipped"; exit 0
fi
anc="$(git merge-base "$base" HEAD)"
changed="$(git diff --name-status --diff-filter=MDR "$anc" HEAD -- "$dir")"
if [ -n "$changed" ]; then
  echo "ERROR: existing migration(s) modified — create a new migration instead:"
  echo "$changed"
  exit 1
fi
echo "existing migrations unchanged since $anc"
