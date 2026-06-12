# STATUS — lire d'abord, mettre à jour en dernier

## Où on en est (2026-06-12)
- **Recherche terminée** : anatomie SotS + hots-parser, verdicts dépendances, comparatif moteurs,
  écosystème (HeroesProfile vivant, HeroesMatchTracker archivé). → `docs/research/`.
- **Spec programme validée** (opérateur + revue agent, 2 passes) : Rust core, Postgres,
  3 étages de données, parité SotS totale en V1 (ligues incluses), remplacement du serveur Node
  local, widget stream, scope C (auto-hébergé V1, open-source, public-ready V2), multi-tokens.
  → `docs/specs/2026-06-12-storm-codex-design.md`.
- **Maquettes validées** (14 écrans, couverture vue-par-vue auditée contre SotS ; widget défaite
  enrichi sur retour opérateur). → `docs/specs/2026-06-12-storm-codex-mockup.html`.

## Prochaine étape — Jalon 0 : spike go/no-go (plan à écrire, puis exécuter)
1. Récupérer un échantillon de replays réels du PC de jeu (≥ 50 fichiers, builds variés
   2023→2026) — via client-rs vers le box, ou copie directe.
2. Spike Rust : ouvrir MPQ + décoder header/details/trackerevents d'au moins 3 replays
   (s'appuyer sur la machinerie s2protocol-rs + tables générées depuis Blizzard/heroprotocol).
3. Bench comparatif sur 50 replays : Rust vs Heroes.StormReplayParser (.NET) vs heroprotocol (Py).
4. **Accept : décodage complet < 500 ms/replay et champs nécessaires présents.**
   Go → jalon 1 (storm-replay). No-go → repli .NET acté dans la spec, même architecture.

## Jalons (résumé — détail et critères dans la spec)
0 spike → 1 storm-replay → 2 storm-stats (diff vs hots-parser) → 3 serveur+DB+backfill →
4 front parité → 5 stream+Jarvis+bascule (décommission Node local) → 6 publication crates.

## Décisions verrouillées (ne pas rouvrir sans l'opérateur)
Rust (repli .NET si spike no-go) · Postgres · design Nexus Codex · remplacement serveur Node
local (EBS Twitch Azure conservé en V1, alimenté par push) · V1 = parité totale · pas de
pré-game · aucune migration de données (backfill + recréation manuelle) · nom Storm Codex.

## Bloquants / besoins opérateur
- Échantillon de replays pour le jalon 0 (action PC de jeu).
- Création des repos publics GitHub (storm-replay/storm-stats) au moment du jalon 6 — d'ici là,
  tout vit dans ce repo.
