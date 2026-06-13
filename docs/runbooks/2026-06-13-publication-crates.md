# Runbook — publication des crates (jalon 6)

Action **opérateur** (compte crates.io). Publie `storm-replay` puis `storm-stats` sur crates.io.
Dry-run validé (`storm-replay` : 54 fichiers, 2.4 MiB, build de vérification OK).

## Pré-requis
- Compte crates.io (se connecter via GitHub sur https://crates.io).
- Jeton API : crates.io → **Account Settings → API Tokens → New Token** (portée publish).
- `cargo login <jeton>` une fois sur la machine qui publie (ici le Mac : toolchain présente).
- Vérifier que les noms `storm-replay` / `storm-stats` sont libres sur crates.io (sinon renommer).

## Ordre de publication (IMPORTANT)
`storm-stats` dépend de `storm-replay` → publier `storm-replay` **d'abord** (crates.io doit
l'indexer avant que `storm-stats` puisse référencer `version = "0.1"`).

```bash
cd ~/Documents/GitHub/storm-codex          # repo racine (workspace)
cargo publish -p storm-replay              # attendre ~30-60s que l'index se mette à jour
cargo publish -p storm-stats               # référence storm-replay 0.1 (déjà publié)
```
Si `storm-stats` échoue avec « no matching package named storm-replay », attendre 1 min
(propagation de l'index) et relancer.

## Notes
- **Permanent** : une version publiée ne se supprime pas (seulement `cargo yank` pour la masquer).
  Publie quand tu es prêt à figer `0.1.0`.
- `repository` pointe sur `github.com/matella/storm-codex` (monorepo, privé aujourd'hui). Pour des
  liens publics fonctionnels : soit rendre `storm-codex` public, soit scinder les crates dans leurs
  propres repos publics. La publication crates.io marche dans tous les cas (le crate se compile
  seul — dry-run OK) ; seul le lien « Repository » sur crates.io serait mort tant que le repo est privé.
- `storm-codex-server` n'est **pas** publié (binaire applicatif, pas une lib) — `publish = false`
  implicite via l'absence d'intérêt ; seuls les deux crates lib sont publiés.

## Après publication
- Mettre à jour les README des crates avec le badge crates.io + un exemple d'usage.
- (optionnel) Annoncer (le créneau « parseur HotS en Rust » est vacant).
