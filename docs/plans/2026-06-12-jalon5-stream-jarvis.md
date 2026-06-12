# Jalon 5 — stream + Jarvis + bascule

> **For Claude:** REQUIRED SUB-SKILL: superpowers:executing-plans.

**Goal :** widget OBS post-game, événement `hots.match.completed` → Redis (invariants spine
Jarvis), push post-game box→Azure (extension Twitch), décommission du serveur Node local.

**Accept (spec) :** partie jouée → widget OBS + brief Jarvis + extension Twitch à jour ; serveur
Node local arrêté sans perte de fonction.

**Buildable ici (sans box)** : widget OBS, émetteur Redis (testable contre Redis local Docker),
code du push Azure (config, untested contre la vraie EBS). **Box/opérateur** : Redis Jarvis réel,
consommateur Jarvis, EBS Azure, arrêt du Node local.

---

### Task 1 : émetteur Jarvis (Redis) + invariants spine
- `redis` au compose (dev) + crate `redis`. Config `REDIS_URL` (option ; absent = pas d'émission).
- `jarvis.rs` : à chaque `match.parsed`, publie `hots.match.completed` sur Redis avec les
  invariants : `schema_version`, `correlation_id`/`causation_id`, `occurred_at`/`recorded_at`,
  type `entity.verb` au passé. Payload : résultat, héros, KDA, carte, durée, awards, écarts.
- Test : publie contre Redis local, un abonné reçoit l'event bien formé. Commit.

### Task 2 : widget OBS (page autonome)
- `/widget` : page (servie par le binaire) résumé dernière partie / session, fond transparent,
  mise à jour live via WS `match.parsed`. Lookup dernière partie via `/api/matches?limit=1`.
- Vérif rendu. Commit.

### Task 3 : push post-game box→Azure (code + config)
- `azure.rs` : POST sortant authentifié (même mécanisme que le patch-digest HotsPatchNotes) du
  résumé post-game vers l'EBS Azure (`AZURE_PUSH_URL`/token, optionnels). Untested contre la
  vraie EBS — flaggé. Commit.

### Task 4 : décommission Node + STATUS + push
- Documenter l'arrêt du serveur Node local (inspection Mongo, arrêt, suppression) — étape box,
  scriptée mais à exécuter par l'opérateur le soir. STATUS, commit, push.
