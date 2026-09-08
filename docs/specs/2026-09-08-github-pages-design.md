# Storm Codex sur GitHub Pages — design

> Spec storm-codex. Scope retenu par l'opérateur le 2026-09-08 (les quatre volets).
> **En attente de validation** avant `writing-plans` et implémentation.

## But

Publier sur `matella.github.io/storm-codex` la part de Storm Codex qui **peut vivre sans le box** :
la doc, les crates, et surtout un **playground qui décode un replay entièrement dans le navigateur**.
Le box ne tourne que le soir et n'est joignable qu'en Tailscale ; Pages est en ligne 24/7 et
public. Ce sont deux surfaces disjointes, pas un miroir.

## Ce que Pages impose (contraintes, non négociables)

- **Statique uniquement** : pas d'API, pas de WS, pas de Postgres, pas de secret. Rien de
  `storm-codex-server` ne part là-bas.
- **Public par construction** : le repo `matella/storm-codex` est public (vérifié) — donc tout
  fichier publié est lisible par n'importe qui, indexable, et un `git revert` ne dépublie pas ce
  qui a été aspiré. **Aucune donnée de match personnelle par défaut** (volet 4 = opt-in explicite,
  match par match).
- Limites de la plateforme : site publié ≤ 1 Go, ~100 Go/mois de bande passante (soft), HTTPS
  fourni. Déploiement par **GitHub Actions** (`actions/deploy-pages`), pas par branche `gh-pages`.
- **Pas de réécriture d'URL côté serveur** → les liens profonds d'une SPA en routage `history`
  cassent. Décision : le build Pages utilise **`HashRouter`**, pas `BrowserRouter` (le build box
  reste inchangé).

## Décisions actées

1. **Une seule Action `pages.yml`** qui construit et publie les quatre volets sous un même site,
   par sous-chemins : `/` (landing), `/spec/`, `/api/` (rustdoc), `/play/`, `/share/`.
2. **Aucun volet ne modifie le serveur ni la projection.** Si un volet exige un changement dans
   `crates/`, il se fait par ajout (nouveau crate `storm-web`), jamais par modification du chemin
   chaud du box.
3. **Le playground ne téléverse rien.** Le replay déposé est lu par `FileReader`, décodé en wasm,
   et n'existe que dans l'onglet. Pas d'analytics, pas de `fetch` sortant, pas de stockage.
4. **Ordre de livraison** : volet 1 (doc) → volet 2 (rustdoc) → volet 3 (playground) → volet 4
   (partage). Les deux premiers sont du YAML et débloquent la chaîne de déploiement ; le troisième
   est le vrai travail ; le quatrième dépend du troisième.

## Volet 1 — Site de doc

`docs/spec/` (spec vivante), `docs/specs/` (specs datées) et `docs/research/` rendus en site
statique. Outil : **mdBook** (binaire unique, zéro dépendance JS, rendu Markdown proche de GitHub,
recherche intégrée). `docs/runbooks/` et `docs/plans/` sont **exclus** : ce sont des notes
d'exploitation qui nomment des hôtes et des chemins du box.

- `docs/book.toml` + `SUMMARY.md` généré (script `scripts/gen-summary.sh` ou table écrite à la
  main, décidée à l'implémentation selon le volume).
- Critère d'acceptation : le site se charge, la recherche trouve « parser_version », et **aucune
  page de `runbooks/` ni de `plans/` n'est publiée** (vérifié par `grep` sur le répertoire de
  sortie dans la CI).

## Volet 2 — Rustdoc + landing des crates

`cargo doc --workspace --no-deps` publié sous `/api/`, plus une page d'accueil qui présente les
quatre crates (`storm-replay`, `storm-stats`, `storm-lobby`, `storm-replay-viewer`), leur état de
publication et les chiffres de parité déjà mesurés. `storm-codex-server` est **exclu** de la doc
publiée (`--exclude storm-codex-server`) : ses items nomment des routes d'admin et la forme de la
base.

