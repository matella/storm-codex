# Now Playing — animated reveal: implementation plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development
> (recommended) or superpowers:executing-plans to implement this plan task-by-task.
> Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a `/now-playing?reveal` variant that announces each new track with a
large card (264 px cover, title, artist) held for 2.6 s, then fades down onto the existing compact
badge at the top right.

**Architecture:** A three-state machine (`hidden|big|mini`) isolated in a **pure reducer**
(`revealState.ts`, testable in a node environment), wrapped in a thin hook that holds the
hold timer, and rendered by a presentational component whose morph is entirely in CSS
(one box that animates its size, two content layers cross-fading). Polling of
`/api/now-playing` goes from 5 s to 2 s.

**Tech Stack:** React 18 + TypeScript + Vite, TanStack Query v5, vitest 2 (**node** environment,
no React Testing Library in this repo), plain CSS per overlay file.

**Spec:** `docs/specs/2026-08-29-now-playing-reveal-design.md`
**Animated mockup:** `docs/specs/2026-08-29-now-playing-reveal-mockup.html` (only animates when served over
http; predates the "two-line title" decision — the spec is authoritative).

## Global Constraints

- **Break nothing in the existing variants.** `/now-playing` (rich card) and
  `/now-playing?mini` must behave exactly as today. The operator's OBS source
  points at one of them.
- **Frozen timings**: entry 440 ms, hold 2600 ms, morph 620 ms, easing
  `cubic-bezier(.22,.9,.24,1)`. Do not "improve" these values, they were tuned by hand.
- **Frozen geometry**: big = 300×408 (padding 18, cover 264×264 radius 12, title block
  **fixed 52 px** on two lines); mini = 290×68 (padding 10, cover 48×48 radius 8) — identical
  to the existing one.
- **Anchoring**: `OverlayFrame anchor="top-right" pad={36}`, unchanged.
- **Mandatory `npr-` CSS prefix.** `web/src/theme.css` already defines `.card`, `.kick` and `.mono`
  globally; an unprefixed `.card` class would break the card. All classes of the new
  CSS file start with `npr-`, under the `.npr` root.
- **Repo conventions**: strict TypeScript, conventional commits, comments and docs **in
  French** like the rest of `web/src` (at the time; the project switched to English on 2026-11-09).
- **No `any`.** The existing parser uses `Record<string, unknown>` + typed guards; follow
  that style.

---

### Task 1: Track identity

The reveal fires on a **change of track identity**. Today `parseTrack`
extracts no identifier: two tracks with the same title would be confused. We add `id` to
`Track` and a `trackKey` function that derives a stable key from it, falling back to `title|artist`.

**Files:**
- Modify: `web/src/api.ts` (interface `Track` ~line 490, body of `parseTrack` ~line 503)
- Create: `web/src/api.test.ts`

**Interfaces:**
- Consumes: nothing (first task).
- Produces:
  - `interface Track` gains the optional field `id?: string`.
  - `export function trackKey(t: Track): string | null` — `null` if the track has neither `id` nor
    `title`. Consumed by tasks 2 and 3.

- [ ] **Step 1: Write the failing tests**

Create `web/src/api.test.ts`:

```ts
import { describe, it, expect } from "vitest";
import { parseTrack, trackKey } from "./api";

describe("parseTrack", () => {
  it("extracts the Spotify id from the /api/playback/now shape", () => {
    const t = parseTrack({
      authenticated: true,
      current: { isPlaying: true, track: { id: "4uLU6hMCjMI75M1A2tKUQC", name: "Ghosts", artists: [{ name: "Foundry" }] } },
    });
    expect(t.id).toBe("4uLU6hMCjMI75M1A2tKUQC");
    expect(t.playing).toBe(true);
  });

  it("falls back to the uri when id is missing", () => {
    const t = parseTrack({
      authenticated: true,
      current: { isPlaying: true, track: { uri: "spotify:track:abc", name: "Ghosts" } },
    });
    expect(t.id).toBe("spotify:track:abc");
  });

  it("leaves id undefined when the source provides none", () => {
    const t = parseTrack({
      authenticated: true,
      current: { isPlaying: true, current: { name: "Ghosts", artist: "Foundry" } },
    });
    expect(t.id).toBeUndefined();
    expect(t.title).toBe("Ghosts");
  });
});

describe("trackKey", () => {
  it("prefers the id when it exists", () => {
    expect(trackKey({ playing: true, id: "abc", title: "Ghosts", artist: "Foundry" })).toBe("abc");
  });

  it("falls back to title|artist without an id", () => {
    expect(trackKey({ playing: true, title: "Ghosts", artist: "Foundry" })).toBe("Ghosts|Foundry");
  });

  it("tolerates a missing artist", () => {
    expect(trackKey({ playing: true, title: "Ghosts" })).toBe("Ghosts|");
  });

  it("returns null without id or title", () => {
    expect(trackKey({ playing: false })).toBeNull();
  });
});
```

