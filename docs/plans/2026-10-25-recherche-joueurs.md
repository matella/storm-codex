# Plan — recherche de joueurs + comptes liés (smurfs)

> **Pour l'exécutant :** utiliser superpowers:subagent-driven-development (recommandé) ou
> superpowers:executing-plans. Cases `- [ ]` = suivi. Lire d'abord CLAUDE.md et `docs/spec/10-developpement.md`.

**Objectif :** boîte de recherche de joueurs (pseudo actuel, anciens pseudos, fragment), onglet
« Identité » (historique daté + comptes devinés + confirmer/écarter), sans jamais fusionner de profils.

**Architecture :** nouveau module serveur `src/players/` — SQL de lecture (`names.rs`, `search.rs`),
**noyau de scoring pur et testé sans DB** (`similarity.rs`), chargement des indices + présélection
bornée (`identity.rs`), routes (`api.rs`). Une migration (`player_links`). Front : page `/players`,
onglet Identité dans la page `player/:toon` existante, helpers purs testés (`players.ts`).

**Stack :** Rust/axum/sqlx (Postgres), React + Vite + TS + TanStack Query, vitest.

**Spec :** `docs/specs/2026-10-25-recherche-joueurs-design.md` (validée le 2026-10-25) — **amendée par la
tâche 0** (voir « Corrections »).

## Corrections à la spec (trouvées en lisant le code et le corpus dev réel)

1. **« Même uploader » est inopérant tel qu'écrit** : chaque replay contient les 10 joueurs, donc les 10
   « partagent l'uploader ». Corpus dev : token 4 = 2 719 des 2 723 parties. Le signal devient
   **« propriétaire de token »** : un compte présent dans ≥ 50 % des parties d'un token (≥ 50 parties).
   Deux comptes propriétaires du **même** token = signal fort (le smurf de l'opérateur).
2. **Le co-équipier récurrent est biaisé par l'opérateur** (`matella` est dans 2 128 parties) : tout le
   monde serait « lié » via lui. Les **propriétaires de token et les `operator_names`** sont exclus du
   signal co-équipiers **et** de la présélection des candidats.
3. **`players.last_name` n'est pas le pseudo actuel** (UPSERT dans l'ordre de traitement, backfill en
   désordre) et `names` n'est pas chronologique. Pseudo actuel, « was … » et « 3 plus récents » viennent
   de l'**historique daté** (`match_players.data->>'name'` + `matches.played_at`). `played_at` NULL =
   plus ancien. `players.names` reste utilisé comme *ensemble* (matching de recherche).
4. **Routes** : la route front existante `player/:toon` est **conservée** (d'autres pages y renvoient) ;
   on ajoute `players` pour la recherche (la spec disait `/players/:toon`).
5. **Pas de `CREATE EXTENSION`** (pg_trgm/unaccent) : les migrations tournent au démarrage sur le
   Postgres partagé du box ; un échec de privilège empêcherait le serveur de démarrer. `lower()` + `ILIKE`
   d'abord (autorisé par la spec : « si la mesure l'exige »). Insensibilité aux accents : hors v1, à signaler.
