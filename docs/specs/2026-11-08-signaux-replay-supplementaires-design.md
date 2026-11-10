# Additional signals from replays — design

> Status: **approved by the operator on 2026-11-08** (order chat → leavers → cosmetics → level →
> keyboard). Chat plan: `docs/plans/2026-11-08-chat-habitudes.md`. The other batches will get their plan
> before being coded. Follows `2026-11-08-profils-hotkeys-comptes-design.md`.

## Intent

Six ideas were proposed after the hotkey profile. A probe on real replays (40 replays
picked at random from `Documents/Heroes of the Storm`, plus the box's corpus: 34,600
`match_players` rows, 3,452 games with messages) tells which ones hold up. The operator wants all of them;
chat is limited to **volume and habits**, with no toxicity judgement, shown on the player
page and in the scouting brief.

## Probe verdict

| # | Idea | Feasible? | Figures |
|---|---|---|---|
| 1 | Equipped cosmetics | **Yes** | skin/mount/announcer already in `match_players.data`: 1,628 skins, 555 mounts, 70 distinct announcers. Banner, spray, voiceLine, clanTag: empty in recent replays or often empty |
| 2 | Groups (premade) | **No** | `m_tandemLeaderUserId` = `null` on 636 slots, `m_teamsTogether` = `false` on 40/40. The players spec's "no group info" constraint remains true |
| 3 | Keyboard footprint | **Yes** | `STriggerKeyPressedEvent`: ~695,000 events over 40 replays (~17,000/replay); `SCmdEvent` ~12,000–17,000/replay |
| 4 | Leavers | **Yes** | `SGameUserLeaveEvent.m_leaveReason`: `0` (normal end, 155) and `11` (left mid-game, 8, all before the last 30 seconds) |
| 5 | Level | **Reduced** | `m_highestLeague` = 0 everywhere (unusable). What remains: `m_heroMasteryTiers` (tier 1–4 per hero) and `m_hasActiveBoost` |
| 6 | Chat habits | **Yes, no extraction** | chat is already in `matches.data.messages` (type 0 = chat: 44,191 messages; other types = pings, announcements) |

**Idea 2 is dropped**: the fields exist but are never filled in.

## Decisions

### A. Cosmetics as a linked-accounts signal (idea 1)
- "Cosmetics" signal in `similarity.rs`: overlap of skins/mounts/announcers **worn in
  game** by both accounts, weighted by the item's rarity (a cosmetic worn by many
  accounts proves nothing, same logic as the generic profile name).
- Strength: **medium**, never HIGH on its own. **[my choice]** Weights and thresholds tuned on a probe of the corpus.
- No migration: the data is already in `data`. Aggregate queries to be measured before adding an index.

### B. Leavers (idea 4)
- Per account: number of games, mid-game departures (`m_leaveReason` = 11 and before the end), rate.
- Storage: column `match_players.left_early BOOLEAN` (migration, `PARSER_VERSION` 4).
- Display: rate on the player page, mention in the scouting brief above a threshold
  **[my choice]** ≥ 3 departures and ≥ 5 % of games. A player cut off by the game (disconnection)
  cannot be told apart from a voluntary leave: the screen says so.

### C. Chat habits (idea 6)
- **Measurements only**: messages per game, team / all share (`recipient`), typical moment
  (early, mid, late), recurring words (list filtered of stop words). **No toxicity score,
  no ranking.**
- Computed at read time from `matches.data.messages` for a single account (no mass pre-computation);
  cache if the measurement exceeds the API budget (p95 < 100 ms).
- Display: "Chat" section on the player page; one line in the opponent's scouting brief.
  The texts are not exposed, only the measurements and the recurring words.

### D. Keyboard footprint (idea 3)
- Per account: most-used keys and their distribution (no chat content), action cadence
  (commands per minute) per hero.
- Weak linked-accounts signal; **[my choice]** displayed but not in the score until a probe has
  shown that it discriminates (compare confirmed linked accounts with unlinked accounts).
- Storage: aggregate per `(match, toon)` computed at ingestion in a dedicated JSONB column, not
  the raw event (17,000 events/replay).

### E. Level and progression (idea 5, reduced)
- `m_heroMasteryTiers`: mastery tier of the hero played, per game; `m_hasActiveBoost`: boolean.
- Display: hero tier on the player page. **[my choice]** no linked-accounts signal (a tier
  depends on play time, not on the person).

## Proposed order
1. C (chat): no extraction, immediate value. 2. B (leavers). 3. A (cosmetics). 4. E.
5. D (keyboard, to be measured first). Each batch has its plan, its PR, its doc update.

## Out of scope
- Toxicity, insults, AI classification of chat (ruled out by the operator).
- Groups/premade (idea 2): data missing.
- Exposing the text of messages.

## Open questions for the operator
1. Is the order C → B → A → E → D acceptable, or should an idea go first?
2. Threshold for mentioning leavers in scouting: ≥ 3 departures and ≥ 5 %?
3. Cosmetics: medium signal, never HIGH on its own?
4. Keyboard: displayed but outside the score until a probe justifies it?