- [ ] **Step 2: Run the tests to check that they fail**

Run: `npm test --prefix web -- api.test.ts`
Expected: FAIL — `trackKey` is not exported by `./api`, and the assertions on `t.id` fail.

- [ ] **Step 3: Add `id` to `Track` and extract it in `parseTrack`**

In `web/src/api.ts`, add the field to the `Track` interface (just after `playing`):

```ts
export interface Track {
  playing: boolean;
  /** stable track identifier (Spotify `id` or `uri`) — missing on the old engine. */
  id?: string;
  title?: string;
  artist?: string;
  art?: string;
  album?: string;
  durationMs?: number;
  progressMs?: number;
}
```

In the body of `parseTrack`, after the line that computes `const title = …`, add:

```ts
  const id = str(t.id) ?? str(t.uri);
```

then add `id` to the returned object:

```ts
  return { playing: !!(np?.authenticated && title && isPlaying), id, title, artist, art, album, durationMs, progressMs };
```

- [ ] **Step 4: Add `trackKey`**

In `web/src/api.ts`, just after `parseTrack`:

```ts
/** Identity key of a track, to detect a track change. `id` when the source
 *  provides one, otherwise `title|artist` (the old engine has no identifier). `null` = nothing
 *  usable, so nothing to announce. */
export function trackKey(t: Track): string | null {
  if (t.id) return t.id;
  if (t.title) return `${t.title}|${t.artist ?? ""}`;
  return null;
}
```

- [ ] **Step 5: Run the tests to check that they pass**

Run: `npm test --prefix web -- api.test.ts`
Expected: PASS — 7 tests.

- [ ] **Step 6: Check that nothing regressed**

Run: `npm test --prefix web`
Expected: PASS — the whole suite (`api`, `clipExport`, `replay2d`, `usePlayback`).

- [ ] **Step 7: Commit**

```bash
git add web/src/api.ts web/src/api.test.ts
git commit -m "feat(now-playing): track identity (Spotify id + trackKey)"
```

---

### Task 2: Reveal reducer

All the trigger policy lives here, pure, without React or DOM — this is the seam that makes
the logic testable in vitest's node environment (this repo has no React Testing Library;
`usePlayback.ts` / `usePlayback.test.ts` follow exactly this model with its pure function
`advance`).

Chosen policy (spec, decisions 2 and 4): **every playback start** announces (new
track, resume after pause, first mount with music already playing); a track change
while the big card is shown **replaces the content and restarts the hold** without going back
through the mini.

**Files:**
- Create: `web/src/revealState.ts`
- Create: `web/src/revealState.test.ts`

Root of `web/src/` and not `pages/`: that is where the repo's pure logic and hooks already live
(`usePlayback.ts`, `replay2d.ts`, `clipExport.ts`). Only the overlay components and their CSS
go into `pages/` (`DraftOverlay.tsx` + `draft-overlay.css`).

**Interfaces:**
- Consumes: nothing from task 1 at the code level (the reducer already receives a computed key).
- Produces:
  - `export type RevealPhase = "hidden" | "big" | "mini"`
  - `export interface RevealState { phase: RevealPhase; key: string | null; holdToken: number }`
  - `export interface RevealInput { playing: boolean; key: string | null }`
  - `export const INITIAL_REVEAL: RevealState`
  - `export function nextRevealState(prev: RevealState, next: RevealInput): RevealState`
  - `export function holdExpired(prev: RevealState): RevealState`

  `holdToken` is incremented **every time the hold must be (re)armed**; this counter is what
  the task 3 hook uses as an effect dependency to restart its timer. Both functions
  return **the `prev` object itself** (referential identity) when nothing changes, so that the
  hook does not re-arm the timer on every poll.

