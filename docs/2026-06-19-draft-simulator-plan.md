# Simulateur de draft HotS — Plan d'implémentation

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Un simulateur de draft HotS piloté manuellement (3 formats), avec un overlay OBS « Timeline » live, dans storm-codex.

**Architecture:** Moteur de draft en **Rust pur, TDD** (format = liste d'étapes data) ; état autoritatif côté serveur (`Arc<RwLock>` + persistance Postgres) muté par des routes REST et diffusé via le broadcast WS existant ; deux pages React (console `/draft` + overlay `/draft/overlay`).

**Tech Stack:** Rust (axum, sqlx, tokio, serde), Postgres, React/Vite/TS, react-router, @tanstack/react-query.

Spec : `docs/2026-06-19-draft-simulator-design.md`. Mockups : `docs/draft-aesthetics.html`, `docs/draft-control-mockup.html`.

**Conventions repo (vérifiées) :**
- Crate binaire `crates/storm-codex-server` (pas de lib) → tests unitaires **inline** `#[cfg(test)]`, lancés par `cargo test -p storm-codex-server`.
- Routes ajoutées dans le `Router` de `crates/storm-codex-server/src/main.rs`, handlers en `async fn(State(AppState), …)`.
- WS : `state.events.send(serde_json::json!({ "type": "…" }))` (cf. `patch.new` dans main.rs ; `src/ws.rs` relaie tel quel).
- Migrations : `crates/storm-codex-server/migrations/NNNN_nom.sql` (prochaine = `0008`).
- Référentiel héros déjà servi : `GET /api/dim/heroes` → `Record<nom, { universe, role, icon }>` (cf. `web/src/api.ts` `useDimHeroes`).
- Overlays OBS = routes React **hors `<Layout>`** (cf. `web/src/App.tsx`, ex. `now-playing`).

---

## Chunk 1 : Moteur de draft (Rust pur, TDD)

**Responsabilité :** types + machine d'état, sans aucune I/O. Tout le risque métier vit ici et est couvert par des tests.

### Task 1 : Types + module

**Files:**
- Create: `crates/storm-codex-server/src/draft/mod.rs`
- Modify: `crates/storm-codex-server/src/main.rs` (ajouter `mod draft;` après `mod dim;`)

- [ ] **Step 1 : Déclarer le module.** Dans `main.rs`, ajouter `mod draft;` dans la liste des `mod`.

- [ ] **Step 2 : Écrire les types** dans `src/draft/mod.rs` :

```rust
//! Moteur de draft HotS — pur (aucune I/O). Un format = une liste ordonnée d'étapes ;
//! la machine d'état « marche » dessus. Sérialisable pour persistance + WS.
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub const SCHEMA_VERSION: i32 = 1;

// Côté visuel (couleur/position) — INDÉPENDANT de l'ordre de draft.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Side { Blue, Red }
impl Side { pub fn other(self) -> Side { match self { Side::Blue => Side::Red, Side::Red => Side::Blue } } }

// Rôle d'ordre — qui draft en premier. Le réglage `first_pick: Side` mappe First→un côté.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Order { First, Second }

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Action { Ban, Pick }

// Une étape est définie en rôle d'ordre (pas en couleur) : c'est ce qui décorrèle first-pick et side.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct Step { pub order: Order, pub action: Action }

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Format { Standard, Normal, Fearless }

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum DraftError { Unavailable, Complete, NothingToUndo }
```

- [ ] **Step 3 : Compile check.** Run: `cargo build -p storm-codex-server`. Expected: PASS (warnings « unused » tolérés).

- [ ] **Step 4 : Commit.** `git add -A && git commit -m "feat(draft): types du moteur de draft"`

### Task 2 : Séquences de formats (data)

**Files:**
- Create: `crates/storm-codex-server/src/draft/formats.rs`
- Modify: `crates/storm-codex-server/src/draft/mod.rs` (`mod formats; pub use formats::steps_for;`)

> Séquence Standard **confirmée par l'opérateur** (HotS, first-pick = First). Elle est en data : la
> corriger = éditer cette liste. Écrite en **rôle d'ordre** (First/Second), donc indépendante du
> côté : `first_pick: Side` mappe First→un côté au moment du rendu (cf. Task 3).

- [ ] **Step 1 : Test de la séquence** (inline dans `formats.rs`) :

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::draft::{Action, Format, Order, Step};

    fn counts(steps: &[Step]) -> (usize, usize, usize, usize) {
        let bf = steps.iter().filter(|s| s.action==Action::Ban  && s.order==Order::First ).count();
        let bs = steps.iter().filter(|s| s.action==Action::Ban  && s.order==Order::Second).count();
        let pf = steps.iter().filter(|s| s.action==Action::Pick && s.order==Order::First ).count();
        let ps = steps.iter().filter(|s| s.action==Action::Pick && s.order==Order::Second).count();
        (bf, bs, pf, ps)
    }

    #[test]
    fn standard_is_3_bans_5_picks_per_role_len_16() {
        let s = steps_for(Format::Standard);
        assert_eq!(counts(&s), (3, 3, 5, 5));
        assert_eq!(s.len(), 16);
    }

    #[test]
    fn standard_exact_order() {
        use Action::*; use Order::*;
        let s = steps_for(Format::Standard);
        let got: Vec<(Order, Action)> = s.iter().map(|x| (x.order, x.action)).collect();
        assert_eq!(got, vec![
            (First,Ban),(Second,Ban),(First,Ban),(Second,Ban),
            (First,Pick),(Second,Pick),(Second,Pick),(First,Pick),(First,Pick),
            (Second,Ban),(First,Ban),
            (Second,Pick),(Second,Pick),(First,Pick),(First,Pick),(Second,Pick),
        ]);
    }

    #[test]
    fn normal_has_no_bans_10_picks() {
        let s = steps_for(Format::Normal);
        assert_eq!(counts(&s), (0, 0, 5, 5));
        assert_eq!(s.len(), 10);
    }

    #[test]
    fn fearless_sequence_equals_standard() {
        assert_eq!(steps_for(Format::Fearless), steps_for(Format::Standard));
    }
}
```

- [ ] **Step 2 : Run, vérifier l'échec.** Run: `cargo test -p storm-codex-server formats`. Expected: FAIL (`steps_for` introuvable).

- [ ] **Step 3 : Implémenter** `formats.rs` :

```rust
use crate::draft::{Action, Format, Order, Step};
use Action::{Ban, Pick};
use Order::{First, Second};

