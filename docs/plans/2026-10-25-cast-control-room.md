# Régie de cast — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Une page `/cast` pilote une série (équipes, pool de cartes, parties, lineups, draft, bandeau en partie) et une seule source OBS `/cast/overlay` suit la scène choisie.

**Architecture:** Un module serveur `cast` porte l'état de série (singleton JSON persisté dans `cast_live`, comme `draft_live`), un moteur pur testé sans I/O, des routes `/api/cast/*` qui persistent puis diffusent `cast.updated` sur le WS. La série est autoritaire pour équipes/score/Bo ; `DraftState` garde ses champs et reçoit une synchro à sens unique. Le front ajoute `/cast` (régie, disposition « B ») et `/cast/overlay` (scènes) ; le draft existant est extrait en composants réutilisés par la régie.

**Tech Stack:** Rust (axum, tokio, serde, sqlx, thiserror), storm-stats, React + Vite + TS + TanStack Query, vitest. **Consulter Context7 (axum, TanStack Query, React Router) avant d'utiliser leurs API** (règle du projet).

**Spec:** `docs/specs/2026-10-25-cast-control-room-design.md` (+ maquettes `docs/specs/2026-10-25-cast-mockups/`). Lire aussi `docs/specs/2026-10-25-draft-replay-design.md`.

## Global Constraints

