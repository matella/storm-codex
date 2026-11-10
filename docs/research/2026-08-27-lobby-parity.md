# Standalone lobby parser parity — go/no-go report

**Measurement run on 2026-08-28** on the box's complete archive (3,322 replays, 25 builds from 2024 to
2026), copied read-only via `docker cp` then deleted. Harness:
`crates/storm-lobby/examples/parity.rs` — this file **itself** produces all the figures of this
report (breakdown per mode, over-capture/under-capture diagnosis, named divergences); no
throwaway script is involved in this measurement any more. Ground truth reference: the complete parse
(`storm-stats`), which knows the real composition of each game.

> Method note. An earlier version of this report computed its comparison base by only
> subtracting the replays rejected by `storm-stats`, leaving the open / missing-blob failures
> in the denominator without counting them anywhere (5 replays silently disappeared from the
> computation). The harness now counts these cases as `not evaluable`, subtracted from the base like
> `rejected (stats)`. The archive also grew by 8 replays between the initial copy and this
> measurement (3,314 → 3,322; Storm League 1,970 → 1,978) — the operator played in the meantime, with no
> consequence on the verdict.
>
> The tool's output labels were translated to English on 2026-11-09; the figures below are the
> 2026-08-28 run, with the labels shown in their current English form.

## Verdict

**GO on matchmade modes. Custom games are outside the guarantee.**

On Storm League, ARAM and Quick Match — 2,710 replays — the parser finds **100 % of BattleTags
and 100 % of teams**, without a single exception. The plan's criterion (≥ 99 %) is exceeded on the
scope that carries 83.9 % of the comparison base.

Custom games fail and must not be presented as reliable.

## The overall figure, and why it misleads

The measurement, all modes combined, gives:

```
replays seen         : 3322
not evaluable        : 5 (open/blob)
rejected (stats)     : 86
comparison base      : 3231
exact battletags     : 3108 (96.19 %)
parse errors         : 0
exact teams          : 2825 / 3108 evaluable (90.89 %)
  of which clean swap (sides swapped)       : 15 (5.30 % of wrong ones)
  of which order without team information  : 268 (94.70 % of wrong ones)
```

96.19 % < 99 %: **no-go** in the literal sense of the criterion, on the whole comparison base
(3,231 replays). But the breakdown per build already showed, before this measurement, an anomaly —
*recent* builds failed more than old ones. A format degradation would have produced
the opposite. The factor was therefore not the build; it is the game mode, below.

Breakdown per game mode (separate denominators: "evaluated" is not "exact battletags"):

| Mode | Evaluated | Exact BattleTags | Exact teams (among exact BattleTags) |
|---|---|---|---|
| 50091 — Storm League | 1,978 | 1,978 (**100.00 %**) | 1,978 (**100.00 %**) |
| 50101 — ARAM | 729 | 729 (**100.00 %**) | 729 (**100.00 %**) |
| 50001 — Quick Match | 3 | 3 (**100.00 %**) | 3 (**100.00 %**) |
| −1 — Custom | 521 | 398 (76.39 %) | 115 (28.89 %) |

The matchmade modes (Storm League + ARAM + Quick Match) total 1,978 + 729 + 3 = **2,710**
replays, i.e. **83.9 %** of the comparison base (2,710 / 3,231). Custom games carry
**16.1 %** (521 / 3,231) — same denominator, named explicitly here to avoid the mistake of an
earlier version of this report, which had reported these shares (87 % / 13 %) by computing them over
only the replays already successful (2,702 / 3,100) rather than over the whole comparison base.

The overall 96.19 % is **entirely** produced by custom games. It is a textbook case
of an average hiding two populations: mixing the two would have led to abandoning a capability
that is perfect where it is useful.

## Failure mode: over-capture, never loss

Checked **exhaustively** on the 3,231 replays of the comparison base, not on a
sample of divergences:

```
failure mode (over the whole base, 3231 replays)
  over-capture (10 real + extra occupants)   : 123 (3.81 %)
  under-capture or wrong decoding            : 0 (0.00 %)
```

The parser **never** misses a player and **never** produces a malformed decoding: all
123 BattleTag divergences of the archive are over-captures — the 10 real players
**plus** extra people. No occurrence of loss or wrong decoding in the whole
archive.

