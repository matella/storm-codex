# Runbook — bascule storm-codex & décommission du serveur Node local (jalon 5)

> Étape **box/opérateur** (le soir, box allumé). Le code est prêt ; cette bascule est
> opérationnelle. À exécuter quand le serveur storm-codex tourne sur le box et que le front est
> validé. Aucune perte de fonction attendue (EBS Twitch Azure conservée, alimentée par push).

## Pré-requis
- `storm-codex-server` déployé sur le box (rsync + `docker compose build`), Postgres + Redis du
  box configurés (`DATABASE_URL`, `REDIS_URL`), `WEB_DIR` = front buildé, archive backfillée.
- `client-rs` re-pointé (`SERVER_URL` → storm-codex) déployé sur le PC de jeu.
- Variables push : `JARVIS_CHANNEL` (canal Redis Jarvis), `AZURE_PUSH_URL`/`AZURE_PUSH_TOKEN`
  (mêmes valeurs que le push patch-digest existant).

## Bascule
1. **Vérifier l'équivalence de fonction** avant d'arrêter le Node :
   - une partie test → page à jour < 5 s (WS), widget OBS à jour, event `hots.match.completed`
     reçu sur le Redis Jarvis (`redis-cli subscribe <JARVIS_CHANNEL>`), extension Twitch à jour
     (push Azure ok).
   - overlay OBS re-pointé sur `https://<box>/widget` (browser source).
2. **Inspecter le Mongo de l'overlay une dernière fois** (la spec note : vide à ce jour,
   0 replays) :
   ```
   docker exec <mongo> mongosh --eval 'db.matches.countDocuments()'
   ```
   S'il contient des définitions non dérivables (équipes/collections de test), les recréer à la
   main dans l'UI storm-codex (jalon 4) avant suppression.
3. **Arrêter le serveur Node local** (`Hots-Overlay/server.js`) :
   ```
   docker compose -f <overlay>/docker-compose.yml stop   # ou pm2 stop / systemctl stop
   ```
4. **Supprimer le Mongo** une fois l'inspection faite et l'overlay arrêté.

## Ce qui RESTE (par conception, V1)
- **EBS Twitch Azure** : conservée, alimentée par le push sortant `box→Azure` de storm-codex
  (`azure.rs`). La validation JWT Twitch reste dans l'EBS Azure (absorption = V2).
- **HotsPatchNotes** (:5001) : référentiel héros/talents, consommé par storm-codex au démarrage
  (jalon 4 — réplication `dim_*`). Inchangé.

## Rollback
Le Node local peut être relancé tant que le Mongo n'est pas supprimé. Garder une fenêtre
d'observation (quelques soirs) avant la suppression définitive du Mongo.
