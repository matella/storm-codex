# Jalon 0 — bench go/no-go : décodage `.StormReplay` Rust vs .NET vs Python

**Date :** 2026-06-12 · **Verdict : GO** (critère spec : décodage complet < 500 ms/replay et
champs nécessaires présents — les deux sont très largement tenus).
**Plan exécuté :** `docs/plans/2026-06-12-jalon0-spike-decode.md` · code : `spike/`.

## Machine & toolchains

PC de jeu Windows 11 — AMD Ryzen 7 7800X3D. Rust 1.94.0 · .NET SDK 10.0.301 ·
Python 3.13.5. Mesures mono-thread, in-process, warm-up exclu, builds optimisés
(`--release` / `-c Release`).

## Corpus

50 replays réels tirés de l'archive locale (2 652 fichiers), stratifiés 17/17/16 sur
2024/2025/2026 (l'archive ne contient rien d'antérieur à 2024 — la spec disait « 2023→2026 »,
ajusté à la réalité). 13 base builds distincts : 92665 → 97039.
Script : `spike/sample_corpus.ps1` → `corpus/spike50/` (non commité) + `manifest.csv`.

## Périmètre mesuré (identique pour les 3 moteurs)

Lecture fichier + ouverture MPQ + décodage **header + replay.details + replay.tracker.events**
(pas de game/message events — non requis pour storm-stats côté chemin chaud).

| Moteur | n | échecs | médiane | p95 | max |
|---|---|---|---|---|---|
| **Rust `storm-decode` (spike)** | 50 | **0** | **12 ms** | **21 ms** | **24 ms** |
| .NET Heroes.StormReplayParser 2.2.1 | 50 | 1 | 37 ms | 60 ms | 72 ms |
| Python heroprotocol 2.55 (+mpyq) | 50 | 0 | 161 ms | 244 ms | 318 ms |

CSV bruts : `spike/bench-results/{rust,dotnet,python}.csv` (non commités, reproductibles).

- Le seuil go/no-go (500 ms) est battu d'un facteur ~40 en médiane ; le **budget jalon 1**
  (« < 150 ms hors stats ») est déjà tenu d'un facteur ~7, et même le budget *parse complet
  decode + stats* de la spec (150 ms) a ~138 ms de marge pour storm-stats.
- Extrapolation backfill (3 000 replays, 8 cœurs, ~21 ms p95) : **< 10 s de décodage** —
  le budget « backfill 3 ans < 5 min » sera dominé par l'upload/Postgres, pas le parse.
- .NET échoue sur 1 replay (`2025-11-03 19.58.34 Lost Cavern.StormReplay`, status=Exception)
  que Rust et Python décodent sans erreur — le repli .NET aurait donc aussi un coût de
  complétude, pas seulement de perf.

## Exactitude des champs (critère « champs nécessaires présents »)

`spike/crosscheck.py` : sortie Rust comparée champ par champ à heroprotocol sur les 3 replays
les plus récents (2026-06-09, build 97039) — **3/3 identiques** : base_build,
elapsed_game_loops, carte, 10 joueurs (nom/héros/résultat), nombre exact de tracker events.
Smoke tests `cargo test` : header (signature + build), details (10 joueurs, 5 vainqueurs),
tracker (> 1 000 événements, SStatGameEvent + SScoreResultEvent présents).

## Architecture du spike (ce qui a marché)

- **MPQ :** crate `nom-mpq` 2.0.8 (celui de s2protocol-rs) — RAS, gère secteurs/compression.
- **Décodeur versioned :** port direct de `VersionedDecoder`/`decoders.py` (heroprotocol, MIT).
  Le format est 100 % aligné octet → un curseur d'octets suffit (pas de lecteur de bits).
- **Tables de protocole :** exportées en JSON depuis un clone GitHub de Blizzard/heroprotocol
  (`spike/export_protocols.py`, 390 builds) et **interprétées à l'exécution** — déjà assez
  rapide ; la génération de code Rust (protocol-gen, jalon 1) est une optimisation, pas un
  prérequis.
- **Fallback « dernier protocole connu » validé en vrai :** le build corpus 97039 n'a pas de
  protocole publié (latest = 96477) et décode parfaitement avec ; idem pour les 50 fichiers
  côté Python (le pip s'arrête à 91756 → tout le corpus est passé par le fallback, 0 échec).

## Trouvailles à retenir pour la suite

1. **PyPI heroprotocol est mort-vivant** : version pip 2.55.4.91756 (cassée sur Python ≥ 3.12,
   `import imp`) vs GitHub 2.55.15.96477. → protocol-gen du jalon 1 doit générer depuis le
   **clone GitHub**, jamais depuis pip. (Contournement importlib dans `spike/bench_python.py`.)
2. Les replays 2024+ de l'archive locale couvrent 13 builds ; le corpus de référence
   multi-builds du jalon 1 devra élargir (NAS/box + replays libres pour le mini-corpus CI).
3. Le .NET par défaut parse game events (~5× plus lent : 202 ms vs 37 ms médiane) — toute
   comparaison future doit fixer le périmètre via `ParseOptions`.
4. L'échec .NET sur Lost Cavern (carte ARAM) est non investigué — sans objet pour la suite
   (moteur écarté), mais à garder si on re-bench.

## Décision

**GO jalon 1 (storm-replay en Rust).** Le repli .NET est définitivement écarté : Rust est
~3× plus rapide que .NET sur ce périmètre, décode 50/50 fichiers, et la sortie est
identique à la référence Blizzard. Prochaine étape : plan du jalon 1 (crate `storm-replay`
publiable : 7 streams, protocol-gen depuis le clone GitHub, corpus multi-builds, < 150 ms).
