# Visionneuse 2D — Fast-follows (Phases 2–5) — Plan d'implémentation

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (fresh subagent per lot + two-stage review). Steps use checkbox (`- [ ]`).

**Goal:** Compléter la visionneuse 2D au-delà du MVP-1 : lecture animée, structures, kill-feed, indicateurs de sorts/talents/niveaux, objectifs par carte, toggle minions, export de clip — tout ce que couvre la spec au-delà de la V1.

**Base :** MVP-1 livré + déployé (crate `storm-replay-viewer`, endpoint `/api/matches/{id}/replay2d`, onglet front `web/src/components/Replay2D.tsx` + `web/src/replay2d.ts`). Ces lots **étendent** ces trois unités selon les mêmes patterns.

**Spec :** `docs/specs/2026-07-09-visionneuse-2d-replay-design.md` (Phases 2–5, US-11, US-16→US-27).

**Vérités terrain (mesurées sur un vrai replay — cf. recon 2026-07-09) :**
- Structures : born `KingsCore`(core), `TownTownHallL{2,3}`(fort/keep), `TownCannonTowerL{2,3}`(tour), `TownWallRadial*`(mur), `TownGate*`(porte), `m_controlPlayerId` **11=team0 / 12=team1** ; destruction = `SUnitDiedEvent` du tag ; `SStatGameEvent{TownStructureInit}`.
- Talents : `SHeroTalentTreeSelectedEvent {_userid.m_userId, m_index, _gameloop}` (pick, l'ordre = tier) ; `SStatGameEvent{TalentChosen}` et `{LevelUp}` (horodatés). Noms via `dim_talents` (peuplé sur le box).
- Sorts : `SCmdEvent` avec `m_abil` non-null (`m_abilLink`), clé `_userid.m_userId`.
- Kill-feed : `deaths` (déjà extraits) + `SStatGameEvent{PlayerDeath, JungleCampCapture}` + morts de structures.
- Minions/camps : `SUnitPositionsEvent` porte aussi les unités non-héros (damage-gated, éparses).

**⚠️ Branche :** `git switch -c feat/replay2d-fastfollows` (pas sur `main`). Chaque lot commit + review avant le suivant.

**Rappel bump `VIEWER_VERSION`** : dès qu'un lot change le **schéma JSON** du modèle (nouveaux champs), incrémenter `VIEWER_VERSION` dans `crates/storm-replay-viewer/src/model.rs` (invalide le cache disque `{id}-replay2d-v{N}.json`) et régénérer le golden (`UPDATE_GOLDEN=1`).

**Ordre de build = valeur/effort décroissant, chaque lot livrable indépendamment.**

---

## Chunk 1 — Lecture animée (Phase 2, US-11) · FRONT SEUL

**Responsabilité :** play/pause + vitesse 0,5×–8× pilotant le `t` existant. Aucune donnée serveur.

**Fichiers :** Créer `web/src/usePlayback.ts` (hook pur testable) ; Modifier `web/src/components/Replay2D.tsx` (contrôles) ; Test `web/src/usePlayback.test.ts`.

- [ ] **Step 1 — test du hook (échoue)** : `usePlayback` expose `{playing, speed, toggle, setSpeed}` et, via un « tick » injectable, avance `t` de `speed × dt` en secondes, s'arrête à `durationSec` (clamp + auto-pause). Écrire un test unitaire de la **fonction pure d'avancement** `advance(t, dt, speed, duration) → {t, playing}` (pas le hook React) : `advance(10, 1, 2, 100) → {t:12}` ; `advance(99.5, 1, 8, 100) → {t:100, playing:false}` (clamp + pause en fin).
- [ ] **Step 2** : `npm run test` → FAIL.
- [ ] **Step 3 — implémenter** `web/src/usePlayback.ts` : `advance()` pure + un hook `usePlayback(duration, {onTick})` qui utilise `requestAnimationFrame` (delta réel via un timestamp passé en argument au callback pour rester testable ; le hook mesure le delta lui-même via `performance.now`). NB : `performance.now` est autorisé côté navigateur (ce n'est pas le sandbox Rust). Le hook ne détient PAS `t` : il appelle `onTick(dt)` et le composant applique `advance`.
- [ ] **Step 4** : `npm run test` → PASS.
- [ ] **Step 5 — brancher dans Replay2D.tsx** : bouton ▶/⏸, sélecteur de vitesse (0.5, 1, 2, 4, 8), câblés au scrub existant (`setT(t => advance(...).t)`). Le scrub manuel met en pause. `aria-label` sur les contrôles.
- [ ] **Step 6** : `npm run build` → `✓ built`. Commit `feat(web): lecture animée play/pause + vitesse (US-11)`.

---

## Chunk 2 — Structures (Phase 3, US-16) · CRATE + FRONT

**Responsabilité :** état des structures (core/fort/keep/tour/mur/porte) vivant→détruit au scrub.

**Fichiers :** Modifier `crates/storm-replay-viewer/src/{model.rs,extract.rs}` ; Modifier `web/src/replay2d.ts` (type) + `web/src/components/Replay2D.tsx` (rendu) ; Test crate `tests/extract.rs`.

- [ ] **Step 1 — modèle** : ajouter à `ViewerModel` un champ `structures: Vec<Structure>` où
  `Structure { team: i64, kind: String, x: f64, y: f64, destroyed_at: Option<f64> }` (camelCase, `destroyedAt`). **Bump `VIEWER_VERSION` → 2.**
- [ ] **Step 2 — test crate (échoue)** : `structures_present_and_classified` : ≥1 structure `kind=="core"` par équipe (0 et 1), toutes coords ∈ [0,1], `destroyed_at` soit `None` soit ∈ [0, durationSec]. (Sur silver-city ARAM il y a 2 `KingsCore`.)
- [ ] **Step 3** : `cargo test -p storm-replay-viewer` → FAIL.
- [ ] **Step 4 — extraction** : dans `build`, parcourir `SUnitBornEvent` où `m_controlPlayerId ∈ {11,12}` **et** `m_unitTypeName` matche un préfixe structure. Classifier `kind` :
  ```
  KingsCore|*Core*        → "core"
  TownTownHall*           → "fort"      // (fort/keep : garder "fort" en V1, raffiner plus tard)
  TownCannonTower*|*Tower*→ "tower"
  TownWall*               → "wall"
  TownGate*               → "gate"
  TownMoonwell*           → "well"
  sinon                   → "other" (ignorer other pour le rendu si trop bruyant)
  ```
  `team = if m_controlPlayerId==11 {0} else {1}`. Position `q3(norm_tile(m_x,m_y))`. `destroyed_at` : chercher un `SUnitDiedEvent` du même `(m_unitTagIndex,m_unitTagRecycle)` → `loop_to_sec(t)`, sinon `None`. Trier par (team, kind). Ignorer les unités éphémères (`WitchDoctorZombieWallUnit`, `*BunkerDrop*`, summons) — restreindre au préfixe `Town`/`KingsCore`/`HallOfStorms`.
- [ ] **Step 5** : régénérer golden (`UPDATE_GOLDEN=1`), `cargo test -p storm-replay-viewer` + `--workspace` PASS, clippy clean.
- [ ] **Step 6 — front** : type `Structure` dans `replay2d.ts` ; dans le draw loop, dessiner chaque structure à `(x,(1-y))` : icône/losange selon `kind` (couleur d'équipe), **grisé + ✕** si `destroyedAt != null && t >= destroyedAt`. Core = plus gros. Dessiner SOUS les héros (structures d'abord).
- [ ] **Step 7** : `npm run build` ✓. Commit `feat(viewer): structures alive/destroyed (US-16)` (crate+front).

---

## Chunk 3 — Kill-feed cliquable (Phase 3, US-20) · CRATE + FRONT

**Responsabilité :** journal d'événements (takedowns, morts de structures, captures de camp) à côté de la carte ; clic → seek.

**Fichiers :** Modifier `crates/storm-replay-viewer/src/{model.rs,extract.rs}` ; `web/src/replay2d.ts` + `Replay2D.tsx` ; test crate.

- [ ] **Step 1 — modèle** : `events: Vec<FeedEvent>` où `FeedEvent { t: f64, kind: String, team: Option<i64>, text: String }`. `kind ∈ {"takedown","structure","camp","objective"}`. **Bump `VIEWER_VERSION` → 3.**
- [ ] **Step 2 — test (échoue)** : `feed_events_sorted_nonempty` : `events` non vide, triés par `t` croissant, chaque `t ∈ [0,durationSec]`, au moins un `kind=="takedown"`.
- [ ] **Step 3** : FAIL.
- [ ] **Step 4 — extraction** : takedowns = depuis `deaths` (texte « <victimHero?> slain » — mais le crate n'a pas les noms héros ; garder `text` générique côté crate : `"Player {victim} killed by {killer}"` avec playerId, le FRONT réécrit avec les vrais noms via `players[]`). Alternative plus propre : le crate émet `events` **sans texte lisible** mais avec des champs structurés (`victimPlayerId`, etc.) et le FRONT compose le libellé. → Choisir : `FeedEvent { t, kind, team, victimPlayerId: Option, killerPlayerId: Option, structureKind: Option }` et le front génère le texte. Structure deaths = depuis `structures` (destroyedAt) → event `structure`. Camps = `SStatGameEvent{JungleCampCapture}` (a `m_intData`/`m_stringData` pour le camp + équipe). Trier par t.
- [ ] **Step 5** : golden + tests + clippy PASS.
- [ ] **Step 6 — front** : panneau liste à droite (sous la légende ou onglet) : chaque event = icône (💀/🏰/🏕️) + libellé (noms via `players[]`/`heroIcon`) + timecode `mm:ss`, couleur d'équipe, **clic → setT(event.t)** + pause. Surligner l'event le plus proche de `t`. Scroll auto vers l'event courant.
- [ ] **Step 7** : `npm run build` ✓. Commit `feat(viewer): clickable kill-feed event log (US-20)`.

---

## Chunk 4 — Indicateurs de sorts (Phase 3, US-18) · CRATE + FRONT

**Responsabilité :** flash sur un héros quand il lance un sort.

**Fichiers :** crate `{model,extract}` ; `replay2d.ts` + `Replay2D.tsx` ; test crate.

- [ ] **Step 1 — modèle** : ajouter aux `HeroTrack` un champ `casts: Vec<f64>` (instants de lancement, triés). **Bump `VIEWER_VERSION` → 4.**
- [ ] **Step 2 — test (échoue)** : `casts_present` : somme des `casts` sur tous les héros > 100 ; chaque instant ∈ [0,durationSec] ; triés.
- [ ] **Step 3** : FAIL.
- [ ] **Step 4 — extraction** : dans le passage `visit_game_events`, pour un `SCmdEvent` avec `m_abil` non-null, `t=loop_to_sec(_gameloop)`, `p=user_to_player[_userid.m_userId]` → pousser `t` dans `casts[p]`. **Dédup temporel** : ignorer un cast à moins de ~0.3 s du précédent du même joueur (spam de re-cast). Ne PAS tenter d'identifier quel sort (hors périmètre — juste « a lancé un sort »). Trier.
- [ ] **Step 5** : golden + tests + clippy.
- [ ] **Step 6 — front** : helper pur `castFlash(casts, t, window=0.6) → 0..1` (intensité du flash le plus proche, 0 si aucun) + test. Dans le draw loop, si intensité>0, dessiner un anneau lumineux pulsé autour de la pastille. Utile en lecture animée.
- [ ] **Step 7** : `npm run build` ✓. Commit `feat(viewer): ability-cast flash indicators (US-18)`.

---

## Chunk 5 — Talents/niveaux + bande talents (Phase 3 US-19 + Phase 5 US-27) · CRATE + FRONT

**Responsabilité :** marqueur de pick de talent par héros + **bande de tiers** sous le scrub.

**Fichiers :** crate `{model,extract}` ; `replay2d.ts` + `Replay2D.tsx` (+ éventuel `web/src/components/TalentStrip2D.tsx`) ; test crate.

- [ ] **Step 1 — modèle** : `HeroTrack.talents: Vec<TalentPick>` où `TalentPick { t: f64, tier: i64, talent_id: Option<String> }` ; `levels: Vec<LevelTick>` (au niveau `ViewerModel`) où `LevelTick { t, team, level }` (ARAM = XP partagé par équipe). **Bump `VIEWER_VERSION` → 5.**
- [ ] **Step 2 — test (échoue)** : `talents_and_levels` : au moins un héros a des `talents` non vides, `tier` croissant par héros ; `levels` non vide, `level` croissant par équipe.
- [ ] **Step 3** : FAIL.
- [ ] **Step 4 — extraction** : talents depuis `SHeroTalentTreeSelectedEvent` (ordre des picks par joueur = tier 1,2,3… → mapper aux niveaux 1,4,7,10,13,16,20) ; `talent_id` : le crate ne connaît pas le référentiel → laisser `talent_id=None` en V1 (le FRONT résout via `dim_talents` + le build `match_players.talents` déjà stocké). **Ou** capturer `SStatGameEvent{TalentChosen}` qui porte l'id de talent dans `m_stringData` → `talent_id`. Niveaux : `SStatGameEvent{LevelUp}` (équipe via `m_intData`/player→team) → `levels`.
- [ ] **Step 5** : golden + tests + clippy.
- [ ] **Step 6 — front marqueur** : sur la pastille, petit badge « ⬆ » ou halo quand `talents[i].t` proche de `t` (≤ 3 s). Résoudre le nom via `talentInfo()`/`dim_talents` (déjà côté front) en tooltip.
- [ ] **Step 7 — bande talents (US-27)** : sous le scrub, une grille 10 lignes (héros) × tiers ; case remplie quand le tier est pris à `t` (couleur d'équipe), vide sinon ; hover = nom du talent. Niveau d'équipe affiché.
- [ ] **Step 8** : `npm run build` ✓. Commit `feat(viewer): talent/level indicators + talent timeline strip (US-19/US-27)`.

---

## Chunk 6 — Objectifs par carte (Phase 4, US-21→US-24) · CRATE (module par carte) + FRONT

**Responsabilité :** cadre par-carte isolé (US-24) ; renseigner les objectifs traçables, **signaler « données indisponibles »** pour les cartes à trou connu (US-7/US-22/US-23) ; Braxis best-effort (US-21).

**Fichiers :** Créer `crates/storm-replay-viewer/src/maps/mod.rs` (+ `braxis.rs`, `generic.rs`) ; Modifier `extract.rs` (appel), `model.rs` ; `replay2d.ts` + `Replay2D.tsx` ; test crate.

- [ ] **Step 1 — modèle** : `objectives: Vec<Objective>` où `Objective { t: f64, kind: String, team: Option<i64>, text: String }` (événements d'objectif horodatés) + réutiliser `warnings: Vec<String>` (déjà présent, US-7). **Bump `VIEWER_VERSION` → 6.**
- [ ] **Step 2 — module par carte (US-24)** : `maps::objectives(map_name, &tracker) -> (Vec<Objective>, Vec<String> warnings)`. Chaque carte = un handler ; défaut `generic` (rien / événements d'objectif génériques s'il y en a). Cartes à trou → `warnings.push("objective data unavailable: <map>")` (Blackheart's Bay, Volskaya Foundry).
- [ ] **Step 3 — test (échoue)** : `objectives_framework` : pour un replay Braxis (ou à défaut, silver-city → `objectives` vide + pas de warning erroné), la fonction ne panique pas et respecte l'invariant (t ∈ [0,dur]). Pour une carte à trou (monter un cas), un warning est présent. (Utiliser les replays dispo ; si Braxis absent du corpus local, tester au moins le routage `generic` + le warning sur un nom de carte-trou simulé via un test unitaire de `maps::objectives`.)
- [ ] **Step 4** : FAIL.
- [ ] **Step 5 — implémenter** : `braxis.rs` (US-21) : inférer la force/timing des vagues depuis les morts d'`Ultralisk`/unités zerg (`SUnitDiedEvent` unit types zerg) → `Objective{kind:"zerg_wave"}`. `generic.rs` : mapper les `SStatGameEvent` d'objectif communs si présents. Cartes-trou → warnings. Brancher dans `build`.
- [ ] **Step 6** : golden + tests + clippy.
- [ ] **Step 7 — front** : afficher les `objectives` dans le kill-feed (kind `objective`) + un bandeau « objective data unavailable » quand `warnings` non vide.
- [ ] **Step 8** : `npm run build` ✓. Commit `feat(viewer): per-map objectives framework + gap warnings + Braxis waves (US-21..24)`.

---

## Chunk 7 — Toggle minions/camps (Phase 5, US-26) · CRATE + FRONT

**Responsabilité :** afficher/masquer les unités non-héros (best-effort, éparses).

**Fichiers :** crate `{model,extract}` ; `replay2d.ts` + `Replay2D.tsx` ; test crate.

- [ ] **Step 1 — modèle** : `minions: Vec<MinionSample>` où `MinionSample { t, x, y, team }` (positions non-héros damage-gated). **Bump `VIEWER_VERSION` → 7.** ⚠️ Volume : quantifier + dédup agressif (grille), plafonner (log si tronqué — pas de cap silencieux).
- [ ] **Step 2 — test (échoue)** : `minions_bounded` : coords ∈ [0,1] ; nombre borné (< 20000) ; team ∈ {0,1} ou -1 (neutre).
- [ ] **Step 3** : FAIL.
- [ ] **Step 4 — extraction** : depuis `SUnitPositionsEvent`, pour les tags **non-héros** connus (via `unit_player` : unité non `isHero`, ou owner 11/12), pousser des `MinionSample` (team depuis owner : 11/12→0/1, autre→-1). Dédup par cellule de grille + temps. Trier par t.
- [ ] **Step 5** : golden + tests + clippy.
- [ ] **Step 6 — front** : case à cocher « minions/camps » (défaut OFF) ; si ON, dessiner de petits points ternes (couleur d'équipe atténuée) à `sampleAt`-like nearest (pas d'interpolation — trop épars, afficher le sample le plus proche dans une fenêtre ~5 s).
- [ ] **Step 7** : `npm run build` ✓. Commit `feat(viewer): minion/camp visibility toggle (US-26)`.

---

## Chunk 8 — Export de clip (Phase 5, US-25) · FRONT SEUL

**Responsabilité :** exporter un extrait [start,end] du canvas en fichier vidéo.

**Fichiers :** Créer `web/src/clipExport.ts` ; Modifier `Replay2D.tsx` (UI) ; test unitaire de la logique de bornes.

- [ ] **Step 1 — test (échoue)** : fonction pure `clipFrames(start, end, fps) → number` (nb de frames) + validation bornes (`start<end`, clamp à durée). `clipFrames(10,15,30)=150`.
- [ ] **Step 2** : FAIL.
- [ ] **Step 3 — implémenter** `clipExport.ts` : `clipFrames()` + `recordClip(canvas, {start, end, fps, speed, onFrame})` utilisant `canvas.captureStream(fps)` + `MediaRecorder` (webm/vp9, fallback vp8) ; boucle interne qui pilote `onFrame(t)` de start→end à `1000/fps` ms et `stop()` à la fin → `Blob` → `URL.createObjectURL` → download `match-<id>-<start>-<end>.webm`. NB : MediaRecorder est une API navigateur standard.
- [ ] **Step 4** : `npm run test` PASS (la partie pure ; l'enregistrement réel = vérif E2E).
- [ ] **Step 5 — UI** : deux poignées « clip start/end » (ou boutons « set in/out » sur le scrub) + bouton « Export clip » → `recordClip` en pilotant le rendu du canvas existant. Désactivé si `start>=end`. Indicateur d'enregistrement.
- [ ] **Step 6** : `npm run build` ✓. Commit `feat(web): clip export to webm (US-25)`.

---

## Chunk 9 — Vérif E2E globale + STATUS

- [ ] **Step 1** : `cargo test --workspace` + `cd web && npm run test` + `npm run build` verts.
- [ ] **Step 2 — smoke local** (comme MVP-1) : PG dev + serveur + upload d'un replay imagé (Cursed Hollow tiré du box en read-only si besoin) ; via preview : lecture animée, structures qui tombent, kill-feed cliquable, flashes de sorts, bande de talents, toggle minions, export d'un court clip. Screenshots.
- [ ] **Step 3 — STATUS** : mettre à jour `docs/STATUS.md` (Phases 2–5 livrées + vérifiées ; ce qui reste éventuellement). Commit.
- [ ] **Step 4** : `superpowers:finishing-a-development-branch` (merge/PR) puis (option opérateur) redéploiement box.

---

## Notes transverses
- **Schéma évolutif** : chaque lot qui ajoute un champ bump `VIEWER_VERSION` + régénère le golden. Le front ignore les champs qu'il ne connaît pas encore (rétro-compat).
- **Budget payload** : structures/events/talents/casts sont légers ; **minions** est le seul risque → dédup grille + plafond + `log`. Objectifs légers.
- **Réutilisation** : noms héros/talents via `players[]` + `dim_*` (front) — le crate reste pur géométrie/temps, sans référentiel.
- **Perf seek** : tout reste 100 % client ; les nouvelles couches sont des lookups `t` (binaire/fenêtre). Pas de round-trip.
- **YAGNI** : pas d'identification du sort précis (US-18 = flash générique) ; fort/keep non distingués finement en V1 ; objectifs = traçables + « unavailable » sinon (pas de sur-inférence).
