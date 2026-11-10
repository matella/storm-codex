# Runbook — Box check of the 2D viewer (minimap alignment)

**Goal:** last step of MVP-1 (milestone Batch 4) — check on the box, on a **real match on a
map WITH a background image** (Cursed Hollow, Dragon Shire, Sky Temple… not an ARAM without an image), that
the heroes are placed at the right spot on the right map, that the scrub is instant, and **nudge the
flip-Y / crop if needed**. The pipeline, the endpoint, the canvas and the 100 % client-side seek are
**already proven locally** (smoke 2026-07-09, see STATUS) — all that remains is the map-relative visual
alignment, which requires a map image + a recognisable match.

## Prerequisites
- Branch `feat/replay2d` merged (or deployed as is) on the box.
- The box is running (in the evening), Postgres backfilled, vendored images (`/images/battlegrounds/*.png`
  present — this is what was missing locally).

## Deployment (rsync + docker compose build — NO git pull on the box)
```sh
# from the Mac, in the repo (⚠️ ALWAYS --exclude .env — see STATUS deployment trap)
rsync -az --delete --exclude .env --exclude target --exclude node_modules --exclude .git \
  ./ matella@192.168.129.85:~/apps/storm-codex/
ssh matella@192.168.129.85 'cd ~/apps/storm-codex && docker compose up -d --build'
```

## Check (browser, via Tailscale)
1. Open `http://192.168.129.85:5102/matches`, pick a recent match **on a map with an image**
   (Cursed Hollow / Dragon Shire / Sky Temple / Braxis Holdout…), not an ARAM.
2. **"Replay 2D"** tab. Expected:
   - The **background = the image of the right map** (no longer the fallback gradient).
   - 10 hero dots with **real portraits** (the vendored images exist on the box), universe colour
     ring, team colour fill.
   - Scrub 0 → end: consistent positions, **instant** (no network round trip per cursor
     move — already proven).
3. **Judge the orientation** by scrubbing to a known moment (e.g. a teamfight near an objective, or
   the death of a hero whose location is known):
   - Are the heroes on the right side (blue base vs red base at the right end of the map)?
   - Is **top/bottom** correct? If everything is **flipped vertically** → toggle the **flip-Y**.

## If the alignment needs fixing (code)
- **Vertical flip**: in `web/src/components/Replay2D.tsx`, the draw loop uses
  `cy = (1 - y) * H` (heroes AND death markers). Switch to `cy = y * H` (in BOTH places:
  dots + `deathsNear`). A single, consistent flip.
- **Horizontal flip** (rare): same on `cx` (`cx = x*W` ↔ `(1-x)*W`).
- **Unplayable border / heroes squashed at the edge**: `MapSize` may include an unplayable margin. Add
  a per-map crop constant (margin fraction) applied to the normalisation — either in the
  crate (`storm-replay-viewer`, in the normalisation), or in the front-end at draw time. **Do not
  over-fit**: aim for "clearly readable", not pixel-perfect. Document the value retained.
- After any front-end change: `cd web && npm run build` (check `✓ built`), redeploy.

## MVP-1 acceptance criterion (to tick)
- [ ] Right map image in the background.
- [ ] 10 heroes correctly placed (right side, right orientation) on a recognised match.
- [ ] Instant scrub, alive/dead dimming + death markers at the right spot.
- [ ] Real portraits shown.
→ Once ticked: update `docs/STATUS.md` ("2D viewer MVP-1: delivered + checked on the box"),
then finalise the branch (merge / PR).

## Reminders
- HP/mana **permanently out of scope** (data absent from the replay) → alive/dead instead.
- Fast-follows (outside MVP-1): play/pause animation, alive/destroyed structures, clickable kill feed,
  talent/level indicators, per-map objectives.
