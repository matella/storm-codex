# Workflow de développement — le dev se pilote depuis la doc

Ce fichier est le **contrat de travail** de tout intervenant, humain ou IA. La règle d'or :

> **Aucune modification de comportement sans documentation dans le même commit.**
> Un diff qui change un contrat (endpoint, schéma, event, page, env, budget perf) sans toucher
> `docs/spec/` est un **changement incomplet** — il ne se merge pas, il se complète.

## Le cycle (spec-first)

1. **Se synchroniser** : `git fetch origin` (le clone local est souvent en retard — du dev se
   fait depuis plusieurs machines), lire `docs/STATUS.md` (où on en est) puis la ou les sections
   de `docs/spec/` touchées par le changement envisagé.
2. **Spécifier avant de coder** :
   - Changement dans le cadre existant (nouvelle stat, nouveau bloc UI, endpoint de lecture…) →
     mettre à jour la section `docs/spec/` concernée (ou la rédiger) **d'abord ou en même temps**
     que le code, même commit.
   - **Décision de conception** (nouvelle capacité, changement d'architecture, scope, UX
     majeure) → spec datée dans `docs/specs/AAAA-MM-JJ-….md` + **validation opérateur avant
     d'implémenter**. Les décisions verrouillées (01-architecture) ne se rouvrent pas sans lui.
3. **Coder** dans le cadre spécifié — conventions : Rust 2021+, clippy strict, erreurs typées,
   pas de `unwrap()` hors tests ; front TS strict ; commits conventionnels ; doc de lib à jour
   via MCP Context7 (ne pas coder de mémoire les API axum/sqlx/TanStack…).
4. **Prouver** (jamais déclarer) : tests + vérification adaptée au type de changement
   (tableau ci-dessous). Une feature UI se vérifie dans le navigateur sur données réelles.
5. **Documenter le reste** : `docs/STATUS.md` (fin de session) ; mémoire IA le cas échéant.
6. **Commit + push** (l'opérateur a autorisé le push direct sur `main` à chaque étape finie).

## Checklist par type de changement (quoi mettre à jour, quoi prouver)

| Changement | Doc à mettre à jour (même commit) | Preuve minimale |
|---|---|---|
| Endpoint REST/WS (ajout/modif/suppression) | `05-api.md` (+ `07-evenements.md` si event) | test ou curl vérifié |
| Migration SQL | `06-modele-donnees.md` | migration appliquée sur le Postgres dev |
| Forme de la projection (`project.rs`, storm-stats output) | `06-modele-donnees.md` + bump `PARSER_VERSION` + `04-serveur.md` si pipeline | reprocess vérifié idempotent |
| Logique de stats (`storm-stats/process.rs`) | `03-storm-stats.md` (toute divergence → tolerances.json) | **diff de parité re-vert** (`tools/parity-harness/`) |
| Décodage (`storm-replay`) | `02-storm-replay.md` | crosscheck heroprotocol + mini-corpus |
| Page/route/overlay front | `08-frontend.md` | `npm run build` vert + vérif navigateur sur données réelles |
| Variable d'env, compose, Dockerfile, CI | `04-serveur.md` / `09-operations.md` | build/déploiement testé |
| Event WS ou Jarvis | `07-evenements.md` | event observé (WS ou Redis) |
| Perf touchée (parse, API, backfill) | budget re-mesuré dans `01-architecture.md` | bench correspondant |
| Nouveau format de draft, réglage, référentiel | `04-serveur.md` + section idoine | test moteur (draft = TDD, zéro I/O) |

Cas sans doc à toucher : refactor pur sans changement de contrat, typo, bump de dépendance sans
changement d'API — le commit le dit explicitement (« aucun contrat modifié »).

## Définition de « fini »

- Critère d'acceptation mesurable vérifié (pas « ça devrait marcher »).
- Tests verts (`cargo test`, build front) ; parité/bench si concernés.
- `docs/spec/` à jour ; `docs/STATUS.md` à jour en fin de session.
- Poussé sur `main` ; si l'utilisateur le demande, déployé sur le box **et vérifié par HTTP**
  (hash du bundle changé, `/api/health` 200).

## Ce qu'un intervenant (IA ou humain) ne fait PAS sans l'opérateur

- Rouvrir une décision verrouillée (01-architecture) ou re-trancher une spec datée validée.
- Casser un budget perf ou introduire une divergence de parité non documentée.
- `cargo publish`, création de repos publics, bascule/décommission de services du box.
- Toucher aux services voisins du box (Jarvis, HotsPatchNotes, overlay Node) hors lecture.
- Modifier une migration déjà appliquée (toujours une nouvelle migration).
