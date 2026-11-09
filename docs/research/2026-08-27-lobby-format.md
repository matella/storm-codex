# Inspection of the `replay.server.battlelobby` format (companion live, task 1)

**Date:** 2026-08-27 · **Status: investigation, no architecture decision** — this document
answers the companion-live spec's 6 questions with supporting evidence; it conditions the writing
of the parser in task 3.

**Tool:** `crates/storm-replay/examples/dump_lobby.rs` — extracts the raw (undecoded)
`replay.server.battlelobby` stream of a `.StormReplay` to a `.bin` file, via
`Replay::open()` + `battlelobby_raw()`. It is the same blob as the one written live by the game into
`%TEMP%\Heroes of the Storm\TempWriteReplayP1\replay.server.battlelobby`.

```
cargo run -p storm-replay --example dump_lobby -- <replay> <output.bin>
```

## Dumped corpus

| Source file | Blob size | Players (via `storm-stats-dump`) |
|---|---|---|
| `crates/storm-stats/tests/data/silver-city-aram.StormReplay` | 279,694 bytes | 10 (ARAM, Silver City) |
| `crates/storm-replay/tests/data/2024-10-07 22.29.55 Industrial District.StormReplay` | 280,388 bytes | 10 |
| `crates/storm-replay/tests/data/2026-05-27 22.37.45 Industrial District.StormReplay` | 281,735 bytes | 10 |
| `crates/storm-replay/tests/data/2026-06-09 20.35.02 Industrial District.StormReplay` | 281,642 bytes | 10 |

4 unique replays → 5 `.bin` files produced by step 3 (`silver-city.bin` and
`silver-city-aram.bin` are two dumps of the same replay, identical sizes 279,694 bytes — expected).

To have a reference truth (player names, heroes *as decoded by the existing
pipeline*, teams), each replay was also run through the already existing binary
`storm-stats-dump` (`crates/storm-stats/src/bin/storm-stats-dump.rs`) — **this reference comes from
`replay.details()`, not from the lobby**; it is only used as a point of comparison to look for the
same strings in the lobby blob.

Excerpt for `silver-city-aram`:

```
2-Hero-1-10724235 | Sando         | Tassadar        | team 0
2-Hero-1-1770353  | Kain          | Qhira           | team 0
2-Hero-1-14148348 | ShieldedSlug  | The Lost Vikings| team 0
2-Hero-1-14148822 | TheBlackKing  | Li Li           | team 0
2-Hero-1-7293976  | Cascon        | Chromie         | team 0
2-Hero-1-6130044  | slashbored    | Varian          | team 1
2-Hero-1-13579289 | SVKpermaBan   | Mephisto        | team 1
2-Hero-1-13495235 | LenaOxton     | Jaina           | team 1
2-Hero-1-1863125  | Hyldii        | Zarya           | team 1
2-Hero-1-2138731  | Shrek         | Rehgar          | team 1
```

(key = `toon_handle`, format confirmed by the existing code: `region-Hero-realm-id`.)

---

## Answers to the 6 questions

### 1. Are the BattleTags present, and in lobby order?

**Yes**, present in clear text, and in an order that matches the grouping by team (5+5) as
returned by `storm-stats` (itself derived from `replay.details()`, not from the lobby).

Evidence (`strings -n 3 silver-city.bin`, lines 2343–2368):

```
Sando#2475
Kain#2164
ShieldedSlug#2429
TheBlackKing#21829
Cascon#11441
slashbored#2867
SVKpermaBan#2161
LenaOxton#21672
Hyldii#21343
Shrek#2533
```

Order identical to the table above (team 0: Sando, Kain, ShieldedSlug, TheBlackKing, Cascon;
team 1: slashbored, SVKpermaBan, LenaOxton, Hyldii, Shrek). Confirmed on a 2nd replay
(`2026-06-09 … Industrial District`, 9 of the 10 tags visible via `strings`, see Q3/limit below)
with the same order pattern.