- Critère d'acceptation : `/api/storm_replay/index.html` et `/api/storm_stats/index.html` répondent
  200, `storm_codex_server` absent du site.
- Note : docs.rs prendra le relais quand le jalon 6 publiera les crates ; Pages sert **avant**.

## Volet 3 — Playground WASM « dépose un replay »

Le volet central. Une page qui accepte un `.StormReplay` par glisser-déposer et affiche la fiche de
match complète — score, talents, chat, visionneuse 2D — **sans serveur**.

### Go technique déjà mesuré (2026-09-08)

```
cargo build -p storm-stats -p storm-replay-viewer --target wasm32-unknown-unknown --release
→ Finished en 17,59 s, aucune modification de code
```

Toute la chaîne est pure Rust : `nom-mpq` → `bzip2-rs` + `flate2`/`miniz_oxide`, plus `regex`,
`serde_json`, `serde`. **Aucune dépendance C**, qui était le seul risque bloquant. Le risque
résiduel est de **taille et de temps**, pas de faisabilité — il est traité par les budgets ci-dessous.

### Architecture

- **Nouveau crate `crates/storm-web`** (`crate-type = ["cdylib", "rlib"]`, `wasm-bindgen`), qui
  n'expose que deux fonctions :
  - `parse_match(bytes: &[u8]) -> Result<JsValue, JsValue>` → l'objet `{match, players}` **exactement
    à la forme de `GET /api/matches/{id}`**, pour que le front n'ait pas deux modèles de données ;
  - `parse_replay2d(bytes: &[u8]) -> Result<JsValue, JsValue>` → la forme de
    `GET /api/matches/{id}/replay2d`.
  Ce crate est un **adaptateur, pas une ré-implémentation** : il appelle `storm_stats` et
  `storm_replay_viewer` tels quels. Toute divergence de sortie entre wasm et serveur serait une
  régression de parité — un test le verrouille (voir critères).
- **Web Worker** : le décodage tourne hors du thread principal (100 k game events par replay), le
  fichier est transféré en `ArrayBuffer` transférable. L'UI affiche une progression, ne gèle jamais.
- **Front** : la même SPA, avec une **source de données commutable**. `web/src/api.ts` est déjà la
  couche unique (convention de `08-frontend.md`) — on y introduit un adaptateur
  `source = "remote" | "wasm"` piloté par `import.meta.env.VITE_SOURCE`. Le build box garde
  `remote` et ne change pas de comportement.
- **Routes du build Pages** : `/play` (dépôt + fiche de match) uniquement. Les pages qui n'ont de
  sens que contre la base (Matches, Player, Heroes, Trends, Draft, Admin, overlays OBS) ne sont pas
  montées ; la nav ne les affiche pas.

### Dégradations assumées (pas de référentiel `dim_*` hors du box)

| Donnée | Sur le box | Sur Pages |
|---|---|---|
| Minimaps de la visionneuse 2D | `/images/minimaps` | **identiques** — `assets/minimaps` est committé (5,2 Mo), embarqué dans le build |
| Portraits de héros | téléchargés depuis HotsPatchNotes dans `images_dir` | **absents** → fallback initiales, déjà géré par `Avatar` |
| Noms de talents (`dim_talents`) | résolus par `/api/dim/talents` | **`talentTreeId` décamelisé**, déjà le fallback de `TalentStrip` |
| Couleur d'anneau par univers | `useDimHeroes` | neutre |

Une V2 possible (**hors de ce volet**) : publier un `dim-snapshot.json` généré depuis l'API du box
et chargé à la demande, ce qui rétablirait noms de talents et portraits. À chiffrer avant de
l'ouvrir — pas dans le périmètre validé ici.

### Budgets (contrats, à mesurer — pas à estimer)