- [ ] **Step 1: Write the failing tests**

Create `web/src/revealState.test.ts`:

```ts
import { describe, it, expect } from "vitest";
import { INITIAL_REVEAL, nextRevealState, holdExpired, type RevealState } from "./revealState";

const at = (phase: RevealState["phase"], key: string | null, holdToken = 0): RevealState =>
  ({ phase, key, holdToken });

describe("nextRevealState", () => {
  it("first poll with music playing → big, hold armed", () => {
    const s = nextRevealState(INITIAL_REVEAL, { playing: true, key: "a" });
    expect(s.phase).toBe("big");
    expect(s.key).toBe("a");
    expect(s.holdToken).toBe(INITIAL_REVEAL.holdToken + 1);
  });

  it("resume after pause → big (same track as before the pause)", () => {
    const paused = nextRevealState(at("mini", "a", 3), { playing: false, key: "a" });
    expect(paused.phase).toBe("hidden");
    const resumed = nextRevealState(paused, { playing: true, key: "a" });
    expect(resumed.phase).toBe("big");
    expect(resumed.holdToken).toBe(paused.holdToken + 1);
  });

  it("track change in big → stays big, content replaced, hold restarted", () => {
    const s = nextRevealState(at("big", "a", 5), { playing: true, key: "b" });
    expect(s.phase).toBe("big");
    expect(s.key).toBe("b");
    expect(s.holdToken).toBe(6);
  });

  it("track change in mini → big", () => {
    const s = nextRevealState(at("mini", "a", 5), { playing: true, key: "b" });
    expect(s.phase).toBe("big");
    expect(s.key).toBe("b");
    expect(s.holdToken).toBe(6);
  });

  it("playback stop → hidden, without incrementing the hold", () => {
    const s = nextRevealState(at("mini", "a", 5), { playing: false, key: "a" });
    expect(s.phase).toBe("hidden");
    expect(s.key).toBeNull();
    expect(s.holdToken).toBe(5);
  });

  it("track without a usable key → hidden", () => {
    const s = nextRevealState(at("mini", "a", 5), { playing: true, key: null });
    expect(s.phase).toBe("hidden");
  });

  it("identical poll → same object, no re-arming", () => {
    const prev = at("mini", "a", 5);
    expect(nextRevealState(prev, { playing: true, key: "a" })).toBe(prev);
  });

  it("identical poll in big → same object (the hold does not restart)", () => {
    const prev = at("big", "a", 5);
    expect(nextRevealState(prev, { playing: true, key: "a" })).toBe(prev);
  });

  it("repeated stop → same object", () => {
    const prev = at("hidden", null, 5);
    expect(nextRevealState(prev, { playing: false, key: null })).toBe(prev);
  });
});

describe("holdExpired", () => {
  it("big → mini", () => {
    expect(holdExpired(at("big", "a", 5))).toEqual(at("mini", "a", 5));
  });

  it("does not touch mini", () => {
    const prev = at("mini", "a", 5);
    expect(holdExpired(prev)).toBe(prev);
  });

  it("does not touch hidden", () => {
    const prev = at("hidden", null, 5);
    expect(holdExpired(prev)).toBe(prev);
  });
});
```

- [ ] **Step 2: Run the tests to check that they fail**

Run: `npm test --prefix web -- revealState.test.ts`
Expected: FAIL — `Failed to resolve import "./revealState"`.

- [ ] **Step 3: Write the reducer**

Create `web/src/revealState.ts`:

