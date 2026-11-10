# Now Playing — animated reveal (large card → mini badge) — design

> storm-codex spec. Brainstorm approved by the operator on 2026-08-29.
> Reference animated mockup: `docs/specs/2026-08-29-now-playing-reveal-mockup.html`
> (predates the "title on two lines" decision: on that point, the spec is authoritative).
> Target: `writing-plans` then TDD implementation.

## Goal

When a track starts, the music overlay shows **large** (big cover art, title and
artist below), holds for ~2.6 s, then **smoothly collapses** into the compact badge already in place
at the top right. The badge then stays displayed until the next track.

Today `/now-playing?mini` shows the compact badge directly: the track changes without anything
announcing it. The reveal gives the title a moment of readability on air, without occupying the
corner any longer than today the rest of the time.

## Scope

**Third variant of the existing route**: `/now-playing?reveal`. The default variant
(fuller card) and `?mini` do not change — the current OBS source keeps working
identically; switching is done by editing the browser source's URL. If the variant proves itself in use,
it could be promoted to the default of `?mini` later; that is not decided here.

## Decisions made (brainstorm)

1. **Chosen morph: "Dissolve"** (option A of the mockup). The box animates in size; the two
   contents (large / mini) cross-fade. Chosen because it keeps the existing
   style and reuses the mini badge's markup as is. Options ruled out: "Morph"
   (cover art and text actually travelling — requires left-aligned text in both states,
   otherwise the alignment jumps mid-flight) and "Swap" (two separate cards, hard cut).
2. **Trigger: every playback start.** Track change, resume after pause, and
   first mount of the page with something playing. Operator's motivation: consistency, and
   they do not change scenes. **Accepted consequence**: reloading the browser source
   (stream start, OBS restart, refresh) re-announces the track already playing.
3. **Polling frequency: 2 s** (instead of 5 s). Under rule 2, resuming after a pause
   triggers a reveal; at 5 s the reveal visibly lags behind the music.
4. **Quick skip: retarget in place.** A new track while the large card is
   displayed replaces the content and **restarts the hold**, without going back through the mini badge. Avoids
   size yo-yo when skipping repeatedly.
5. **Timings**: entry 440 ms, hold 2.6 s, morph 620 ms, easing `cubic-bezier(.22,.9,.24,1)`.
   Tuned by hand on the mockup and approved as is.
6. **Contents of the large card**: cover art, `NOW PLAYING` kicker + equaliser, title, artist.
   **No album or progress bar** — it is a 2.6 s card, not a player.

## Behaviour

Three-state machine: `hidden` → `big` → `mini`.

| Event | Current state | Transition |
|---|---|---|
| `playing` goes false → true | `hidden` | → `big`, hold armed |
| Track identity changes | `big` | stays `big`, content replaced, **hold restarted** |
| Track identity changes | `mini` | → `big`, hold armed |
| Hold expired | `big` | → `mini` |
| `playing` goes true → false | any | → `hidden` |
| Identical poll | any | no effect |

The mini badge persists indefinitely as long as playback continues — current behaviour unchanged.
Pause and stop hide the card, as today.

### Track identity

`parseTrack` (`web/src/api.ts`) extracts no identifier: two tracks with the same title would
not be told apart. The `id` key is added to `Track` (Spotify `id` or `uri` field when
present), falling back to `title|artist`. It is this key, not the title, that triggers the reveal.

## Breakdown

`NowPlaying.tsx` already holds ~110 lines and two inline layouts; adding a third
plus a state machine overloads it. Split along the seam already used by the repo
(`usePlayback.ts` + pure `advance` function tested in a node environment — no RTL here):

- **`revealState.ts`** — `nextRevealState(prev, incoming)`, a **pure** reducer returning the state
  (`hidden|big|mini`) and whether to re-arm the hold. All the triggering and
  retargeting policy lives there. No React or DOM: unit-testable.
- **`useTrackReveal.ts`** — thin hook around the reducer: hold timer, cleanup on
  unmount.
- **`RevealCard.tsx`** — pure presentation. Receives `{ track, state }`, renders the morphing box.
- **`NowPlaying.tsx`** — switch between the three variants, otherwise unchanged.

## Animation

A single `.card` box transitioning `width` / `height` / `border-radius` over 620 ms with
`cubic-bezier(.22,.9,.24,1)`. Two absolutely positioned content layers cross-fade over ~340 ms
with a delay, so that the incoming layer arrives when the box has already done most of its
size change.

- **Large**: 300×408, padding 18, cover art 264×264 (radius 12), kicker + equaliser at the top
  left, 20 px title and 14 px artist centred under the cover art. The height includes the fixed
  52 px title block (two lines, see "Edge cases"): the mockup, which predates this
  decision, shows 382 on a single line. These dimensions are those of the **outer box**
  (global `box-sizing: border-box` in `theme.css`, 1 px border): the available interior is
  therefore 298×406 and the content uses 1 px of margin on each edge. Discrepancy accepted by the operator
  on 2026-08-29 — it is the rendering approved on the mockup, which had the same border; nothing
  overflows the card.
- **Mini**: 290×68, padding 10, cover art 48×48 (radius 8), 14 px title, 12 px artist — identical
  to the existing one.
- **Anchoring**: `OverlayFrame anchor="top-right" pad={36}`, unchanged.

## Edge cases

- **Long titles.** 264 px at 20 px truncates around 22 characters — unacceptable for a card whose
  only role is to announce the title. The large card's title area is a block of **fixed
  52 px** height allowing **two lines**, with an ellipsis beyond: the height stays fixed, so the box
  stays animatable. The mini stays on one line.
- **`prefers-reduced-motion`**: hard switch between states, no morph.
- **Missing cover art**: the existing `♫` placeholder, sized according to the state.
- **Orpheus missing or not authenticated**: `{authenticated:false}` → `playing:false` → `hidden`.
  Unchanged.
- **Unmount during a reveal**: timers cancelled.

## Assumption to check (non-blocking)

The move to 2 s assumes Orpheus does not relay the Spotify API on every call without a cache. ~30
requests/min per open source should stay comfortable, but **it is not verified**:
Orpheus is not in this repo and the box only runs in the evening. To be confirmed on the box before
considering the budget met; if Orpheus has no cache, the fallback is WS push (see Out of scope).

## Tests

Unit tests of the reducer (vitest, node environment), modelled on `usePlayback.test.ts`:

- first poll with playback in progress → `big`, hold armed;
- resume after pause → `big`;
- track change in `big` state → stays `big`, hold restarted;
- track change in `mini` state → `big`;
- hold expired → `mini`;
- playback stopped → `hidden`;
- repeated identical poll → no state change and no re-arm.

Visual check: animated mockup + live check in the browser against the dev
server.

## Out of scope

- **WS push** (`music.changed` broadcast by the server, like `lobby.detected`): architecturally
  correct and sub-second, but it is server work for a visual flourish.
  To be reopened only if 2 s polling proves insufficient or if Orpheus cannot cope.
- Album and progress bar in the large card.
- Sound.
- Any change to the default and `?mini` variants.
