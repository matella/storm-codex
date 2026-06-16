#!/usr/bin/env bash
# Sauvegarde simple de la base storm-codex : pg_dump → fichier .sql.gz horodaté.
# Usage   : ./scripts/backup.sh [dossier_dest]      (défaut : ./backups)
# Restore : gunzip -c backups/<fichier>.sql.gz | docker exec -i storm-codex-pg psql -U storm -d storm_codex
# Usage purement local → pas de chiffrement/offsite ; un cron léger peut l'appeler chaque nuit.
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

# rétention : ne garde que les 14 derniers
ls -1t "$DEST"/${DB}-*.sql.gz 2>/dev/null | tail -n +15 | xargs -r rm -f