```ts
// State machine of the music reveal (/now-playing?reveal), pure: no React or DOM, so
// testable in vitest's node environment. Same split as usePlayback.ts / advance().

/** `hidden` = nothing on screen; `big` = large announcement card; `mini` = persistent compact badge. */
export type RevealPhase = "hidden" | "big" | "mini";

export interface RevealState {
  phase: RevealPhase;
  /** key of the track carried by the card; `null` when `phase === "hidden"`. */
  key: string | null;
  /** incremented every time the hold must be (re)armed — effect dependency of the hook. */
  holdToken: number;
}

export interface RevealInput {
  playing: boolean;
  key: string | null;
}

export const INITIAL_REVEAL: RevealState = { phase: "hidden", key: null, holdToken: 0 };

/**
 * Transition on a poll update.
 *
 * Chosen rule (spec, decision 2): **every playback start announces** — new track,
 * resume after pause, first mount with music already playing. And (decision 4) a track change
 * during the big card **retargets it in place**: we stay in `big`, restart the
 * hold, and do not go back through the mini — otherwise the card yo-yos when skipping repeatedly.
 *
 * Returns `prev` **as is** when nothing changes, so that the hook does not re-arm its timer on
 * every poll.
 */
export function nextRevealState(prev: RevealState, next: RevealInput): RevealState {
  if (!next.playing || next.key === null) {
    return prev.phase === "hidden" ? prev : { phase: "hidden", key: null, holdToken: prev.holdToken };
  }
  if (prev.phase === "hidden" || next.key !== prev.key) {
    return { phase: "big", key: next.key, holdToken: prev.holdToken + 1 };
  }
  return prev;
}

/** Transition on hold expiry: the big card folds down onto the badge. */
export function holdExpired(prev: RevealState): RevealState {
  return prev.phase === "big" ? { ...prev, phase: "mini" } : prev;
}
```

- [ ] **Step 4: Run the tests to check that they pass**

Run: `npm test --prefix web -- revealState.test.ts`
Expected: PASS — 12 tests.

- [ ] **Step 5: Commit**

```bash
git add web/src/revealState.ts web/src/revealState.test.ts
git commit -m "feat(now-playing): pure reducer for the reveal state machine"
```

---

### Task 3: Hook, card, route and live check

The hook, the CSS and the component are only observable once routed: they form a single
deliverable, checked on screen. No unit test of the hook — this repo has no React Testing Library
and introducing it for a six-line hook would be disproportionate; all the logic that deserves a
test is already covered in task 2.

**Files:**
- Create: `web/src/useTrackReveal.ts`
- Create: `web/src/pages/now-playing-reveal.css`
- Create: `web/src/pages/RevealCard.tsx`
- Modify: `web/src/pages/NowPlaying.tsx` (variant switch + `refetchInterval`)
- Modify: `docs/STATUS.md`

**Interfaces:**
- Consumes:
  - `trackKey(t: Track): string | null` and `Track` (with `id?`) — task 1, from `../api`.
  - `INITIAL_REVEAL`, `nextRevealState`, `holdExpired`, `RevealPhase` — task 2, from
    `./revealState` (in `useTrackReveal.ts`, at the root) or `../revealState` (in
    `RevealCard.tsx`, under `pages/`).
  - `OverlayFrame` — existant, `../components/OverlayFrame`.
- Produces:
  - `export function useTrackReveal(playing: boolean, key: string | null, holdMs?: number): RevealPhase`
  - `export function RevealCard({ track, phase }: { track: Track; phase: RevealPhase }): JSX.Element`

- [ ] **Step 1: Write the hook**

Create `web/src/useTrackReveal.ts`:

```ts
// Timer of the music reveal. The hook holds no rule: it applies the pure reducer of
// revealState.ts and arms the hold. `holdToken` as an effect dependency means any (re)targeting
// cancels the running timer and starts a fresh one.
import { useEffect, useState } from "react";
import { INITIAL_REVEAL, nextRevealState, holdExpired, type RevealPhase } from "./revealState";

/** How long the big card stays before folding onto the badge (spec: frozen at 2.6 s). */
export const HOLD_MS = 2600;

export function useTrackReveal(playing: boolean, key: string | null, holdMs = HOLD_MS): RevealPhase {
  const [state, setState] = useState(INITIAL_REVEAL);

  useEffect(() => {
    setState((prev) => nextRevealState(prev, { playing, key }));
  }, [playing, key]);

  useEffect(() => {
    if (state.phase !== "big") return;
    const id = setTimeout(() => setState(holdExpired), holdMs);
    return () => clearTimeout(id);
  }, [state.phase, state.holdToken, holdMs]);

  return state.phase;
}
```