// Séquence Standard HotS (confirmée). En rôle d'ordre ; First = équipe first-pick.
// 1 ban F · 1 ban S · 1 ban F · 1 ban S · 1 pick F · 2 pick S · 2 pick F ·
// 1 ban S · 1 ban F · 2 pick S · 2 pick F · 1 pick S.
const STANDARD: [(Order, Action); 16] = [
    (First, Ban), (Second, Ban), (First, Ban), (Second, Ban),
    (First, Pick), (Second, Pick), (Second, Pick), (First, Pick), (First, Pick),
    (Second, Ban), (First, Ban),
    (Second, Pick), (Second, Pick), (First, Pick), (First, Pick), (Second, Pick),
];

pub fn steps_for(format: Format) -> Vec<Step> {
    STANDARD.iter()
        .filter(|(_, a)| format != Format::Normal || *a == Pick)
        .map(|&(order, action)| Step { order, action })
        .collect()
}
```

- [ ] **Step 4 : Run, vérifier le succès.** Run: `cargo test -p storm-codex-server formats`. Expected: PASS.

- [ ] **Step 5 : Commit.** `git add -A && git commit -m "feat(draft): séquences de formats (standard/normal/fearless)"`

### Task 3 : Machine d'état (apply / undo / reset / légalité)

**Files:**
- Modify: `crates/storm-codex-server/src/draft/mod.rs`

- [ ] **Step 1 : Écrire les tests** (inline dans `mod.rs`) couvrant : marche complète du Standard ; refus d'un héros déjà pris (`Unavailable`) ; refus quand terminé (`Complete`) ; undo libère le dernier ; reset remet à zéro en gardant la config ; fearless seed rend indispo ; override manuel ; `current_step`/`active_team`/`current_phase`. Exemple représentatif :

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn new_standard() -> DraftState {
        DraftState::new(Format::Standard, Side::Blue, "Sky Temple".into())
    }

    #[test]
    fn walks_full_standard_then_completes() {
        let mut d = new_standard();
        for i in 0..16 { d.apply(&format!("hero{i}")).unwrap(); }
        assert!(d.is_complete());
        assert_eq!(d.apply("late"), Err(DraftError::Complete));
    }

    #[test]
    fn rejects_already_used_hero() {
        let mut d = new_standard();
        d.apply("Muradin").unwrap();
        assert_eq!(d.apply("Muradin"), Err(DraftError::Unavailable));
    }

    #[test]
    fn undo_frees_last_hero() {
        let mut d = new_standard();
        d.apply("Muradin").unwrap();
        d.undo().unwrap();
        assert_eq!(d.cursor, 0);
        d.apply("Muradin").unwrap(); // de nouveau dispo
    }

    #[test]
    fn fearless_seed_blocks_series_heroes() {
        let mut d = DraftState::new(Format::Fearless, Side::Blue, "Sky Temple".into());
        d.seed_series(&["Jaina".into()]);
        assert_eq!(d.apply("Jaina"), Err(DraftError::Unavailable));
    }

    #[test]
    fn manual_override_toggles_availability() {
        let mut d = new_standard();
        d.set_unavailable("Abathur", true);
        assert_eq!(d.apply("Abathur"), Err(DraftError::Unavailable));
        d.set_unavailable("Abathur", false);
        d.apply("Abathur").unwrap();
    }

    #[test]
    fn first_pick_is_independent_of_side() {
        // first_pick = Red : le rôle First est joué par le CÔTÉ rouge (la 1re étape est « côté rouge »).
        let d = DraftState::new(Format::Standard, Side::Red, "Sky Temple".into());
        assert_eq!(d.active_side(), Some(Side::Red));
        let d2 = DraftState::new(Format::Standard, Side::Blue, "Sky Temple".into());
        assert_eq!(d2.active_side(), Some(Side::Blue));
    }

    #[test]
    fn current_phase_len_groups_consecutive_same_order() {
        let mut d = new_standard();
        assert_eq!(d.current_phase_len(), 1); // étape 1 = ban First, isolée
        for i in 0..5 { d.apply(&format!("h{i}")).unwrap(); } // curseur = 5
        assert_eq!(d.current_phase_len(), 2); // étapes 6-7 = 2 picks Second consécutifs
    }
}
```