These extra people are real accounts present in the lobby but not playing —
**observers**. Diagnostic excerpt, named, on three faulty replays (file names
elided and indentation tightened for reading — the tool's raw output is more verbose;
the two blocks of figures above are, on the other hand, verbatim apart from the label translation):

```
2088f5ef… — 12 battletags decoded
    EXTRA -> RydzuSA#2791
    EXTRA -> Xardas182#21665
1e923b98… — 13 battletags decoded
    EXTRA -> RydzuSA#2791
    EXTRA -> TLHasuObs#2433
    EXTRA -> Bahamut#11165
a1f867de… — 11 battletags decoded
    EXTRA -> Krankle123#2459
```

`TLHasuObs` carries "Obs" in its name: it is an observer account. The lobby blob lists the
occupants of the lobby, not the players of the game — a distinction with no consequence in matchmaking (where
the two sets coincide), decisive in custom games.

## Teams: two failures of a different nature

The 5+5 deduction from the order is **100 % exact** in matchmaking. In custom games, out of the 398
replays whose 10 BattleTags are nevertheless correct:

- 115 correct (28.9 %)
- **283 wrong**, which the harness splits into two categories of a very different nature:
  - **15 clean swaps** (5.3 % of the wrong ones): the two sides are simply swapped.
    The order does carry the team information, all that is missing is which side is which —
    it is the only case a product can repair with one click.
  - **268 orders without team information** (94.7 % of the wrong ones): the composition cannot be
    reconstructed from the order, under any convention.

**The second case dominates by nearly 18 to 1** (268 / 15 = 17.9). In a custom lobby, players change slot and
team before the launch, and the blob's order reflects the order of arrival, not the final
composition. It is therefore not a sign to correct: the information is absent.

## Structural limit to know

The blob **does not carry the game mode** (established in task 1). The parser therefore cannot know
by itself whether it is reading a matchmade or a custom lobby. Consequence: on a custom game
with exactly 10 occupants and no observer, it will assign teams with the same
confidence as in Storm League — and will be wrong in most of these cases (283/398 ≈ 71 % of
custom games with correct BattleTags).

## Recommendations (operator decision)

1. **Keep the team deduction.** It is perfect on 2,710 matchmade games. Removing
   it to guard against custom games would degrade 83.9 % of cases for 16.1 %.
2. **Do not try to filter observers in the crate.** Nothing in the blob distinguishes them
   from a player; any heuristic would be an unmeasured guess.
3. **Treat "≠ 10 occupants" as the available signal.** This is already the parser's rule
   (`team: None` outside the exactly-10 case) and it catches custom games with observers — now tested
   by `crates/storm-lobby/tests/team_rule.rs` on a synthetic blob.
4. **Show the caveat on the product side** for the residual case — a custom game with exactly 10
   occupants — and do not count on an interface fix to catch it. A "swap
   the two teams" button only repairs **5.3 %** of the faulty cases; the remaining 94.7 % cannot be
   repaired by any manipulation, the order not carrying the information. The honest answer
   is therefore to let the operator correct the assignment by hand, or to accept a flat list
   in this specific case — not to present the swap as a general solution.

## Avenues not implemented

| Avenue | Cost | Risk |
|---|---|---|
| Find an observer marker in the blob | additional reverse-engineering, unpredictable duration | probable failure: task 1 found no usable structural field |
| Resolve teams via the archive (who usually plays with whom) | medium | only works for players already known; useless on unknowns |
| Correlate the blob's `.s2ma` hashes to the map via the archive | low | independent of the team problem; would remove the manual entry of the map |

## Reproduce

A single command regenerates the whole of this report — the overall figure, the breakdown per
mode, the over-capture/under-capture diagnosis, the breakdown per build and the named divergences:

```bash
cargo run --release -p storm-lobby --example parity -- <replay folder>
```

An optional `max` limits the number of files scanned (`... -- <folder> 500`); a non-numeric
value makes the tool fail rather than being silently swallowed.

Quick run-in on the committed corpus (no external archive required):
`cargo run --release -p storm-lobby --example parity -- crates/storm-replay/tests/data` →
3/3 exact (100 %).
