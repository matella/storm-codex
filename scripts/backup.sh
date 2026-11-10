#!/usr/bin/env bash
# Simple backup of the storm-codex database: pg_dump → timestamped .sql.gz file.
# Usage   : ./scripts/backup.sh [dest_dir]      (default: ./backups)
# Restore : gunzip -c backups/<file>.sql.gz | docker exec -i storm-codex-pg psql -U storm -d storm_codex
# Purely local use → no encryption/offsite; a light cron can call it every night.
set -euo pipefail

DEST="${1:-./backups}"
CONTAINER="${PG_CONTAINER:-storm-codex-pg}"
DB="${PG_DB:-storm_codex}"
USER="${PG_USER:-storm}"

mkdir -p "$DEST"
TS=$(date +%Y%m%d-%H%M%S)
OUT="$DEST/${DB}-${TS}.sql.gz"

docker exec "$CONTAINER" pg_dump -U "$USER" "$DB" | gzip > "$OUT"
echo "backup → $OUT ($(du -h "$OUT" | cut -f1))"

# retention: keep only the last 14
ls -1t "$DEST"/${DB}-*.sql.gz 2>/dev/null | tail -n +15 | xargs -r rm -f