- [ ] **Step 2 : Run, vérifier l'échec.** Run: `cargo test -p storm-codex-server draft`. Expected: FAIL (`DraftState` introuvable).

- [ ] **Step 3 : Implémenter** `DraftState` dans `mod.rs` :

```rust
mod formats;
pub use formats::steps_for;

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct TeamInfo { pub name: String, pub players: [String; 5] }

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DraftState {
    pub schema_version: i32,
    pub format: Format,
    pub first_pick: Side,                 // quel CÔTÉ joue le rôle First — réglage INDÉPENDANT du nommage
    pub map: String,
    pub blue: TeamInfo,                    // équipe côté bleu (couleur/position fixes)
    pub red: TeamInfo,                     // équipe côté rouge
    pub score: [u8; 2],                   // [blue, red]
    pub bo: u8,
    pub steps: Vec<Step>,                 // en rôle d'ordre (First/Second), indépendant du side
    pub cursor: usize,
    pub assignments: Vec<Option<String>>, // parallèle à steps : héros assigné à l'étape i
    pub manual_unavailable: BTreeSet<String>,
    pub series_bans: BTreeSet<String>,    // fearless : héros pické aux parties précédentes
}

impl DraftState {
    pub fn new(format: Format, first_pick: Side, map: String) -> Self {
        let steps = steps_for(format);
        let n = steps.len();
        Self {
            schema_version: SCHEMA_VERSION, format, first_pick, map,
            blue: TeamInfo { name: "Blue".into(), players: Default::default() },
            red:  TeamInfo { name: "Red".into(),  players: Default::default() },
            score: [0, 0], bo: 5, steps, cursor: 0,
            assignments: vec![None; n],
            manual_unavailable: BTreeSet::new(),
            series_bans: BTreeSet::new(),
        }
    }

    /// Résout un rôle d'ordre en côté visuel selon le réglage `first_pick` (le découplage).
    pub fn side_of(&self, order: Order) -> Side {
        if order == Order::First { self.first_pick } else { self.first_pick.other() }
    }
    pub fn current_step(&self) -> Option<Step> { self.steps.get(self.cursor).copied() }
    pub fn active_side(&self) -> Option<Side> { self.current_step().map(|s| self.side_of(s.order)) }
    pub fn is_complete(&self) -> bool { self.cursor >= self.steps.len() }

    fn is_used(&self, hero: &str) -> bool {
        self.assignments.iter().flatten().any(|h| h == hero)
    }
    pub fn is_unavailable(&self, hero: &str) -> bool {
        self.manual_unavailable.contains(hero) || self.series_bans.contains(hero) || self.is_used(hero)
    }

    pub fn apply(&mut self, hero: &str) -> Result<(), DraftError> {
        if self.is_complete() { return Err(DraftError::Complete); }
        if self.is_unavailable(hero) { return Err(DraftError::Unavailable); }
        self.assignments[self.cursor] = Some(hero.to_string());
        self.cursor += 1;
        Ok(())
    }
    pub fn undo(&mut self) -> Result<(), DraftError> {
        if self.cursor == 0 { return Err(DraftError::NothingToUndo); }
        self.cursor -= 1;
        self.assignments[self.cursor] = None;
        Ok(())
    }
    pub fn reset(&mut self) {
        self.cursor = 0;
        self.assignments = vec![None; self.steps.len()];
    }
    pub fn set_unavailable(&mut self, hero: &str, value: bool) {
        if value { self.manual_unavailable.insert(hero.to_string()); }
        else { self.manual_unavailable.remove(hero); }
    }
    pub fn seed_series(&mut self, heroes: &[String]) {
        self.series_bans = heroes.iter().cloned().collect();
    }
    /// Longueur du bloc d'étapes consécutives du même rôle d'ordre (= même côté) à partir du curseur,
    /// pour le timer **par phase** (le compte à rebours couvre tout le bloc, pas chaque pick).
    pub fn current_phase_len(&self) -> usize {
        let Some(step) = self.current_step() else { return 0 };
        self.steps[self.cursor..].iter().take_while(|s| s.order == step.order).count()
    }
}
```