- **Règle n° 0** : toute tâche qui change un contrat met à jour `docs/spec/` dans le même commit (05-api, 04-serveur, 06-modele-donnees, 07-evenements, 08-frontend).
- Rust : clippy strict (`-D warnings`), `unwrap()` interdit hors tests, erreurs typées `thiserror`, commits conventionnels. Aucun `unwrap` dans les tests d'intégration sans `#![allow(clippy::unwrap_used)]` local (voir `players/testutil.rs`).
- Tests DB : `DATABASE_URL=postgres://storm:storm@localhost:5433/storm_codex cargo test -p storm-codex-server` (sans elle les tests DB s'ignorent en silence — **toujours la fournir**). Front : `cd web && npm test && npm run build`.
- Une seule source OBS `/cast/overlay`, 1920×1080, fond transparent ; changement de scène → overlay à jour **< 500 ms** (mesuré).
- Scènes : `lineups`, `series`, `draft`, `ingame`, `hide`. Libellés de l'UI en **anglais**.
- Statuts de carte : `available` · `banned(by)` · `picked(by)` · `decider` ; **aucun ordre imposé** ; un seul `decider` ; la carte d'une partie est choisie parmi `picked`/`decider`.
- Carte jouée : « **WON** » dans la couleur de l'équipe gagnante, **sans chiffres** ; « **NEXT** » doré sur la prochaine.
- Onglets en partie : **rectangles** (pas d'angles), coin supérieur interne ancré à **39,2 %** (Blue) et **59,65 %** (Red) de la largeur, **y = 9,9 %**, hauteur **4,6 %**, largeur max **26 %**, nom tronqué par « … » ; pastille haut-droite = `BO{n}` + `GAME {k}` + format (`FEARLESS`/`STANDARD`/`NORMAL`). Ancres = **constantes réglables**.
- Lineups : mode A (aucune partie terminée) = 3 héros les plus joués sur les **20 dernières parties** (tous modes), **masqués sous 5 parties** ; **pas de win rate** ; ligne libre saisie. Mode B = héros joués dans la série, verrouillés Fearless atténués ; bascule auto A→B à la 1re partie terminée, forçage manuel.
- Logos : PNG/WebP/JPEG **≤ 512 Ko** en base, ou URL. Post-game : replay **Custom uniquement**, **rien écrit dans `matches`/`uploads`**, résumé compact gardé dans l'état de série.
- Une valeur de breakdown absente de la sortie storm-stats est **retirée de l'écran**, jamais inventée ; aucune nouvelle métrique dans storm-stats.
- `SCHEMA_VERSION` de l'état de série = 1 ; un état persisté plus ancien/inconnu doit se charger ou retomber sur un état neuf, jamais faire échouer le démarrage.

## Review Focus

- Noms d'équipe de 40+ caractères ou vides : onglet tronqué sans déborder sur le HUD ; vide → « Blue »/« Red » (Task 5).
- Opérations de série invalides (carte hors pool, deux `decider`, bannie puis choisie, vainqueur d'une partie inconnue/posé deux fois, démarrer une partie sur une carte non choisie) : 409, aucun changement d'état (Task 1 + 2).
- Replay de post-game non Custom, tronqué, ou aucun replay pour une partie : refus propre / slides vides, jamais 500 ni écran cassé (Task 9).
- Joueur sans résultat, plusieurs toons, ou < 5 parties récentes : ligne de héros masquée, carte toujours propre (Task 8).
- Logo trop gros ou de mauvais type, corps vide : 413/415/400, rien stocké (Task 3).

---

### Task 1: Moteur de série (pur)

**Files:**
- Create: `crates/storm-codex-server/src/cast/mod.rs` (types, `CastError`), `crates/storm-codex-server/src/cast/series.rs` (opérations pures + tests)
- Modify: `crates/storm-codex-server/src/main.rs` (`mod cast;`)

**Interfaces:**
- Produces (réutilise `draft::{Side, Format}`) :
  - `pub enum Scene { Lineups, Series, Draft, Ingame, Hide }` (serde `snake_case`, défaut `Hide`)
  - `pub enum MapStatus { Available, Banned { by: Side }, Picked { by: Side }, Decider }` (serde taggé `status`)
  - `pub struct PoolMap { pub name: String, #[serde(flatten)] pub status: MapStatus }`
  - `pub struct Player { pub name: String, pub tagline: String, pub toon: Option<String> }` (défaut vide)
  - `pub struct Team { pub name: String, pub logo: Option<String>, pub record: String, pub players: [Player; 5], pub captain: Option<u8> }`
  - `pub struct GameRecord { pub number: u8, pub map: String, pub winner: Option<Side>, pub takedowns: [u32; 2], pub duration_secs: Option<u32>, pub summary: Option<GameSummary> }` (`GameSummary` défini Task 9 ; en attendant `pub struct GameSummary {}` vide `#[derive(Default)]`)
  - `pub enum LineupMode { Auto, A, B }` ; `pub struct SlideState { pub game: Option<u8>, pub index: u8, pub auto: bool, pub interval_ms: u32 /*défaut 12_000*/ }`
  - `pub struct Series { schema_version: i32, active: bool, blue: Team, red: Team, bo: u8, format: Format, score_override: Option<[u8;2]>, pool: Vec<PoolMap>, games: Vec<GameRecord>, scene: Scene, lineup_mode: LineupMode, slide: SlideState }` + `Series::new()`
  - `pub enum CastError { UnknownMap, NotInPool, DuplicateDecider, MapNotChosen, UnknownGame, WinnerAlreadySet, SeriesInactive }`
  - `impl Series { pub fn set_pool(&mut self, names: &[String]); pub fn mark_map(&mut self, name: &str, status: MapStatus) -> Result<(), CastError>; pub fn start_game(&mut self, map: &str) -> Result<u8, CastError>; pub fn set_winner(&mut self, number: u8, side: Side) -> Result<(), CastError>; pub fn score(&self) -> [u8; 2]; pub fn next_map(&self) -> Option<&str>; pub fn lineup_mode_effective(&self) -> LineupMode /* A ou B */; pub fn display_name(&self, side: Side) -> &str /* « Blue »/« Red » si vide */ }`

- [ ] **Step 1: Tests (rouges)** dans `series.rs` : `set_pool_keeps_status_of_retained_maps_and_drops_others`, `mark_map_rejects_map_outside_pool` (`NotInPool`), `second_decider_is_rejected` (`DuplicateDecider`, état inchangé), `marking_banned_then_picked_replaces_status` (dernier statut gagne), `start_game_requires_picked_or_decider` (`MapNotChosen` pour `Available`/`Banned`), `start_game_numbers_games_from_one_and_rejects_replaying_a_map` (la même carte deux fois → `MapNotChosen`), `set_winner_unknown_game` (`UnknownGame`), `set_winner_twice` (`WinnerAlreadySet`), `score_counts_winners_unless_overridden`, `next_map_is_first_picked_or_decider_without_a_game` (ordre du pool), `lineup_mode_auto_switches_to_b_after_first_finished_game`, `lineup_mode_forced_wins_over_auto`, `display_name_falls_back_to_blue_red_when_blank`, `series_roundtrips_json_and_loads_default_for_missing_fields` (JSON `{}` partiel → défauts, pas d'erreur).
- [ ] **Step 2:** `cargo test -p storm-codex-server cast::` → rouge.
- [ ] **Step 3: Implémenter** types + méthodes ci-dessus (`#[serde(default)]` partout pour la tolérance de schéma ; `Series::new()` = `active: false`, `bo: 5`, `format: Fearless`, scène `Hide`). `mark_map` n'accepte `Decider` que si aucune autre carte ne l'est.
- [ ] **Step 4:** tests PASS ; `cargo clippy -p storm-codex-server --all-targets -- -D warnings`.
- [ ] **Step 5: Commit** `feat(cast): moteur de série pur — pool, parties, score, modes de lineup`

### Task 2: Persistance, routes `/api/cast/*`, WS

**Files:**
- Create: `crates/storm-codex-server/migrations/0012_cast_live.sql`, `crates/storm-codex-server/src/cast/store.rs`, `crates/storm-codex-server/src/cast/api.rs`
- Modify: `main.rs` (champ `cast: Arc<RwLock<cast::Series>>` dans `AppState`, chargement au démarrage avec repli état neuf, routes), `docs/spec/{04-serveur,05-api,06-modele-donnees,07-evenements}.md`
- Test: tests d'intégration dans `cast/api.rs` (modèle : `draft/replay_api.rs`, `players/testutil.rs`)

**Interfaces:**
- Produces : table `cast_live (id INT PK DEFAULT 1 CHECK (id=1), state JSONB NOT NULL, updated_at TIMESTAMPTZ NOT NULL DEFAULT now())` ; `store::load(&PgPool) -> Option<Series>`, `store::save(&PgPool, &Series) -> Result<(), sqlx::Error>` ; `async fn persist(&AppState)` (save + `events.send({"type":"cast.updated"})`).
- Routes (JSON) : `GET /api/cast` → `Series` ; `POST /api/cast/series` `{blue:{name}, red:{name}, bo, format, pool:[nom]}` (crée/redémarre : `active=true`, parties/score remis à zéro) ; `PUT /api/cast/teams` `{blue: Team, red: Team}` ; `POST /api/cast/pool/map` `{name, status}` ; `POST /api/cast/games/start` `{map}` → `{number}` ; `POST /api/cast/games/{n}/winner` `{side, takedowns?, duration_secs?}` ; `POST /api/cast/score` `{blue, red}|null` ; `POST /api/cast/scene` `{scene}` ; `POST /api/cast/lineup-mode` `{mode}` ; `POST /api/cast/slide` `{game?, index?, auto?, interval_ms?}`. Erreurs `CastError` → `409 {"error": "<Variante>"}` sans changement d'état. Mutations : un seul write-lock, puis `persist`.

- [ ] **Step 1: Tests (rouges)** : `get_cast_returns_default_inactive_series`, `post_series_activates_and_resets_games`, `invalid_map_marking_returns_409_and_leaves_state` (carte hors pool), `scene_change_broadcasts_cast_updated` (abonné à `events` reçoit `{"type":"cast.updated"}`), `state_survives_restart` (save → `store::load` identique), `load_of_garbage_json_returns_none` (ligne `{"x":1}` illisible → `None` n'est **pas** requis : `{}` partiel charge des défauts ; JSON non-objet → `None`), `scene_switch_to_overlay_poll_under_500ms` (POST scene puis GET : < 500 ms, mesuré avec `Instant`).
- [ ] **Step 2:** `cargo test -p storm-codex-server cast::` → rouge (migration/routes absentes).
- [ ] **Step 3: Implémenter** migration, store, handlers, enregistrer les routes **avant** les routes génériques, charger `cast` dans `AppState` (`store::load(&db).await.unwrap_or_else(Series::new)`). Mode ouvert comme `/api/draft`.
- [ ] **Step 4:** tests PASS + clippy ; mettre à jour les 4 docs spec (routes, table, event `cast.updated` sans état, module).
- [ ] **Step 5: Commit** `feat(cast): routes /api/cast, persistance cast_live, event cast.updated`

### Task 3: Synchro série → draft, logos

**Files:**
- Create: `crates/storm-codex-server/src/cast/sync.rs`, `crates/storm-codex-server/migrations/0013_cast_logo.sql`
- Modify: `cast/api.rs` (appel de la synchro après chaque mutation d'équipes/score/Bo/format ; routes logo), `draft/api.rs` (`teams`/`score`/`config` refusent avec `409 {"error":"series_active"}` quand `cast.active`), `main.rs` (routes), `docs/spec/{05-api,06-modele-donnees}.md`
- Test: dans `sync.rs` et `cast/api.rs`

**Interfaces:**
- Consumes : `Series`, `DraftState` (champs `blue`, `red` = `TeamInfo{name, players:[String;5]}`, `score:[u8;2]`, `bo`, `format`).
- Produces : `pub fn sync_into_draft(series: &Series, draft: &mut DraftState)` (copie noms, pseudos, score = `series.score()`, `bo`, **sans** toucher curseur/assignations/file replay) ; table `cast_logo (side TEXT PRIMARY KEY CHECK (side IN ('blue','red')), mime TEXT NOT NULL, bytes BYTEA NOT NULL, updated_at TIMESTAMPTZ NOT NULL DEFAULT now())` ; routes `POST /api/cast/logo/{side}` (corps = octets, en-tête `Content-Type`) et `GET /api/cast/logo/{side}` ; après upload `Team.logo = Some("/api/cast/logo/{side}?v={updated_at_ms}")`.

- [ ] **Step 1: Tests (rouges)** : `sync_copies_names_players_score_bo` ; `sync_leaves_draft_progress_untouched` (curseur, assignations, `replay.queue` identiques avant/après) ; `draft_teams_route_returns_409_when_series_active` et `..._200_when_inactive` ; `logo_upload_stores_and_serves_same_bytes` (PNG minimal) ; `logo_over_512kb_is_413` ; `logo_wrong_mime_is_415` (`image/gif`) ; `logo_empty_body_is_400` ; aucun octet stocké dans les trois cas d'erreur.
- [ ] **Step 2:** rouge. **Step 3: Implémenter** (`DefaultBodyLimit` autorise jusqu'à 513 Ko pour renvoyer 413 proprement). **Step 4:** PASS + clippy + docs. **Step 5: Commit** `feat(cast): synchro série→draft à sens unique, logos en base`

### Task 4: Couche API front + utilitaires purs

**Files:**
- Create: `web/src/castApi.ts` (types miroir de `Series`, fetchers, `useCast()`), `web/src/castState.ts` (+ `castState.test.ts`)
- Modify: `web/src/api.ts` (dans `useLiveUpdates`, `cast.updated` → `qc.invalidateQueries({ queryKey: ["cast"] })`)

**Interfaces:**
- Produces : `export const fetchCast = () => get<CastSeries>("/api/cast")` et un fetcher par route de Task 2 (`castSeries`, `castTeams`, `castMarkMap`, `castStartGame`, `castWinner`, `castScore`, `castScene`, `castLineupMode`, `castSlide`, `castLogoUpload(side, file)`) ; `useCast(): UseQueryResult<CastSeries>` (clé `["cast"]`). `castState.ts` : `mapCardView(m: PoolMap, games: GameRecord[], nextMap: string|null): { tone: "blue"|"red"|"gold"|"muted"|"none"; label: string; struck: boolean }` (« WON » + couleur du vainqueur ; « NEXT » or ; `banned by X` barrée ; `picked by X` ; `decider`) ; `effectiveLineupMode(s: CastSeries): "A"|"B"` ; `displayName(t: CastTeam, side: Side): string`.

- [ ] **Step 1: Tests vitest (rouges)** `castState.test.ts` : `mapCardView` — carte jouée gagnée par Blue → `{tone:"blue", label:"WON"}` ; prochaine carte → `{tone:"gold", label:"NEXT"}` ; bannie par Red → `struck:true` ; `effectiveLineupMode` suit la règle auto/forcée ; `displayName` vide → « Blue ».
- [ ] **Step 2:** `cd web && npx vitest run castState` rouge. **Step 3: Implémenter.** **Step 4:** PASS + `npm run build` (types). **Step 5: Commit** `feat(web): couche API cast + helpers purs`

### Task 5: Overlay `/cast/overlay` — coque, scène In-game, Hide

**Files:**
- Create: `web/src/pages/CastOverlay.tsx`, `web/src/cast/anchors.ts` (+ `anchors.test.ts`), `web/src/cast/InGameScene.tsx`, `web/src/cast/cast-overlay.css`
- Modify: `web/src/App.tsx` (route hors `Layout`, comme `draft/overlay`), `docs/spec/08-frontend.md`

**Interfaces:**
- Consumes : `useCast`, `displayName`, `OverlayFrame`-style fond transparent.
- Produces : `export const INGAME_ANCHORS = { blueInnerX: 0.392, redInnerX: 0.5965, topY: 0.099, tabH: 0.046, maxW: 0.26 }` ; `export function tabBox(side: "blue"|"red", a = INGAME_ANCHORS): { top: string; height: string; maxWidth: string; left?: string; right?: string }` (pourcentages à 2 décimales ; Blue : `right = (1 - blueInnerX)`, Red : `left = redInnerX`) ; `CastOverlay` rend la scène courante (switch sur `scene`), `data-skin` via `?skin=` comme `DraftOverlay`, transition d'opacité ≈ 300 ms ; scène `draft`/`series`/`lineups` = composant vide jusqu'aux Tasks 7/8/10.

- [ ] **Step 1: Tests (rouges)** `anchors.test.ts` : `tabBox("blue")` → `{top:"9.90%", height:"4.60%", maxWidth:"26.00%", right:"60.80%"}` ; `tabBox("red")` → `left:"59.65%"` ; `tabBox` accepte des ancres surchargées (décalage fin).
- [ ] **Step 2:** rouge. **Step 3: Implémenter** : `InGameScene` = deux onglets rectangulaires (nom `overflow:hidden; text-overflow:ellipsis; white-space:nowrap`, score de série, soulignement couleur d'équipe) + pastille haut-droite (`BO{bo}`, `GAME {games.length+1}`, format en capitales) ; noms via `displayName`. Test composant léger : nom de 40 caractères → l'élément a `text-overflow: ellipsis` (style calculé via `getComputedStyle` ou classe) et ne dépasse pas `maxWidth`.
- [ ] **Step 4:** `npm test && npm run build` ; vérifier dans le navigateur (`preview_start`) : `POST /api/cast/series` puis `POST /api/cast/scene {"scene":"ingame"}`, l'overlay change sans rechargement ; capture sur la capture réelle `docs/specs/2026-10-25-cast-mockups/ingame.webp` en fond (alignement des coins sur les points rouges). Docs 08-frontend.
- [ ] **Step 5: Commit** `feat(web): overlay /cast/overlay — scène In-game, Hide, ancres réglables`

### Task 6: Régie `/cast` — coque et rail série

**Files:**
- Create: `web/src/pages/Cast.tsx`, `web/src/cast/SeriesRail.tsx` (équipes, score, Bo, mode, logos, lineups saisie), `web/src/cast/PoolPanel.tsx` (pool + veto + parties), `web/src/cast/ScenePreview.tsx`, `web/src/cast/cast.css`
- Modify: `web/src/App.tsx` (route `cast` dans `Layout`), `web/src/components/Layout.tsx` (lien « Cast »), `docs/spec/08-frontend.md`

**Interfaces:**
- Consumes : fetchers/`useCast` (Task 4) ; liste des cartes sélectionnables = la constante `MAPS` de `pages/Draft.tsx` (la **déplacer** dans `web/src/castState.ts` et l'importer des deux côtés — une seule liste) ; visuels via `mapImage(map)` de `api.ts` (peut renvoyer une image 404 → fond uni de repli).
- Produces : disposition B : rail gauche (Série / Teams & lineups / Maps), barre d'onglets de scène avec indicateur « on air », `ScenePreview` = `<iframe src="/cast/overlay">` 1920×1080 mis à l'échelle (CSS `transform: scale`) ; espace de travail par scène (`ingame` : steppers de score + correction de pastille ; `hide` : rien ; les autres : emplacements remplis aux Tasks 7–10) ; `PoolPanel` : cases du pool (≈ 10 cartes), boutons Ban Blue/Ban Red/Pick Blue/Pick Red/Decider/Reset par carte, « Start game » (liste des cartes choisies), vainqueur par partie.

- [ ] **Step 1: Tests vitest (rouges)** sur les helpers de formulaire extraits en fonctions pures dans `castState.ts` : `togglePool(names: string[], map: string): string[]` (ajout/retrait, ordre stable), `startableMaps(s: CastSeries): string[]` (cartes `picked`/`decider` sans partie), `canSetWinner(s, n): boolean`.
- [ ] **Step 2:** rouge. **Step 3: Implémenter** page + panneaux (libellés anglais, tokens `ds.css`/dossier). **Step 4:** `npm test && npm run build` ; vérifier dans le navigateur : créer une série (2 équipes, Bo5, pool de 10), marquer bans/picks dans un ordre quelconque, démarrer une partie, poser un vainqueur ; l'aperçu et le score suivent ; rafraîchir la page ne perd rien. Docs 08.
- [ ] **Step 5: Commit** `feat(web): régie /cast — rail série, pool de cartes, scènes et aperçu`

### Task 7: Scène Series — bandeau des cartes

**Files:**
- Create: `web/src/cast/SeriesScene.tsx` ; Modify: `CastOverlay.tsx`, `Cast.tsx` (espace de travail Series : choix du jeu affiché, slide courante, auto/manuel — branché Task 9), `cast-overlay.css`

**Interfaces:**
- Consumes : `mapCardView`, `useCast`, visuels `mapImage(map)` (`api.ts`).
- Produces : `SeriesScene` = en-tête (noms + score de série) + bandeau de toutes les cartes du pool (une carte = visuel, nom, « picked/banned by X », `WON` couleur d'équipe, `NEXT` doré, `decider`, bannies barrées) + zone de slides (vide jusqu'à Task 9).

- [ ] **Step 1: Tests (rouges)** composant : pool de 6 cartes (1 jouée gagnée par Blue, 1 jouée gagnée par Red, 1 next, 1 decider, 2 bannies) → le rendu contient exactement un « NEXT », deux « WON » avec les classes `tone-blue`/`tone-red`, deux éléments `struck`, aucun chiffre de score sur les cartes.
- [ ] **Step 2–4:** rouge → implémenter → `npm test && npm run build` + vérification navigateur (scène `series`). **Step 5: Commit** `feat(web): scène Series — bandeau des cartes (WON/NEXT)`

### Task 8: Lineups — héros récents (serveur) et cartes d'équipe A/B

**Files:**
- Create: `crates/storm-codex-server/src/cast/lineup.rs`, `web/src/cast/LineupsScene.tsx`, `web/src/cast/PlayerPicker.tsx`
- Modify: `cast/api.rs` + `main.rs` (route), `Cast.tsx` (espace de travail Lineups : mode Auto/A/B, picker), `docs/spec/{05-api,08-frontend}.md`
- Test: dans `lineup.rs` (DB, modèle `players/testutil.rs`) ; vitest pour la scène

**Interfaces:**
- Consumes : tables `match_players(match_id, toon_handle, hero)`, `matches(played_at)`, historique de pseudos (`players::names`), `GET /api/players/search`.
- Produces : `pub async fn recent_heroes(db: &PgPool, toon: &str) -> Result<Vec<HeroCount>, sqlx::Error>` (`HeroCount{hero:String, games:u32}`, ≤ 3, tri par parties puis récence, sur les 20 dernières parties **tous modes** ; **vide si < 5 parties**) ; `GET /api/cast/lineup?blue=toon,toon,…&red=…` → `{ blue: [[HeroCount]], red: [[HeroCount]] }` (budget < 100 ms pour 10 joueurs, mesuré) ; `PlayerPicker` (champ pseudo → `players/search` → liste de résultats `toon, name, games` → choisit ; aucun résultat = joueur « non lié », jamais de 1er résultat pris en silence) ; `LineupsScene` mode A (3 portraits de héros récents si présents + ligne libre) / mode B (héros joués dans la série par joueur depuis `games[].summary`, ceux verrouillés Fearless atténués).

- [ ] **Step 1: Tests (rouges)** serveur : `recent_heroes_top3_over_last_20_games` (25 parties insérées, seule la fenêtre des 20 dernières compte), `recent_heroes_hidden_under_5_games` (4 parties → vide), `recent_heroes_counts_all_modes`, `lineup_route_under_100ms_for_10_players` ; vitest : `LineupsScene` mode A n'affiche **aucun** pourcentage ni « win » ; mode B atténue les héros verrouillés ; joueur sans toon → pas de rangée de héros.
- [ ] **Step 2–4:** rouge → implémenter (requête paramétrée, index existants à vérifier par `EXPLAIN` dans le test de budget) → PASS + clippy + `npm test && npm run build` + navigateur. **Step 5: Commit** `feat(cast): héros récents par joueur + cartes d'équipe A/B`

### Task 9: Post-game — résumé de replay, slides, diaporama

**Files:**
- Create: `crates/storm-codex-server/src/cast/summary.rs`, `crates/storm-codex-server/src/cast/fixtures/custom_summary.json` (sortie réduite d'un replay Custom réel), `web/src/cast/PostGameSlide.tsx`
- Modify: `cast/mod.rs` (`GameSummary` complet), `cast/api.rs` + `main.rs` (route), `web/src/cast/SeriesScene.tsx`, `Cast.tsx` (contrôles diaporama), `docs/spec/{05-api,06-modele-donnees,08-frontend}.md`

**Interfaces:**
- Consumes : `storm_stats::process_replay`, `storm_stats::Output { match_, players, status }` (modèle d'usage : `draft/replay_api.rs::load`).
- Produces : `pub struct GameSummary { pub duration_secs: u32, pub winner: Side, pub takedowns: [u32;2], pub team_level: [u32;2], pub structures: [u32;2], pub merc_camps: [u32;2], pub objectives: [u32;2], pub bans: [Vec<String>;2], pub players: Vec<PlayerSummary>, pub xp_lead: Vec<(u32, i32)> /*(s, avance Blue)*/ }` ; `pub struct PlayerSummary { pub side: Side, pub name: String, pub hero: String, pub takedowns: u32, pub deaths: u32, pub assists: u32, pub hero_damage: u64, pub siege_damage: u64, pub healing: u64 }` ; `pub fn summary_from_output(out: &storm_stats::Output) -> Result<GameSummary, SummaryError>` (`SummaryError { NotCustom, Parse(i64), Missing(&'static str) }`) ; `POST /api/cast/games/{n}/replay` (octets bruts, comme `draft/replay/load`) → 201 `GameSummary`, 422 `{error: not_custom|parse_failed|…}` ; **rien écrit en base** ; `winner`/`takedowns`/`duration_secs` du `GameRecord` remplis depuis le résumé.

- [ ] **Step 1 (vérification des données, avant tout code d'écran)** : sur le fixture et 3 replays Custom réels, lister dans la sortie storm-stats les chemins JSON de chaque valeur du breakdown (`match.teams[t].level`, `.stats.mercCaptures`, `.stats.structures`, `match.objective`, `match.winner`, `match.length`, `players[].gameStats.{HeroDamage,SiegeDamage,Healing,Deaths,Takedowns/SoloKill,Assists}`, `match.levelAdvTimeline`, `match.XPBreakdown`) ; **ne garder que ce qui existe** (avance d'XP : `XPBreakdown` par équipe si présent, sinon `levelAdvTimeline`) ; noter le mapping retenu dans `docs/spec/03-storm-stats.md` (section lecture) et ajuster `GameSummary`.
- [ ] **Step 2: Tests (rouges)** serveur : `summary_matches_match_sheet_values` (fixture : chaque champ du résumé = valeur lue directement dans `match_`/`players`), `summary_rejects_non_custom` (`NotCustom`), `summary_rejects_failed_status`, `replay_route_garbage_bytes_is_422_not_500`, `replay_route_empty_body_is_400`, `replay_route_writes_nothing_to_matches` (compte `matches`/`uploads` avant/après identique), `replay_route_fills_game_record` ; vitest : `PostGameSlide` n'affiche pas une ligne dont la donnée est absente (`structures` manquant → pas de ligne « FORTS · KEEPS »), onglets dégâts/soins/siège/takedowns.
- [ ] **Step 3: Implémenter** extraction, route, slide (maquette `postgame-breakdown.html`), diaporama (rotation auto `interval_ms`, ou manuel via `POST /api/cast/slide`, ouvrir le breakdown de n'importe quelle partie via `slide.game`). Sans résumé → zone de slides vide.
- [ ] **Step 4:** `cargo test` (avec `DATABASE_URL`) + clippy + `npm test && npm run build` ; vérifier dans le navigateur sur un vrai replay Custom : valeurs concordent avec `/match/:id` d'une partie équivalente si disponible. **Step 5: Commit** `feat(cast): post-game breakdown depuis replay + diaporama`

### Task 10: Draft dans la régie, redirections

**Files:**
- Create: `web/src/cast/DraftWorkspace.tsx` (corps extrait de `pages/Draft.tsx`), `web/src/cast/DraftReplayPanel.tsx` (corps extrait de `pages/DraftReplay.tsx`), `web/src/cast/DraftScene.tsx` (vue overlay extraite de `pages/DraftOverlay.tsx`)
- Modify: `pages/Draft.tsx`, `pages/DraftReplay.tsx`, `pages/DraftOverlay.tsx` (réduits à des enveloppes minces sur les composants extraits), `App.tsx` (`/draft` et `/draft/replay` → `<Navigate to="/cast?scene=draft" replace />` ; `/draft/overlay` conservé), `Cast.tsx`, `CastOverlay.tsx`, `docs/spec/08-frontend.md`

**Interfaces:**
- Consumes : tout l'existant (`useDraft`, `draft*`, `draftReplay*`) **inchangé** ; seule différence : noms d'équipe, pseudos, score, Bo ne sont plus éditables dans ce workspace (affichés, édition = rail de la régie).
- Produces : `DraftWorkspace` (picker central, colonnes Blue/Red, format, map, first pick, Undo/Reset, Partie suivante / Nouvelle série) ; `DraftReplayPanel` (zone de dépôt, file, Suivant/Retour/Reset/Play, fearless) ; scène `draft` de l'overlay = `DraftScene` (même rendu que `/draft/overlay`, skins conservés).

- [ ] **Step 1: Tests (rouges)** : les tests existants (`draftOverlay.test.ts`, `draftReplay.test.ts`, tests Rust draft) **restent verts sans modification** ; nouveau vitest : `DraftWorkspace` en série active n'expose pas de champ d'édition de nom/score (rendu en lecture seule) ; route `/draft` redirige vers `/cast?scene=draft`.
- [ ] **Step 2–4:** extraire sans changer le comportement (déplacer, ne pas réécrire) → `cargo test` (draft inclus) + `npm test && npm run build` ; vérifier dans le navigateur : un draft Standard complet depuis la régie, puis un replay Custom en auto, Fearless sur 2 parties ; l'overlay `/cast/overlay` en scène `draft` reflète chaque clic ; `/draft/overlay` fonctionne toujours. **Step 5: Commit** `refactor(web): draft déplacé dans la régie /cast, redirections de /draft*`

### Task 11: Vérification de bout en bout et STATUS

**Files:** Modify `docs/STATUS.md` ; (aucun code sauf corrections trouvées)

- [ ] **Step 1:** Parcours complet sur le serveur release + Postgres dev : créer une série Bo5 Fearless (pool 10), saisir veto, lineups A→B après la partie 1, charger un replay Custom (post-game), draft live puis replay, scène In-game sur la capture réelle 1920×1080 ; rafraîchir régie et overlay à chaque étape (aucune perte), redémarrer le serveur (état conservé).
- [ ] **Step 2:** Mesurer et consigner : scène → overlay < 500 ms ; `GET /api/cast/lineup` (10 joueurs) < 100 ms ; décodage post-game < 500 ms. Vérifier les 7 critères d'acceptation de la spec un par un, avec la preuve (commande/capture) pour chacun.
- [ ] **Step 3:** `cargo test --workspace` (avec `DATABASE_URL`), `cargo clippy --all-targets -- -D warnings`, `cd web && npm test && npm run build` tous verts.
- [ ] **Step 4:** Mettre à jour `docs/STATUS.md` (en-tête en haut : livré, mesures, limites, ce qui reste, « pas encore déployé sur le box ») ; relire que chaque contrat modifié a sa section `docs/spec/` à jour.
- [ ] **Step 5: Commit** `docs(status): régie de cast livrée`
