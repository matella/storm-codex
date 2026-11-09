# "Dossier" design for the whole app

> storm-codex spec. Operator request of 2026-09-27: "the scouting style is really good,
> can we do something similar for the whole app?". OBS overlays: "mockup to
> see, but very probably out of scope".
> **Approved by the operator on 2026-09-27** ("those look hella good, you can also do the entire
> website"). Overlays: out of scope (no request to the contrary). Order of batches: the spec's.
> Mockups: `2026-09-27-design-dossier-app-mockup.html` (Session, Match detail, Hero page) and
> `2026-09-27-design-dossier-overlays-mockup.html` (OBS exploration).

## Context and decision to make

`01-architecture.md` locks "Nexus Codex design". This spec **does not replace it**: it
proposes an **evolution** — "Nexus Codex · Dossier" — that keeps the palette and tokens of the
2026-06-12 mockup (background `#06070b`, accent `#7f77dd`, win `#5dcaa5`, loss `#e24b4a`,
team and universe colours) and adds the editorial layer born with the scouting reports
(HTML export, `/scouting` pages). Reopening a locked decision is up to the operator: approving
this spec counts as the decision.

## The visual language

| Element | Rule |
|---|---|
| Typography | **Big Shoulders Display** (condensed display type) for page titles, hero/map names and large numbers; **Figtree** for text; **JetBrains Mono** for labels, metadata and table numbers |
| Cover | each detail page opens on a "cover" panel: kicker (context), very large title, stat tiles, map art as background (minimap blurred or cut at an angle) |
| Sections | numbered headings (`01 DRAFT`), rule, subtitle on the right in mono |
| Portraits | circle with a ring in the universe colour; tones **pick** (green halo), **enemy** (red halo), **ban** (grey struck through in red), **loss** (red ring) |
| Results | W/L pips, "VICTORY / DEFEAT" verdict in display type, `k–n` records in large numbers |
| Bars | thin gauges (accent → light accent; reds for the opposing team) |
| Motion | staggered appearance of sections, respects `prefers-reduced-motion` |

**Density rule**: the condensed display type **never** goes into table rows. Dense pages
(Matches, Heroes, Trends) keep readable tables (numbers in mono, clean rows);
the new language **frames** them (cover, headers, filters), it does not replace them.

## The three mockups (real data from the archive)

- **Session** — "Tonight" cover: the evening's record very large (4–0 on 2026-06-09),
  pips, latest game (halo portrait, MVP, link to the match), tiles (winrate, K/D/A, MVP,
  heroes). "Tonight's games": one card per game (map banner, mode, hero, K/D/A, award;
  striped pattern for ARAM maps without a minimap). "Your form": last 30 results, overall winrate,
  last 10, winrate per mode, most-played heroes.
- **Match detail** (2368, Braxis Holdout, Storm League) — cover with cropped minimap, verdict,
  takedowns / level / first objective / first fort score. Draft: two panels (struck-through bans
  with phase, picks in order, "FIRST PICK", "YOU"). Scoreboard: bars relative to the game's max,
  awards as labels, the player's row highlighted. Level advantage: curve
  above/below the line + first fort. The player's build per tier.
- **Hero page** (Gul'dan) — large portrait with universe ring, stats, best map (≥ 10
  games). Maps as coloured minimap tiles (≥ 60 % green, < 45 % red). "Builds that win":
  talent paths per tier with winrate. Patch history with BUFF / NERF /
  BUGFIX labels.

The repository's mockups load portraits from `../../.images/` (reference data vendored
locally, not versioned) and minimaps from `../../assets/minimaps/`; without portraits, they fall back
to initials medallions.

## Technical architecture

1. **Self-hosted fonts**: the three families (SIL OFL licence) served from the bundle
   (`web/public/fonts/*.woff2`, Latin subset), no more Google Fonts — the app runs on a LAN
   and in OBS. Budget: ≤ 250 KB of fonts.
2. **Tokens**: `theme.css` gains typography and tone tokens; the existing colour tokens
   do not change.
3. **Shared components** `web/src/components/ds/`: `Portrait` (tones), `Cover`, `SectionHead`,
   `StatTile`, `Pips`, `Chips`, `MapHeader`, `Bar`, `Verdict`, `EvidenceLine`. The Scouting pages
   migrate from their local copies (`Dossier.tsx`, `scouting-dossier.css` scoped `.sd`) to these
   components — a single source.
4. **Chrome**: redesigned top bar (brand in display type, tabs in mono uppercase,
   live indicator) — see mockups.
5. The scouting HTML export stays self-contained (it does not use the bundle) and is not modified.

## Rollout in batches (each deliverable and verifiable on its own)

| Batch | Contents |
|---|---|
| 0 — foundations | self-hosted fonts, tokens, `ds/` components, chrome, migration of the Scouting pages |
| 1 — core | Session (Dashboard), Matches (frame + filters, dense table), Match detail |
| 2 — reference | Heroes / Hero, Player, Maps |
| 3 — analyses | Synergies, Trends, Patch notes / Hero changes, Leagues |
| 4 — tools | Draft console, Admin (lightened) |
| OBS overlays | **out of scope** by default (exploration mockup only). If they were retained: skin only — sizes, anchors and timings frozen by their specs unchanged |

## Acceptance criteria (per batch)

- `npm test` and `npm run build` green; no regression of real-time updates (WS).
- Browser check on real data of each page in the batch, at 1400 px and 390 px.
- Batch 0: fonts served by the app (no request to Google Fonts), ≤ 250 KB; Scouting pages
  visually identical after migration; OBS sources unchanged.
- Dense pages: table readability preserved (numbers in mono, no display type in rows).

## Documentation impact

`08-frontend.md` (design conventions, `ds/` components, each redone page);
`01-architecture.md` (the "Nexus Codex design" line specifies "· Dossier" once approved);
`docs/STATUS.md`.

## Open questions for the operator

1. Is the direction of the three mockups the right one (and the redesigned top bar)?
2. OBS overlays: confirm "out of scope" after seeing the exploration mockup?
3. Order of batches: Session / Matches / Match detail first, or another priority?