- [ ] **Step 4 : Run, vérifier le succès.** Run: `cargo test -p storm-codex-server draft`. Expected: PASS. Compléter les tests manquants (phase, reset garde la config) jusqu'au vert.

- [ ] **Step 5 : Clippy.** Run: `cargo clippy -p storm-codex-server -- -D warnings`. Expected: PASS.

- [ ] **Step 6 : Commit.** `git add -A && git commit -m "feat(draft): machine d'état (apply/undo/reset/légalité/fearless)"`

---

## Chunk 2 : État serveur, persistance & API

**Responsabilité :** exposer le moteur via REST + WS, avec un état partagé persistant.

### Task 4 : Migration (persistance draft + série)

**Files:**
- Create: `crates/storm-codex-server/migrations/0008_draft.sql`

- [ ] **Step 1 : Écrire la migration :**

```sql
-- État de draft « live » (singleton : une seule ligne id=1) + historique de série (fearless).
CREATE TABLE draft_live (
    id          INT PRIMARY KEY DEFAULT 1 CHECK (id = 1),
    state       JSONB NOT NULL,
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE TABLE draft_series (
    id          BIGSERIAL PRIMARY KEY,
    format      TEXT NOT NULL,
    bo          INT  NOT NULL,
    team_names  JSONB NOT NULL,
    score       JSONB NOT NULL,
    games       JSONB NOT NULL DEFAULT '[]',  -- [[hero,…] par partie terminée]
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);
```

- [ ] **Step 2 : Vérifier l'application** au démarrage (migrations auto, cf. `main.rs`). Run: `cargo build -p storm-codex-server` puis lancer le serveur en local (ou `sqlx migrate run`). Expected: migration `0008` appliquée sans erreur.

- [ ] **Step 3 : Commit.** `git add -A && git commit -m "feat(draft): migration draft_live + draft_series"`

### Task 5 : État partagé dans AppState + chargement