- [ ] **Step 2: Write the morph CSS**

Create `web/src/pages/now-playing-reveal.css`. **All classes are prefixed `npr-`**:
`theme.css` defines `.card`, `.kick` and `.mono` globally.

```css
/* Overlay musique OBS — variante « reveal » (/now-playing?reveal).
   "Dissolve" morph: a single box animates its size, two content layers fade
   into each other. Timings frozen by the 2026-08-29 spec — do not touch.
   Ported from docs/specs/2026-08-29-now-playing-reveal-mockup.html (option A). */
.npr {
  --npr-morph: 620ms;
  --npr-enter: 440ms;
  --npr-ease: cubic-bezier(.22, .9, .24, 1);
}

.npr-card {
  position: relative;
  background: linear-gradient(135deg, rgba(18, 20, 28, .96), rgba(12, 13, 18, .96));
  border: 1px solid var(--hairline-strong);
  box-shadow: 0 14px 40px rgba(0, 0, 0, .55);
  will-change: width, height, transform, opacity;
  /* resting state = mini badge geometry; the entry grows from this size. */
  width: 290px;
  height: 68px;
  border-radius: 16px;
  opacity: 0;
  transform: translateY(-10px) scale(.94);
  transition:
    width var(--npr-morph) var(--npr-ease),
    height var(--npr-morph) var(--npr-ease),
    border-radius var(--npr-morph) var(--npr-ease),
    opacity var(--npr-enter) ease,
    transform var(--npr-enter) var(--npr-ease);
}

.npr-layer { position: absolute; inset: 0; opacity: 0; transition: opacity calc(var(--npr-morph) * .55) ease; }

/* ── "big card" layer ─────────────────────────────────────────────── */
.npr-big { padding: 18px; display: flex; flex-direction: column; align-items: center; text-align: center; }
.npr-big .npr-krow { display: flex; align-items: center; gap: 8px; align-self: flex-start; height: 12px; margin-bottom: 10px; }
.npr-big .npr-art { width: 264px; height: 264px; border-radius: 12px; }
.npr-big .npr-title {
  margin-top: 14px; height: 52px; font-size: 20px; line-height: 1.3; font-weight: 700; width: 100%;
  /* two lines max: 264px at 20px truncates around 22 characters on a single line. The height
     stays fixed so the box keeps an animation target. */
  display: -webkit-box; -webkit-line-clamp: 2; -webkit-box-orient: vertical; overflow: hidden;
}
.npr-big .npr-artist { margin-top: 2px; height: 18px; font-size: 14px; color: var(--text-2); width: 100%;
  white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }

/* ── "mini badge" layer (geometry identical to the ?mini variant) ─────── */
.npr-mini { padding: 10px; display: flex; gap: 12px; align-items: center; }
.npr-mini .npr-art { width: 48px; height: 48px; border-radius: 8px; }
.npr-mini .npr-txt { flex: 1; min-width: 0; }
.npr-mini .npr-title { font-size: 14px; font-weight: 700; line-height: 1.25;
  white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
.npr-mini .npr-artist { margin-top: 1px; font-size: 12px; color: var(--text-2);
  white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }

/* ── cover and placeholder ─────────────────────────────────────────────── */
.npr-art { background-size: cover; background-position: center; flex-shrink: 0; box-shadow: 0 4px 14px rgba(0, 0, 0, .5); }
.npr-art.npr-none { display: flex; align-items: center; justify-content: center;
  background: var(--surface-2); color: var(--text-2); }
.npr-big .npr-art.npr-none { font-size: 100px; }
.npr-mini .npr-art.npr-none { font-size: 18px; }

/* ── kicker + equaliser ──────────────────────────────────────────────────── */
.npr-kick { color: var(--accent); font-size: 10px; letter-spacing: .08em; text-transform: uppercase; font-weight: 600; }
.npr-eq { display: inline-flex; align-items: flex-end; gap: 2px; height: 11px; }
.npr-eq i { width: 3px; background: var(--accent); border-radius: 1px; animation: npr-eq .9s ease-in-out infinite; }
.npr-eq i:nth-child(2) { animation-delay: .18s; }
.npr-eq i:nth-child(3) { animation-delay: .36s; }
.npr-eq i:nth-child(4) { animation-delay: .54s; }
@keyframes npr-eq { 0%, 100% { height: 3px; } 50% { height: 11px; } }

/* ── states ───────────────────────────────────────────────────────────────── */
.npr.is-big .npr-card { width: 300px; height: 408px; border-radius: 18px; opacity: 1; transform: none; }
.npr.is-big .npr-big { opacity: 1; transition-delay: calc(var(--npr-morph) * .35); }
.npr.is-mini .npr-card { width: 290px; height: 68px; border-radius: 16px; opacity: 1; transform: none; }
.npr.is-mini .npr-mini { opacity: 1; transition-delay: calc(var(--npr-morph) * .45); }

@media (prefers-reduced-motion: reduce) {
  .npr-card, .npr-layer { transition: none; }
  .npr-eq i { animation: none; height: 7px; }
}
```