| Budget | Valeur | Comment on mesure |
|---|---|---|
| Taille du `.wasm` servi | **≤ 3 Mo gzip** | taille du fichier publié, imprimée par la CI |
| Décodage + stats d'un replay type | **≤ 1 s** sur le PC de jeu, Chrome | `performance.now()` autour de l'appel worker, médiane sur les 4 replays de test |
| Chargement initial de `/play` | ≤ 2 s | mesure navigateur, réseau non bridé |

Référence : 133 ms en natif (`docs/spec/03-storm-stats.md`). Un facteur ~5 en wasm reste dans le
budget ; au-delà de 1 s, `wasm-opt -Oz` puis, si nécessaire, réexamen du volet.

### Critères d'acceptation

1. Les **4 replays de test committés** (`crates/*/tests/data/*.StormReplay`) déposés dans le
   playground produisent une fiche de match complète : score, chat, timeline, visionneuse 2D qui
   joue.
2. **Parité wasm ↔ serveur prouvée** : un test compare, pour ces 4 replays, la sortie de
   `storm_web::parse_match` à celle du chemin serveur — **égalité JSON stricte**. Une divergence
   est un échec, pas une tolérance.
3. Budgets ci-dessus mesurés et inscrits dans la spec vivante.
4. **Aucune requête réseau sortante** pendant un décodage : vérifié par l'onglet réseau (seuls les
   assets statiques du site apparaissent).

## Volet 4 — Partage statique d'un match

Envoyer une partie à des coéquipiers sans ouvrir le box ni leur donner Tailscale.

- **Mécanique** : un export depuis la fiche de match du box produit un fichier
  `share/<slug>.json` (l'objet `{match, players}` + le bloc `replay2d`) ; le fichier est committé
  dans le repo, publié par la même Action, et lu par le viewer du volet 3 via `/play#/share/<slug>`.
  Aucun code serveur nouveau : l'export est un bouton qui télécharge ce que l'API renvoie déjà.
- **Décision de vie privée** (à trancher par l'opérateur, recommandation par défaut) : l'export
  **pseudonymise les BattleTags des joueurs non-opérateur** (`Joueur 2`… ), avec une case pour
  garder les vrais noms quand la partie est une partie d'équipe consentante. Les héros, stats,
  positions et le **chat** ne sont pas anonymisables sans vider l'export de son intérêt : le fil de
  chat part tel quel, ce que l'opérateur assume à l'export.
- Critère d'acceptation : un match exporté depuis le box s'ouvre sur Pages, chat et visionneuse
  compris, sans qu'aucune requête ne parte vers le box ; et un export pseudonymisé ne contient plus
  aucun BattleTag des autres joueurs (`grep` sur le JSON dans le test).

## Non-buts (explicites)

- Pas de miroir de la base sur Pages, pas de statistiques agrégées publiques.
- Pas d'overlay OBS sur Pages (ils dépendent du WS et de l'état de session).
- Pas de remplacement du box : Pages ne sert **jamais** de plan de repli pour le serveur.
- Pas de nom de domaine dédié en V1 (`matella.github.io/storm-codex` suffit).

## Impact documentaire (règle n° 0)

| Fichier | Ce qui change |
|---|---|
| `docs/spec/09-operations.md` | nouvelle Action `pages.yml`, ce qu'elle publie, ce qu'elle exclut |
| `docs/spec/08-frontend.md` | `VITE_SOURCE`, routage `HashRouter` du build Pages, route `/play`, dégradations |
| `docs/spec/01-architecture.md` | budgets wasm mesurés ; Pages posé comme surface publique disjointe |
| `docs/spec/README.md` | pointeur vers le crate `storm-web` |

## Questions ouvertes pour l'opérateur

1. **Volet 4** : pseudonymisation par défaut — d'accord, ou noms réels par défaut ?
2. **Nom du site** : `matella.github.io/storm-codex` ou un domaine à toi plus tard ?
3. Publier le **mini-corpus de replays** de test sur Pages pour que le playground ait un bouton
   « essayer avec un exemple » — ces fichiers sont déjà committés dans le repo public, donc c'est
   une exposition nulle en plus. Recommandation : oui.