**Important nuance for the parser (task 3):** a non-ASCII BattleTag (Russian player
`ЛовкийЭльф#215346` in the 2026-06-09 Industrial District replay) **does not appear** in the
output of `strings` (which only detects printable ASCII runs) — it is indeed present in the
blob but in multi-byte UTF-8. Checked by searching the raw bytes:

```
offset 0x43e38 : 07 ce 86 4b 21 1b d0 9b d0 be d0 b2 d0 ba d0 b8 d0 b9 d0 ad d0 bb d1 8c d1 84
                 23 32 31 35 33 34 36 00
                 [len=0x1b=27] Л  о  в  к  и  й  Э  л  ь  ф  #  2  1  5  3  4  6  \0
```

(`0x43e3e`, wrongly cited as the starting offset in an earlier version of this document, is
actually the first byte of the Cyrillic name itself — `d0 9b` = "Л" — that is, byte
0x43e38 + 6, once past the 6 bytes `07 ce 86 4b 21 1b` that precede the UTF-8 string.)

The prefix `0x1b` = 27 matches exactly the length **in UTF-8 bytes** of
`"ЛовкийЭльф#215346"` (10 Cyrillic characters × 2 bytes = 20, + `"#215346"` = 7 bytes → 27).
**The task 3 parser must therefore read a byte count then decode as UTF-8, not assume
pure ASCII or a character count.**

### 2. Are the toon handle components (`m_region`, `m_programId`, `m_realm`, `m_id`) present?

**Unknown / probably not in a directly usable form.** Each BattleTag is preceded by
a text field of the form `T:<number>#<number>`, for example just before `Sando#2475`:

```
0004338e: 03 00 00 00 19 01 54 3a 31 30 35 36 31 37 35 37   ......T:1056175
0004339e: 36 23 32 32 37 00 01 01 ...                        6#227...
```
i.e. the string `"T:105617576#227\0"` immediately followed, further on, by `"Sando#2475"` (with its
own length byte `0x0a`=10, well checked, see below). **Unlike the BattleTag
field**, the length-prefix mechanism of this `T:…#…` field has not been identified with
certainty (the immediately preceding byte, `0x01`, does not match the string's length
`15`; it is possibly a type tag or a multi-byte varint) — noted here
as a raw observation, not as a confirmed mechanism.