6. **Couverture** (conséquence de l'existant, non re-tranchée) : les replays de scouting sont isolés par
   construction (migration 0010) et n'atteignent jamais `players` : un joueur vu **uniquement** en
   scouting n'est pas cherchable.

## Contraintes globales

- Règle n° 0 : **chaque tâche committe son diff `docs/spec/` avec son code** (05-api, 06-modele-donnees,
  08-frontend). Pas de tâche « docs » finale.
- `git add` avec **chemins explicites** (le repo contient un fichier non suivi `e399b92…` et
  `.superpowers/` non ignoré — ne pas les committer). Commits conventionnels, français ou anglais comme
  le reste de l'historique ; pied `Co-Authored-By: Claude Sonnet 5.5 <noreply@anthropic.com>`.
- Rust : clippy strict, `thiserror`, pas de `unwrap()` hors tests. **Context7 avant toute API
  axum / sqlx / TanStack Query** (`resolve-library-id` puis `query-docs`) ; le signaler s'il est absent.
- **Tests d'intégration** : ils s'ignorent sans `DATABASE_URL` (« ignoré » = vert vide). Toujours lancer
  avec `DATABASE_URL=postgres://storm:storm@localhost:5433/storm_codex` et **vérifier l'absence de la
  ligne « ignoré »**. Base partagée entre tests : fixtures à toon/pseudo uniques (`uuid`), assertions de
  *contenance*, jamais de comptes exacts globaux.
- Budgets : `GET /api/players/search` p95 < 100 ms ; identité bornée (≤ 30 candidats détaillés, pas de
  comparaison deux à deux de tous les joueurs). Mesurer sur le corpus dev (2 723 parties, 7 618 joueurs).
- Écritures `PUT …/links` : gardées par `crate::manage::is_admin` (mode ouvert si `ADMIN_TOKEN` vide).
- Lecture : visible de tous ; toute estimation est étiquetée « estimation » côté UI.

## Review Focus (cas que la spec sous-entend sans les tester)

1. Pseudo contenant `_` ou `%` (ex. `Kerrigan_Main`) : la recherche les traite **littéralement** (tâche 2).
2. Requête < 2 caractères / vide / espaces : liste vide, aucune requête SQL lourde (tâche 2).
3. Deux comptes qui ne partagent **que** l'opérateur ou le même token : **aucun lien** (tâche 3, 4).
4. Comptes à très peu de parties : le chevauchement de héros ne peut **jamais** atteindre HAUT (tâche 3).
5. `PUT …/links/{soi-même}` → 400 ; paire stockée ordonnée (`CHECK toon_a < toon_b`) ; PUT idempotent
   et symétrique (A→B ≡ B→A) (tâches 1, 5).
6. `played_at` NULL ou historique à un seul pseudo : pas de crash, ordre stable (tâche 1).

---

## Tâche 0 — Amender la spec (docs seulement)

**Fichiers :** modifier `docs/specs/2026-10-25-recherche-joueurs-design.md`.

- [ ] Reporter les 6 corrections ci-dessus dans la spec (sections Signaux, Données, API/Routes,
  Hors périmètre) et passer son statut à « validée, amendée le 2026-10-25 ».
- [ ] Commit : `docs(spec): recherche joueurs — signal uploader redéfini, pseudo actuel daté, routes`
  (`git add docs/specs/2026-10-25-recherche-joueurs-design.md docs/plans/2026-10-25-recherche-joueurs.md`).
- [ ] L'opérateur est informé que 1 et 2 corrigent le texte qu'il a validé (dans le message de fin).

## Tâche 1 — Migration `player_links` + historique daté des pseudos

**Fichiers :** créer `crates/storm-codex-server/migrations/0011_player_links.sql`,
`src/players/mod.rs`, `src/players/names.rs` ; modifier `src/main.rs` (`mod players;`), `src/read.rs`
(`get_player`, l. ~135-160) ; docs `06-modele-donnees.md`, `05-api.md`.

**Interfaces — produit :**
```rust
// src/players/names.rs
pub struct NameSpan { pub name: String, pub first_seen: Option<DateTime<Utc>>,
                      pub last_seen: Option<DateTime<Utc>>, pub games: i64 }
/// Triés du plus récent au plus ancien (last_seen DESC, NULL en dernier, puis nom ASC : ordre stable).
/// Vide si aucune ligne datée ou non ; l'appelant retombe alors sur `players.names`.
pub async fn name_history(db: &PgPool, toon: &str) -> Result<Vec<NameSpan>, sqlx::Error>;
/// Pseudo actuel = premier de `name_history`, sinon `players.last_name`, sinon None.
pub async fn current_name(db: &PgPool, toon: &str) -> Result<Option<String>, sqlx::Error>;
```

- [ ] **Migration** : `player_links(toon_a TEXT, toon_b TEXT, status TEXT NOT NULL, decided_at
  TIMESTAMPTZ NOT NULL DEFAULT now(), PRIMARY KEY (toon_a, toon_b), CHECK (toon_a < toon_b), CHECK
  (status IN ('confirmed','dismissed')))`. **Aucun `CREATE EXTENSION`.** Index sur `toon_b`.
- [ ] **Test (échoue d'abord)** `names::tests::historique_ordonne_et_current_name` (intégration, toon
  `uuid`) : insérer un match ancien (pseudo « Old »), un récent (pseudo « New »), un avec
  `played_at NULL` (pseudo « Undated ») ; attendre `[New, Old, Undated]`, `current_name == "New"`,
  `first_seen/last_seen` justes, `games` justes. Second test : un seul pseudo → un seul `NameSpan`.
- [ ] Implémenter `name_history` (un `GROUP BY data->>'name'` sur `match_players` JOIN `matches`) ;
  `current_name`.
- [ ] `get_player` : `name` = `current_name`, `names` = noms de `name_history` (récent d'abord),
  repli sur l'ancien comportement si vide. La forme JSON ne change pas (le front existant continue).
  Test de non-régression : les champs existants de `/api/players/{toon}` sont toujours présents.
- [ ] Exécuter avec `DATABASE_URL` ; vérifier absence de « ignoré » ; `cargo clippy --workspace -- -D warnings`.
- [ ] Docs : `06` (table `player_links`, 0011 ; règle « pseudo actuel = historique daté »), `05`
  (`/api/players/{toon}` : `name`/`names` sémantique). Commit :
  `feat(server): historique daté des pseudos + table player_links`.

## Tâche 2 — Recherche `GET /api/players/search`

**Fichiers :** créer `src/players/search.rs` ; modifier `src/main.rs` (route, **avant** `{toon}` dans la
lecture du routeur — vérifier avec Context7 la priorité statique/paramétrée d'axum) ; docs `05`.

**Interfaces — produit :**
```rust
pub fn escape_like(q: &str) -> String;           // échappe \ % _ ; pur
pub struct SearchHit { pub toon: String, pub name: Option<String>, pub matched_name: String,
    pub matched_old: bool, pub games: i64, pub wins: i64, pub roles: Vec<(String, i64)>,
    pub last_seen: Option<DateTime<Utc>> }        // roles : 2 premiers, via dim_heroes (hero→role)
pub async fn search(db: &PgPool, q: &str, limit: i64) -> Result<Vec<SearchHit>, sqlx::Error>;
```
Route : `GET /api/players/search?q=&limit=` (défaut 20, max 50) → `{ "hits": [...] }`.

- [ ] **Tests purs (échouent d'abord)** : `escape_like("Kerrigan_Main") == "Kerrigan\\_Main"`, `%` et `\`
  idem ; `q` trim + `chars().count() < 2` → `search` renvoie `[]` sans toucher la DB (test avec pool
  paresseux/`connect_lazy` ou fonction pure `normalize_query(&str) -> Option<String>`).
- [ ] **Test d'intégration** : fixtures uuid — joueur A (pseudo actuel `Zed{u}Knight`, ancien
  `Zed{u}Night`), joueur B (`Zed{u}_Main`). `q="zed{u}night"` trouve A avec `matched_old=true` ;
  `q="Zed{u}_M"` trouve B et **pas** A (le `_` est littéral : un `_` joker matcherait `Zed{u}XM…` — ajouter
  un joueur C `Zed{u}XMain` qui ne doit PAS sortir) ; tri exact > préfixe > sous-chaîne.
- [ ] **Test routeur** (`api_tests`) : `GET /api/players/search?q=ab` ne tombe pas dans `get_player`
  (réponse `{hits}` et non `{toon:"search"}`).
- [ ] Implémenter : filtre sur `players` (`lower(name) ILIKE … ESCAPE '\'` sur `jsonb_array_elements_text(names)`),
  tri/limite **d'abord**, puis agrégats (parties, victoires, rôles, last_seen, pseudo actuel) **seulement
  pour les ≤ limit lignes retenues** (budget p95).
- [ ] Mesure : 20 requêtes sur le corpus dev, noter p95 dans le message de commit (cible < 100 ms).
- [ ] Docs `05`. Commit : `feat(server): GET /api/players/search (pseudos actuels et anciens)`.

## Tâche 3 — Noyau de scoring pur `similarity.rs`

**Fichiers :** créer `src/players/similarity.rs` (aucune dépendance DB/axum).

**Interfaces — produit :**
```rust
pub struct PairFeatures {
    pub games_a: u32, pub games_b: u32,
    pub hero_cosine: f32,                  // 0..1, parts de parties par héros
    pub shared_teammates: u32,             // co-équipiers communs hors opérateur/propriétaires de token
    pub shared_teammate_games: u32,        // parties cumulées avec eux (borne la force)
    pub talent_agreement: Option<f32>,     // 0..1 sur héros joués ≥ 3 fois par les deux, None si aucun
    pub hour_cosine: f32,                  // histogramme 24 h UTC
    pub overlapping_games: bool,           // les deux comptes dans la même partie / parties chevauchantes
    pub same_token_owner: bool,
}
pub enum Level { High, Low }
pub struct Reason { pub kind: &'static str, pub text: String }   // kind : "token"|"teammates"|"heroes"|"talents"|"hours"
pub struct Suggestion { pub level: Level, pub score: u8, pub reasons: Vec<Reason> }
/// None = pas de suggestion (score trop bas, ou exclusion temporelle).
pub fn score(f: &PairFeatures) -> Option<Suggestion>;
pub const MIN_GAMES_FOR_HIGH: u32 = 15;   // point de départ, à régler en tâche 8
```
Règles (spec « Signaux ») : `overlapping_games` → `None` ; `same_token_owner` = signal fort ; teammates
et talents = moyens ; heroes = faible-moyen ; hours = faible ; **HAUT exige ≥ 2 signaux indépendants dont
un fort/moyen** et `min(games_a, games_b) ≥ MIN_GAMES_FOR_HIGH` ; heroes/hours seuls → au mieux BAS ;
chaque suggestion porte ses `reasons` lisibles.

- [ ] **Tests (échouent d'abord)**, un par comportement : `chevauchement_exclut`,
  `heroes_seuls_jamais_haut` (cosine 0.99, rien d'autre), `peu_de_parties_jamais_haut` (signaux forts mais
  `games_a = 3`), `token_plus_heures_donne_haut`, `teammates_plus_talents_donne_haut`,
  `aucun_signal_none`, `deux_comptes_partageant_seulement_loperateur_aucun_lien`
  (`shared_teammates = 0`, `same_token_owner = false`, cosine moyen → `None`), `raisons_non_vides`.
- [ ] Implémenter `score` (pondération libre, constantes nommées en tête de fichier, doc d'une ligne
  chacune). Test `score_monotone` : augmenter un signal ne baisse jamais le score.
- [ ] `cargo test -p storm-codex-server similarity` vert ; clippy.
- [ ] Docs : `05`/`06` non concernés ; section « Signaux » de la spec renvoie à `similarity.rs`
  (commit inclut ce renvoi dans la spec). Commit : `feat(server): scoring pur des comptes liés`.

## Tâche 4 — Indices et présélection bornée `identity.rs`

**Fichiers :** créer `src/players/identity.rs`.

**Interfaces :**
- Consomme : `names::name_history/current_name` (t.1), `similarity::{PairFeatures, score, Suggestion}` (t.3).
- Produit :
```rust
/// Opérateur (`app_settings.operator_names` → toons dont un pseudo correspond) ∪ propriétaires de token
/// (présents dans ≥ 50 % des parties d'un token ayant ≥ 50 parties, via uploads.token_id/match_id).
pub async fn excluded_toons(db: &PgPool) -> Result<HashSet<String>, sqlx::Error>;
/// Comptes propriétaires de chaque token : token_id → toons (pour `same_token_owner`).
pub async fn token_owners(db: &PgPool) -> Result<HashMap<i64, HashSet<String>>, sqlx::Error>;
pub struct Candidate { pub toon: String, pub name: Option<String>, pub suggestion: Suggestion }
/// ≤ 30 candidats détaillés, triés par score décroissant. Exclut `target` lui-même, les `excluded_toons`
/// (sauf pour le signal token), et les paires déjà décidées dans `player_links`.
pub async fn suggest(db: &PgPool, target: &str) -> Result<Vec<Candidate>, sqlx::Error>;
```
Présélection : (a) vecteurs héros de tous les joueurs ≥ `MIN_GAMES` en **une** requête, cosinus calculé en
Rust, top 30 ; (b) ∪ joueurs partageant ≥ 3 parties d'équipe avec la cible hors exclus ; puis détail
(talents, heures, chevauchement) pour ≤ 30 seulement. Talents : `match_players.data->'talents'`
(`Tier1Choice`…`Tier7Choice`, objet vu sur le corpus dev).

- [ ] **Tests d'intégration (échouent d'abord)**, fixtures `uuid`, helper `insert_match(db, played_at,
  players: &[(toon, name, team, hero, talents)])` :
  `smurf_evident_est_suggere` (deux comptes : mêmes héros/talents, mêmes co-équipiers non-opérateur,
  jamais dans la même partie → suggestion HAUT) ; `lien_via_loperateur_seul_ignore` (deux comptes ne
  partageant que l'opérateur → absents) ; `comptes_dans_la_meme_partie_exclus` ; `paire_ecartee_nest_plus_proposee`
  (ligne `dismissed` insérée) ; `deux_proprietaires_du_meme_token_fort` (uploads + `token_id` fictif ≥ 50 parties) ;
  `borne_30_candidats`.
- [ ] Implémenter ; EXPLAIN/chrono sur le corpus dev : `suggest` d'un joueur à 300 parties < 1 s (budget
  « borné », à consigner ; pas de p95 contractuel).
- [ ] Commit : `feat(server): présélection et indices des comptes liés`.

## Tâche 5 — Routes identité + décision

**Fichiers :** créer `src/players/api.rs` ; modifier `src/main.rs` (routes) ; docs `05`.

**Interfaces — produit :**
- `GET /api/players/{toon}/identity` →
  `{ toon, name, names:[{name, first_seen, last_seen, games}], suggestions:[{toon, name, level:"high"|"low",
  score, reasons:[{kind,text}]}], confirmed:[{toon, name, decided_at}] }` (404 si toon inconnu).
- `PUT /api/players/{toon}/links/{other}` 🔒 corps `{ "status": "confirmed" | "dismissed" }` → 200
  `{status}` ; `toon == other` → 400 ; paire normalisée (min,max) ; **upsert idempotent**.

- [ ] **Tests routeur (échouent d'abord)** : identity d'un toon inconnu → 404 ; PUT soi-même → 400 ;
  PUT avec `ADMIN_TOKEN` défini sans Bearer → 401, avec Bearer → 200 ; PUT A→B puis lecture depuis B
  montre le lien confirmé (symétrie) ; PUT deux fois = une seule ligne ; statut invalide → 400/422 ;
  `dismissed` retire la suggestion de la réponse suivante.
- [ ] Implémenter (garde `is_admin`, pattern `scouting/api.rs`) ; Context7 pour l'extracteur `Json`/`Path`.
- [ ] Docs `05` (3 routes). Commit : `feat(server): GET identity + PUT links (comptes liés)`.

## Tâche 6 — Couche de données front + helpers purs

**Fichiers :** modifier `web/src/api.ts` ; créer `web/src/players.ts`, `web/src/players.test.ts`.

**Interfaces — produit :**
```ts
export interface PlayerHit { toon: string; name: string | null; matched_name: string; matched_old: boolean;
  games: number; wins: number; roles: [string, number][]; last_seen: string | null }
export interface NameSpan { name: string; first_seen: string | null; last_seen: string | null; games: number }
export interface LinkSuggestion { toon: string; name: string | null; level: "high" | "low"; score: number;
  reasons: { kind: string; text: string }[] }
export interface PlayerIdentity { toon: string; name: string | null; names: NameSpan[];
  suggestions: LinkSuggestion[]; confirmed: { toon: string; name: string | null; decided_at: string }[] }
export const searchPlayers = (q: string) => Promise<PlayerHit[]>;
export const fetchIdentity = (toon: string) => Promise<PlayerIdentity>;
export const putLink = (toon: string, other: string, status: "confirmed" | "dismissed") => Promise<void>; // via le helper d'écriture authentifiée (api.ts ~l.682)
// players.ts (purs)
export function highlightSegments(name: string, q: string): { text: string; hit: boolean }[];
export function recentNames(h: NameSpan[], n?: number): { shown: NameSpan[]; hidden: number };   // n = 3
export function seenRange(s: NameSpan): string;   // « jan–août 2025 », « jan 2025 », « date inconnue »
export function likelyAlias(i: PlayerIdentity): { toon: string; name: string | null } | null; // confirmé d'abord, sinon 1er HAUT, jamais BAS
```
- [ ] **Tests vitest (échouent d'abord)** : `highlightSegments` insensible à la casse, `q` vide → un seul
  segment non surligné, `q` contenant des caractères regex (`_`, `.`, `(`) traité littéralement ;
  `recentNames` 14 noms → 3 + hidden 11 ; ≤ 3 → hidden 0 ; `seenRange` avec dates nulles ; `likelyAlias`
  ne renvoie **jamais** une suggestion BAS, préfère un lien confirmé.
- [ ] Implémenter ; `npm test` + `npx tsc --noEmit`. Docs `08` (types ajoutés). Commit :
  `feat(web): types et helpers de recherche/identité des joueurs`.

## Tâche 7 — Pages : recherche, onglet Identité, ligne d'indice

**Fichiers :** créer `web/src/pages/Players.tsx`, `web/src/components/PlayerIdentity.tsx` ; modifier
`web/src/App.tsx` (route `players`), `web/src/components/Layout.tsx` (entrée de nav « Players »),
`web/src/pages/Player.tsx` (onglets Stats | Identity + ligne d'indice) ; docs `08-frontend.md`.

Maquette validée : `.superpowers/brainstorm/171-1792937326/content/search-and-profile-nexus.html`
(variante **B** + ligne d'indice). Classes `ds-` (`ds.css`), tokens Nexus Codex.

- [ ] Context7 : TanStack Query v5 (`useQuery` avec debounce de la clé, `useMutation` + `invalidateQueries`).
- [ ] `Players.tsx` : champ (debounce ~250 ms, `q` dans l'URL `?q=`), < 2 caractères = état vide explicatif,
  rangées « médaillon » (nom surligné via `highlightSegments`, étiquette « was … » si `matched_old`, parties,
  winrate, rôles, dernière partie), lien vers `/player/:toon`. États chargement / aucun résultat.
- [ ] `PlayerIdentity.tsx` : noms récents (`recentNames`) + « show all (N) » dépliant la liste datée
  (`seenRange`), défilante ; encart pointillé or « Estimation » avec comptes devinés (badge HAUT/BAS +
  score + raisons) ; boutons **Confirm** / **Not the same person** (`putLink`, en cas de 401 message
  « admin token required » renvoyant à `/admin`) ; liens confirmés listés à part. **Pas de bouton de fusion.**
- [ ] `Player.tsx` : onglets ; ligne « Also plays as X → » (onglet Identité) uniquement si
  `likelyAlias(identity)` ; l'identité se charge séparément (le résumé reste rapide).
- [ ] `npm run build` sans erreur ; `npm test` vert. Docs `08` (route `/players`, onglet Identité, ligne
  d'indice). Commit : `feat(web): page Players + onglet Identité`.

## Tâche 8 — Preuves, réglage des seuils, STATUS

- [ ] `DATABASE_URL=… cargo test --workspace` (pas de « ignoré »), `cargo clippy --workspace -- -D warnings`,
  `npm test`, `npm run build` : sorties collées dans le message final.
- [ ] **Réglage sur données réelles** : sur le corpus dev, lancer `identity` pour 5 joueurs ayant plusieurs
  pseudos (`jsonb_array_length(names) > 1`, 95 existent) et les 3 premiers comptes par parties ; relire les
  suggestions HAUT à la main avec l'opérateur ; ajuster **seulement** les constantes de `similarity.rs`
  (tests de la tâche 3 restent verts) ; consigner les cas relus dans `docs/STATUS.md`.
- [ ] Mesures : p95 de `search` (cible < 100 ms) et durée de `identity` pour un joueur lourd, relevées sur
  le serveur release + Postgres dev ; si p95 > 100 ms, ajouter l'index trigram **dans une migration
  séparée, hors démarrage bloquant, après accord de l'opérateur**.
- [ ] Vérification navigateur (preview_start) : recherche d'un ancien pseudo, ouverture du profil, onglet
  Identité, confirmer puis écarter un lien, capture.
- [ ] `docs/STATUS.md` : état + prochaine étape. Commit : `docs(status): recherche joueurs livrée`.

## Hors plan (volontairement)

- Annuler une décision (pas de `DELETE …/links`) : absent de la spec, à proposer à l'opérateur si besoin.
- Recherche insensible aux accents, extension trigram, joueurs vus seulement en scouting.
- Fuseau horaire de l'opérateur pour les plages horaires (UTC pour les deux comptes, donc comparable).
