#!/usr/bin/env sh
# Refuse toute modification, suppression ou renommage d'une migration sqlx déjà présente dans la
# base de comparaison. sqlx vérifie chaque migration appliquée par somme de contrôle : en réécrire
# une fait échouer le démarrage (VersionMismatch) de toute base qui l'a déjà appliquée. Un
# changement de schéma passe par une NOUVELLE migration.
# Usage : scripts/check-migrations.sh <commit-de-base>   (ex. origin/main, ou le SHA avant un push)
set -eu
base="${1:?usage : check-migrations.sh <commit-de-base>}"
dir="crates/storm-codex-server/migrations"
case "$base" in
  ""|0000000000000000000000000000000000000000) echo "pas de base de comparaison : contrôle sauté"; exit 0 ;;
esac
if ! git cat-file -e "$base^{commit}" 2>/dev/null; then
  echo "base $base introuvable (historique tronqué ?) : contrôle sauté"; exit 0
fi
anc="$(git merge-base "$base" HEAD)"
changed="$(git diff --name-status --diff-filter=MDR "$anc" HEAD -- "$dir")"
if [ -n "$changed" ]; then
  echo "ERREUR : migration(s) existante(s) modifiée(s) — créer une nouvelle migration à la place :"
  echo "$changed"
  exit 1
fi
echo "migrations existantes inchangées depuis $anc"