**Files:**
- Modify: `crates/storm-codex-server/src/main.rs` (champ `draft` dans `AppState`, init dans `run`)
- Create: `crates/storm-codex-server/src/draft/store.rs` (`load`/`save` Postgres ; `mod store;` dans `draft/mod.rs`)

- [ ] **Step 1 :** Ajouter à `AppState` : `pub draft: std::sync::Arc<tokio::sync::RwLock<crate::draft::DraftState>>`.
- [ ] **Step 2 :** Dans `run()`, après les migrations : charger depuis `draft_live` (sinon `DraftState::new(Format::Standard, Team::Blue, "Sky Temple")`) et construire l'`Arc<RwLock<…>>`.
- [ ] **Step 3 :** `store.rs` : `async fn load(db) -> Option<DraftState>` (SELECT state FROM draft_live WHERE id=1) et `async fn save(db, &DraftState)` (INSERT … ON CONFLICT (id) DO UPDATE). Sérialisation via `serde_json`.
- [ ] **Step 4 : Build.** Run: `cargo build -p storm-codex-server`. Expected: PASS.
- [ ] **Step 5 : Commit.** `git add -A && git commit -m "feat(draft): état partagé + persistance (load/save)"`

### Task 6 : Routes REST + broadcast WS

**Files:**
- Create: `crates/storm-codex-server/src/draft/api.rs` (handlers ; `mod api;` dans `draft/mod.rs`)
- Modify: `crates/storm-codex-server/src/main.rs` (enregistrer les routes)

Routes (toutes muent l'état sous write-lock, persistent via `store::save`, puis `state.events.send(json!({"type":"draft.updated"}))`) :
- `GET  /api/draft` → l'état courant (JSON).
- `POST /api/draft/config` `{format, map, first_pick (side "blue"|"red"), blue:{name,players[5]}, red:{name,players[5]}, bo}` → recrée l'état. `first_pick` est **indépendant** du nommage blue/red.
- `POST /api/draft/action` `{hero}` → `apply` ; 409 si `Unavailable`/`Complete`.
- `POST /api/draft/undo`, `POST /api/draft/reset`.
- `POST /api/draft/unavailable` `{hero, value}` → `set_unavailable`.
- `POST /api/draft/score` `{blue, red}`.
- `POST /api/draft/series/next` → push picks de la partie courante dans `draft_series.games`, recrée un draft pré-rempli (`seed_series` = union des games) ; `POST /api/draft/series/new` → vide la série.

- [ ] **Step 1 :** Implémenter `api.rs` (un handler par route ; helper `broadcast_and_save(state)`).
- [ ] **Step 2 :** Enregistrer les routes dans le `Router` de `main.rs` (bloc `.route("/api/draft", …)` etc.).
- [ ] **Step 3 : Build + clippy.** Run: `cargo build -p storm-codex-server && cargo clippy -p storm-codex-server -- -D warnings`. Expected: PASS.
- [ ] **Step 4 : Test manuel** (serveur local) : `curl -s localhost:PORT/api/draft | jq` puis `curl -XPOST …/api/draft/action -d '{"hero":"Muradin"}'` → l'état avance ; un 2e onglet `/ws` reçoit `draft.updated`.
- [ ] **Step 5 : Commit.** `git add -A && git commit -m "feat(draft): routes REST + broadcast WS draft.updated"`

---

## Chunk 3 : Web — couche API + console `/draft`

### Task 7 : Couche API typée

**Files:**
- Modify: `web/src/api.ts` (types `DraftState`, helpers `fetchDraft`, `draftAction`, etc. ; hook `useDraft` qui refetch sur `draft.updated` via le WS existant `useLiveEvents`/équivalent)

- [ ] **Step 1 :** Types miroir du JSON serveur (`Team`, `Action`, `Step`, `DraftState`).
- [ ] **Step 2 :** `useDraft()` : `useQuery(["draft"], fetchDraft)` + invalidation sur event WS `draft.updated` (étendre le handler WS de `api.ts` qui gère déjà `match.parsed`/`patch.new`).
- [ ] **Step 3 :** POST helpers (`draftAction(hero)`, `draftUndo()`, `draftConfig(...)`, …).
- [ ] **Step 4 : Build web.** Run: `cd web && npm run build`. Expected: PASS (tsc strict).
- [ ] **Step 5 : Commit.** `git add -A && git commit -m "feat(draft): couche API web + hook useDraft (live WS)"`

### Task 8 : Page console `/draft`

**Files:**
- Create: `web/src/pages/Draft.tsx`
- Modify: `web/src/App.tsx` (route `<Route path="draft" element={<Draft/>} />` **dans** `<Layout>`)

Suivre fidèlement `docs/draft-control-mockup.html` : barre de config (format/map/first-pick/Bo/score + Undo/Reset/Partie suivante/Nouvelle série), bandeau de phase courante + timer, ruban series bans (fearless), colonnes Blue/Red (nom équipe + 5 pseudos éditables + bans + slot courant surligné), picker central (recherche + onglets rôle depuis `dim_heroes.role` + grille ; indispo grisés ; clic = `draftAction`). Réutiliser `Avatar`, `useDimHeroes`.

- [ ] **Step 1 :** Implémenter la page (lecture `useDraft`, écritures via helpers Task 7).
- [ ] **Step 2 :** Enregistrer la route.
- [ ] **Step 3 : Build web.** Run: `cd web && npm run build`. Expected: PASS.
- [ ] **Step 4 : Vérif preview** : lancer le serveur + front, ouvrir `/draft`, drafter un Standard complet au clic ; vérifier surbrillance d'étape + indispo + undo. (Workflow preview, pas de demande manuelle à l'utilisateur.)
- [ ] **Step 5 : Commit.** `git add -A && git commit -m "feat(draft): console de contrôle /draft"`