- [ ] **Step 3: Write the presentational component**

Create `web/src/pages/RevealCard.tsx`:

```tsx
import type { Track } from "../api";
import type { RevealPhase } from "../revealState";
import "./now-playing-reveal.css";

/** Cover, or a ♫ placeholder when the source provides none. */
function Art({ art }: { art?: string }) {
  if (!art) return <div className="npr-art npr-none">♫</div>;
  return <div className="npr-art" style={{ backgroundImage: `url(${art})` }} />;
}

/**
 * Music reveal card: a single box that animates between the big announcement and the compact
 * badge, with the two content layers cross-fading. Both layers are **always
 * mounted** — that is what makes the fade possible; the state class on the root decides.
 */
export function RevealCard({ track, phase }: { track: Track; phase: RevealPhase }) {
  return (
    <div className={`npr is-${phase}`}>
      <div className="npr-card">
        <div className="npr-layer npr-big">
          <div className="npr-krow">
            <span className="npr-kick">Now playing</span>
            <span className="npr-eq"><i /><i /><i /><i /></span>
          </div>
          <Art art={track.art} />
          <div className="npr-title">{track.title}</div>
          <div className="npr-artist">{track.artist}</div>
        </div>
        <div className="npr-layer npr-mini">
          <Art art={track.art} />
          <div className="npr-txt">
            <div className="npr-title">{track.title}</div>
            <div className="npr-artist">{track.artist}</div>
          </div>
        </div>
      </div>
    </div>
  );
}
```

- [ ] **Step 4: Wire the variant and tighten the polling**

In `web/src/pages/NowPlaying.tsx`:

1. Add the imports at the top of the file:

```ts
import { parseTrack, trackKey } from "../api";
import { RevealCard } from "./RevealCard";
import { useTrackReveal } from "../useTrackReveal";
```

(the existing `import { parseTrack } from "../api";` line is replaced by the one above)

2. In the body of `NowPlaying`, replace the line that reads the URL parameter:

```ts
  const params = new URLSearchParams(window.location.search);
  const mini = params.has("mini");
  const reveal = params.has("reveal");
```

3. Change `refetchInterval` from `5000` to `2000` in the `useQuery` (spec, decision 3: under the rule
   "every playback start announces", 5 s makes the reveal arrive visibly late).

4. Right after `const t = parseTrack(data);`, call the hook — **unconditionally**, before
   any early `return`, otherwise the rules of hooks are violated:

```ts
  const phase = useTrackReveal(t.playing, trackKey(t));
```

5. Add the variant branch **before** the existing `if (!t.playing) return …` (the card must
   be able to stay mounted while the CSS makes it disappear):

```tsx
  if (reveal) {
    return (
      <OverlayFrame anchor="top-right" pad={36}>
        <RevealCard track={t} phase={phase} />
      </OverlayFrame>
    );
  }
```

6. Update the file's header comment to mention the three variants:

```
 *   - default: rich card (cover, title/artist/album, progress, equaliser).
 *   - `?mini`: compact (cover + title + artist only).
 *   - `?reveal`: big announcement at each playback start, then folds onto `?mini`.
```

