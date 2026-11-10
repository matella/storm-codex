# Spec — In-game minimap backgrounds for the 2D viewer

**Date:** 2026-07-09 · **Status:** design approved (brainstorm), ready for a plan.
**Goal:** replace the 2D viewer's background (today the **painted loading-screen art**,
visually busy) with the **real in-game tactical minimap** (schematic top-down view) — for
**much better readability** AND **more precise calibration** of hero positions.

## Motivation / double gain
Today `mapImage()` serves `/images/battlegrounds/<slug>.png` = the painted loading-screen art
(busy texture, veiled by a gradient). Two problems:
1. **Readability**: hero dots read poorly on the painted art (e.g. the busy purple of Cursed
   Hollow).
2. **Calibration**: positions are normalised by `MapSize` (= **playable** area) and then stretched
   over the painted art, whose **decorative border does not match** the playable area → slight offset.

The **in-game minimap matches the playable area / camera bounds ~1:1** → cleaner to read **and**
better aligned. Two gains at once.

## Sourcing findings (spike done, 2026-07-09)
Clean minimaps are **NOT publicly downloadable**. HeroesDataParser (the `heroes-images`
ecosystem, the one HotsPatchNotes uses) only publishes `loadingscreens` and `replaypreviews` — both
are **painted art** (checked: `replayspreviewimage_cursedhollow.png` = the purple scene, not a
minimap). The `jamiephan/HeroesOfTheStorm_Gamedata` repo (named in MVP-1 as a potential source)
contains **only XML/data, no images** — also ruled out as an image source (but its
`CameraBounds` XML remain useful for calibration, see Batch 3). The real minimap textures are
`.dds`/`.tga` **in the game files** (`base.stormassets/Assets/Textures/`, and/or `Minimap.tga`
baked into each map archive). → **They must be extracted from the HotS installation** (on the gaming PC).

## Locked decisions
1. **Background = real in-game minimap** (full schematic, not just an outline; we do not draw our
   own diagram).
2. **Source = extraction from the game files** (PC), no wiki scraping, no dependency on a
   public download.
3. **Distribution = baked/vendored assets** (like the reference data: "distribute the produced data,
   the user never scrapes, baked fallback"). Fallback: current painted art if there is no minimap.
4. **Target maps**: the ~20 standard battlegrounds played (+ ARAM if a minimap is available). Priority to the
   maps actually present in the box's archive.

## Batch 1 — Extraction (gaming PC, operator, guided by the agent)
- **Tool**: **CascView** (zezula.net) — opens the HotS CASC read-only, browse/extract by path.
- **Candidate targets** (to be confirmed on the 1st map): `Minimap.tga` baked into each map's
  archive (SC2/HotS convention), **and/or** a dedicated minimap texture under
  `base.stormassets/Assets/Textures/`.
- **Procedure**: **confirm on Cursed Hollow first** (extract the candidate texture, check by eye
  that it is indeed the clean top-down wanted), **then batch** on the ~20 battlegrounds. Convert
  TGA/DDS → **PNG**.
- **Output naming**: `<slug>.png` where `slug` = **same convention as `mapImage()`** (lowercase, apostrophes
  removed, spaces→hyphens; e.g. `cursed-hollow.png`, `braxis-holdout.png`).
- **Delivery = COMMIT to the repo** (locked decision, not an ad hoc folder on the box). The extracted minimaps
  have **no runtime re-fetch source** (unlike the art vendored from
  HotsPatchNotes): leaving them in an uncommitted folder on the box would expose them to silent loss
  (rsync/rebuild — see the `.env` incident in STATUS). → The PNGs are **committed to the repo** (e.g.
  `assets/minimaps/<slug>.png`) and **baked into the image** (see Batch 2). The operator transfers them from the PC
  to the repo (scp/share); the agent commits them.
- **Per-map fallback, without blocking the batch**: if a map's `Minimap.tga` is too flat/unusable,
  that map falls back to its painted art (fallback) — one map's failure **does not invalidate the batch**. Optional
  case-by-case fallback: dedicated texture or wiki version for that specific map (not systematic).
- **Detailed runbook** to be written in `docs/runbooks/` (exact CascView steps + conversion + slugs).

## Batch 2 — Vendoring + serving (agent, box)
- The committed minimaps (`assets/minimaps/<slug>.png`) are **baked into the image** (COPY at build) and
  **copied into `IMAGES_DIR/minimaps` at startup** (idempotent, like a baked fallback) — so they
  survive redeploys (rsync brings the repo, the image contains them, startup puts them into the volume),
  **no runtime fetch** (consistent with the suite design). Served on `/images/minimaps/<slug>.png` (ServeDir on
  `IMAGES_DIR`, no new route).
- **No change to the HotsPatchNotes pipeline** (it keeps providing the painted art = fallback).

## Batch 3 — Front-end swap + calibration + E2E check (agent)
- **Front-end**: new `minimapImage(map)` → `/images/minimaps/<slug>.png`; `Replay2D.tsx` uses it as the
  **preferred** background, fallback chain **minimap → painted art (`mapImage`) → gradient**. NB: the current
  fallback only has a `mapBroken` boolean (1 level) → plan **two independent `onError` states**
  (broken minimap → try the painted art; broken painted art → gradient), not just a one-line tweak. **Lighter**
  veil (the minimap is already dark/schematic) — just a slight darkening so the
  dots stand out.
- **Calibration**: the minimap ≈ camera bounds. Re-check the `MapSize`-normalised transform **per
  map**; add the **per-map crop/scale table** deferred in MVP-1 (small
  `crop`/`scale` constants where the minimap has a margin). Use the gamedata's `CameraBounds` if it helps. The
  **Y flip is already correct** (validated on Cursed Hollow) — leave it alone.
- **E2E check**: as in MVP-1 — local server + real images + replays with images (Cursed Hollow +
  1–2 others), browser preview: heroes correctly placed on the clean minimap, readability much
  better, tight calibration. Screenshots.

## Coordinate model (reminder)
The crate/endpoint is **unchanged** — it already emits coordinates normalised to `[0,1]` by `MapSize`. All the
minimap calibration is **front-end side** (choice of image + optional per-map `crop`/`scale` when drawing).
No `VIEWER_VERSION` bump, no payload change.

## Out of scope (YAGNI)
- Drawing our own diagram (option ruled out).
- Scraping wikis automatically (manual case-by-case fallback only).
- Touching the HotsPatchNotes / HeroesDataParser pipeline.
- Animated minimaps / baked objectives (we keep our own overlays).
- Automated extraction from the Mac (no access to the PC's CASC; it is a guided PC task).

## Risks / unknowns
- **Quality/format of the extracted texture**: confirmed in Batch 1 on Cursed Hollow before the batch.
- **Coverage**: a map without a clean minimap falls back to the painted art (silent fallback).
- **Minimap margin vs `MapSize`**: handled by the per-map crop table (Batch 3), tuned by eye
  on real matches (aim for "clearly better", not pixel-perfect).
- **DDS→PNG conversion**: CascView exports, otherwise a converter (ImageMagick/texconv) — specified in the runbook.

## Breakdown
1. **Guided extraction (PC/CascView)** → PNGs of the ~20 battlegrounds (confirm Cursed Hollow → batch).
2. **Vendoring + serving** `/images/minimaps/`.
3. **Front-end swap + calibration + E2E check**, then STATUS + (optional) box redeployment.