---

## Chunk 4 : Web — overlay Timeline `/draft/overlay`

### Task 9 : Skins (variables CSS)

**Files:**
- Create: `web/src/pages/draft-overlay.css` (les 4 skins = blocs `[data-skin=…]` ; copier les tokens de `docs/draft-aesthetics.html`)

- [ ] **Step 1 :** Porter les variables des 4 skins (nexus/glass/tactical/mono) + charger les polices web (Oswald, JetBrains Mono, Cinzel) via `@import`/`<link>`.
- [ ] **Step 2 : Commit.** `git add -A && git commit -m "feat(draft): skins overlay (4 thèmes CSS)"`

### Task 10 : Overlay Timeline

**Files:**
- Create: `web/src/pages/DraftOverlay.tsx`
- Modify: `web/src/App.tsx` (route `<Route path="draft/overlay" element={<DraftOverlay/>} />` **hors** `<Layout>`, comme `now-playing`)

Porter la vue **Timeline** de `docs/draft-aesthetics.html` : en-tête (map, noms + point clignotant sur `active_team`, score, timer de phase), ruban series bans (si fearless), picks en grille chronologique (colonnes = étapes de pick dans l'ordre, bleu haut / rouge bas, portrait + héros + pseudo, tuile active = halo or), bande de bans centrale. Données via `useDraft` (live WS). Skin via `?skin=` (défaut `nexus`). Le timer se réinitialise quand `active_team` change (timer par phase).

- [ ] **Step 1 :** Implémenter l'overlay (présentation pure depuis `DraftState`).
- [ ] **Step 2 :** Enregistrer la route (hors Layout).
- [ ] **Step 3 : Build web.** Run: `cd web && npm run build`. Expected: PASS.
- [ ] **Step 4 : Vérif preview** (1920×1080) : drafter depuis `/draft` dans un onglet ; `/draft/overlay` se met à jour **en direct** ; tester `?skin=glass|tactical|mono`. Screenshot de preuve.
- [ ] **Step 5 : Commit.** `git add -A && git commit -m "feat(draft): overlay Timeline live (4 skins)"`

---

## Critères d'acceptation (rappel spec)

1. Moteur : 3 séquences de bout en bout, refus indispo, undo/reset — tests verts (`cargo test -p storm-codex-server`).
2. `/draft` construit un Standard au clic ; `/draft/overlay` live via WS ; `?skin=` change l'esthétique.
3. Fearless : partie 2 → héros de la partie 1 indisponibles (ruban) ; override manuel OK.
4. Noms d'équipe + pseudos + score éditables et reflétés dans l'overlay.

## Hors périmètre (YAGNI)

Bot/IA, suggestions Jarvis, talents, multi-opérateur, contrainte de temps dure, overlay « vue joueur » dédié.