- [ ] **Step 5: Check types and build**

Run: `npm run build --prefix web`
Expected: PASS — `tsc -b` without errors then the Vite build. On a "React Hook called
conditionally" error, the `useTrackReveal` call was placed after a `return`: move it up.

- [ ] **Step 6: Check that the test suite still passes**

Run: `npm test --prefix web`
Expected: PASS — full suite, including `api.test.ts` and `revealState.test.ts`.

- [ ] **Step 7: Live visual check**

Start the dev server via the preview tool (config `storm-codex-web` of
`.claude/launch.json`, port 5180) — **not via Bash**.

Since Orpheus cannot be reached from the Mac, `/api/now-playing` answers with an error or
`{authenticated:false}`: the card stays hidden. So drive the state machine by hand from
the page console, at `http://localhost:5180/now-playing?reveal`:

```js
// force the three states to check geometry and fade
document.querySelector(".npr").className = "npr is-big";
document.querySelector(".npr").className = "npr is-mini";
```

Check:
- [ ] `is-big` → card 300×408, anchored 36 px from the top and right edges;
- [ ] `is-mini` → card 290×68, same anchoring, top-right corner unchanged;
- [ ] the transition between the two takes ~620 ms with no position jump;
- [ ] a long title (inject `document.querySelector(".npr-big .npr-title").textContent = "A deliberately very long title to test line wrapping"`) shows on **two lines** then an ellipsis, **without changing the card's height**;
- [ ] without a cover, the ♫ placeholder shows at both sizes.

Take a screenshot of the `is-big` state and of the `is-mini` state as evidence.

- [ ] **Step 8: Check non-regression of the two other variants**

Open `http://localhost:5180/now-playing` then `http://localhost:5180/now-playing?mini`.
Expected: empty frame in both cases (Orpheus unreachable from the Mac → `playing:false`), no
error in the console. This is today's behaviour, unchanged.

- [ ] **Step 9: Update `docs/STATUS.md`**

In the section "OBS scene "between games"", replace the existing bullet:

```
- **`/now-playing`** (persistent source, EN): music widget reading **Orpheus** via the proxy
  `/api/now-playing` (config `ORPHEUS_URL`). Shows "Music — off" while Spotify is asleep.
```

with:

```
- **`/now-playing`** (persistent source, EN): music widget reading **Orpheus** via the proxy
  `/api/now-playing` (config `ORPHEUS_URL`). Shows "Music — off" while Spotify is asleep.
  Three variants: default (rich card), `?mini` (compact badge), `?reveal` (big announcement
  at each playback start — entry 440 ms, hold 2.6 s, morph 620 ms — then folds onto the
  badge). Spec `docs/specs/2026-08-29-now-playing-reveal-design.md`, plan
  `docs/plans/2026-08-29-now-playing-reveal.md`. Polling changed from 5 s to 2 s.
  **Still to check on the box**: (a) that Orpheus does not relay the Spotify API without a cache — the
  2 s polling makes ~30 requests/min per open source; documented fallback = WS push
  `music.changed`; (b) that the reveal actually shows, which requires the Spotify OAuth to be done
  (see the "Orpheus (music)" section further down). Until then only the geometry is verified,
  by driving the state classes by hand.
```

- [ ] **Step 10: Commit**

```bash
git add web/src/useTrackReveal.ts web/src/pages/now-playing-reveal.css \
        web/src/pages/RevealCard.tsx web/src/pages/NowPlaying.tsx docs/STATUS.md
git commit -m "feat(now-playing): ?reveal variant (big announcement → mini badge)"
```

---

## Still to check on the box (outside the plan)

These two points cannot be checked from the Mac and do not block the implementation:

1. **Orpheus cache.** The switch to 2 s assumes Orpheus does not relay the Spotify API without a cache.
   Documented fallback if the assumption fails: WS push `music.changed` (out of the spec's scope).
2. **Real rendering.** The reveal can only be seen end to end with the Spotify OAuth done on the
   Orpheus side (`docs/STATUS.md`, section "Orpheus (music)"). Until then, only the geometry is
   checkable, by driving the classes by hand.