This number (`105617576`) **matches none of the real `m_id`s** of the 10 players
(`10724235, 1770353, 14148348, 14148822, 7293976, 6130044, 13579289, 13495235, 1863125,
2138731`) — checked by an exhaustive search of the 10 `m_id`s as ASCII and as 32-bit
little/big-endian integers in the blob: **no occurrence, in any of the three forms**. The
`T:…#…` field therefore has **unknown** semantics (lead: a session/lobby identifier on the
Battle.net side, not the player's `toon_handle`) and must not be confused with `m_id`.

No string `m_region`, `m_programId`, `m_realm`, `toonHandle`, nor a text pattern like
`"2-Hero-1-…"` (confirmed format of the application toon handle, see `ReplayHeader.toon_handle`) was
found in the blob. **Conclusion: the toon handle components are not present in an obvious
readable form; the link will have to be rebuilt from the set of BattleTags (name#tag)
rather than by toon handle** — consistent with the default resolution planned in case of doubt.

### 3. Can team membership be deduced (explicit field, or position/order)?

**By order only, to be confirmed — no explicit "team" field identified near the
player records.** The order of appearance of the 10 BattleTags matches, on the 2 replays
inspected in detail, exactly the split first 5 = team 0 / last 5 = team 1 as
returned by `storm-stats` (which derives the team from `replay.details()`, not from the lobby).

In xxd, the structure around each record (repeated pattern `42 21 95 c9 2f` before/after
each BattleTag, variable block size ~90-110 bytes because of variable-length fields) shows
no byte clearly distinguishing the 5th from the 6th record (team boundary) from the
1st to the 2nd (same team) — the separation pattern is identical. It is therefore possible that the team
field exists elsewhere in the blob (bit-packed zone not explored byte by byte) without us having
located it. **Answer: observed order consistent with a grouping by team on 2 samples,
but no explicit field found → to handle with care in task 3 (do not bet only
on position without a fallback signal).**

### 4. Is the picked hero present? — the most important question

**Not found as a readable string, neither canonical nor internal, associated with the BattleTags.**
Active but negative search:

- None of the 10 hero names actually played in `silver-city-aram` (`Tassadar`, `Qhira`,
  `Lost Vikings`, `Li Li`, `Chromie`, `Varian`, `Mephisto`, `Jaina`, `Zarya`, `Rehgar`) appears
  in `strings -n 3 silver-city.bin` (case-insensitive search, canonical forms tested).
- No internal `HeroXxx` form matching these heroes appears either.
- Hero-sounding fragments do exist in the blob, but **match none
  of the heroes actually played** in this game — which suggests a generic catalogue
  (portraits/rewards), not the pick:
  - `DVA4`, `DVAJ`, `DVA9` (offsets ~659–666 in `strings`): no player of this game
    plays D.Va.
  - A separate list, at the end of the file (offset 0x43ddd–0x4444f), of 4-byte tokens like
    `Arts`, `Crus`, `FENX`, `Guld`, `HL--`, `Junk`, `Luci`, `Monk`, `Ragn`, `Sylv`, `Uthe`, `Zaga`
    — in a zone of fixed-size records (~64 bytes each, see xxd). None
    matches a plausible abbreviation of the 10 heroes played (`TASS`, `QHIR`, `LILI`, `CHRO`, `VARI`,
    `MEPH`, `JAIN`, `ZARY`, `REHG`: zero occurrences). Semantics of this zone: **unknown**
    (unverified hypothesis: catalogue of portraits/loot/announcer generic to the client, not
    linked to the game's picks).
- A zone containing many `Hero…` tokens (`HeroPOVT8`, `HeroGOLD`, `HeroLOOT`, `HeroICON`,
  `HeroLcns`, `HeroPORT`) was found (offset ~line 2385 in `strings`) but these are
  asset type suffixes (`ICON`, `PORT`rait, `LOOT`, `Lcns`=license) attached to the generic word
  `Hero`, **not hero names** — probably keys of the rewards/lobby
  UI structure, unrelated to the pick.
- **Note on the exhaustiveness of the search above:** the search for full names
  (first point) and the one for abbreviations in the fixed-record zone (third point) both
  remained negative, but a complementary raw substring search (`grep -a`,
  first 4 characters of each hero name, case-insensitive, over the whole blob —
  not limited to that zone) does find several of these prefixes in `silver-city.bin`:
  `Tass` (Tassadar, ×1, offset 0x8f50), `Vari` (Varian, ×1, offset 0x12b1), `Meph` (Mephisto, ×3,
  offsets 0x14be/0x5903/0x91d9), `Zary` (Zarya, ×3, offsets 0x15b2/0x59f7/0x92cd), `Chro`
  (Chromie, ×1, offset 0x97d1) and `Rehg` (Rehgar, ×1, offset 0x8f13). These offsets are scattered and
  with no obvious relation between them — notably, each `Zary` occurrence sits exactly 0xf4
  bytes after the matching `Meph` occurrence, which suggests a repeated record of
  fixed structure rather than an isolated pick. **Unconfirmed hypothesis:** noise from a
  binary zone/generic table (repetition of the same token at unrelated places, no occurrence
  attached to a BattleTag or to a usable length-prefix pattern), not a readable trace of the
  pick. This does not change the conclusion below: no **complete canonical form** of a
  hero name was found associated with a player.

**Honest conclusion: the picked hero cannot be found in clear text in this blob by a search
for known strings or patterns. Answer to the ARAM sub-question (does it match the hero actually
played, given the shuffle bug on attribute 4002 documented in `docs/STATUS.md`): not applicable,
since no "hero" candidate was identified in the blob to begin with — unknown.**
It is possible that the pick is encoded only as a bit-packed numeric ID (not
byte-aligned) requiring a complete structural decoding of the blob (outside the scope of this
investigation task) — **if so, the "hero visible live in the lobby" ergonomics is
not achieved with a simple string extraction, and task 3 will have to either decode the format bit
by bit, or give up showing the hero before the end of the game.**

### 5. Are the map and the mode present?

**Map: indirectly, via `.s2ma` cache paths, but no name in clear text.**
The very start of the blob contains 9 Battle.net cache paths pointing to `.s2ma` files
(maps) identified by content hash, for example:

```
C:\ProgramData\Blizzard Entertainment\Battle.net\Cache\1f\1b\1f1b228ddb1f72205cbfd44405528710
0b0f39959be816548162e4081ea85511.s2ma
```

None of these hashes resolves to a map name without an external mapping table (absent from
this blob and from the crate) — **unknown** whether one of these 9 files specifically matches Silver
City without decoding the hash. Complementary negative search: neither `"Silver City"`, nor `"ARAM"`, nor
any canonical map/mode name appears as ASCII (`strings`) or as UTF-16LE (dedicated
programmatic search) in the blob.

**Mode: not found.** Search for the known mode codes documented in `docs/STATUS.md`
(`50091`=Storm League, `50101`=ARAM, `-1`=Custom) as ASCII and as 32-bit little/big-endian integers:
no occurrence in the blob. **Answer: map and mode are not present in a
directly readable form; the map may be deducible via the `.s2ma` hash (requires an external
hash→map table, to check — unknown at this stage); the mode has no identified lead.**

### 6. Is the live blob bit-for-bit identical to the archived blob?

**Not tested — requires the gaming PC.** This comparison requires copying
`%TEMP%\Heroes of the Storm\TempWriteReplayP1\replay.server.battlelobby` during a game in
progress on the Windows gaming machine, unavailable for this task. To carry over to task 5 (or as soon
as access to the gaming PC is possible). By default, in the absence of this evidence, **the
replay↔lobby link will have to be made by set of BattleTags**, as planned by the spec's default
resolution — not by byte hash.

---

## Difficulties encountered

- The blob's structure is **not a fixed-size record format** in the BattleTag
  zone: the distance between two consecutive BattleTags varies (92, 101… bytes), because of
  preceding numeric fields encoded with variable length (most likely varints of
  Blizzard's versioned protocol). A reliable extraction in task 3 will therefore have to work by
  pattern search (length-prefix + `#` + digits) rather than by fixed offsets.
- `strings` (BSD, macOS) has no `-e`/`-el` option to force a UTF-16 or multi-byte UTF-8
  read — detecting the Cyrillic BattleTag required a direct programmatic search
  for the UTF-8 bytes in the file (Python), not the `strings` tool alone. To document for
  task 3: do not rely only on `strings`/ASCII grep for the BattleTag inventory.
- No dedicated `storm-stats` example for displaying heroes existed; the already present binary
  `crates/storm-stats/src/bin/storm-stats-dump.rs` (`storm-stats-dump <replay> <output.json>`) was
  reused as is (no modification) to obtain the reference truth
  players/heroes/teams used in this document — this reference comes from `replay.details()`
  (attribute 4002, potentially biased by the ARAM shuffle according to `docs/STATUS.md`), not from the
  lobby; it is only used to know *which strings to search for* in the blob, not as absolute truth
  about the hero actually played.

## What task 3 can safely retain

1. The BattleTags (`name#tag`) can be extracted from the blob with a pattern
   `[length byte in bytes][UTF-8 bytes]['#' separator][digits][\0]`, in lobby
   order (empirically = order by team on 2 samples).
2. Decode as UTF-8 over the given number of bytes, do not assume ASCII.
3. The complete toon handle is not present in clear text → link by BattleTag, not by
   toon handle, and not by explicit team (no confirmed field) as long as a team field has not
   been located more precisely.
4. The picked hero was not found in this blob by this method — **potential blocking point
   for the targeted ergonomics**, to handle explicitly in task 3 (either complete bit-packed decoding,
   or a product delivered without a visible hero before the end of the game, to be decided by the operator).
5. Map and mode cannot be used without deeper decoding or an external table.
