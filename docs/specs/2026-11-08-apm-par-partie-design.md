# APM per game and per player — design

> Status: **requested by the operator on 2026-11-08** ("add APM for each game for each
> player, and an average per player as a stat"). Narrows batch D "keyboard footprint" of
> `2026-11-08-signaux-replay-supplementaires-design.md` down to what was asked: APM. The choices
> marked **[my choice]** remain to be confirmed.

## Findings (checked on 2026-11-08 on a real replay, 21.6 min, 10 players)
The `game` stream contains `SCmdEvent` (~26,000, with `_userid`): moves (right click,
null `m_abil`), attacks, abilities. Per player: 1,300 to 4,400 `SCmdEvent`, i.e. 60 to 200 per minute.
`STriggerKeyPressedEvent` (keys) repeats held keys, `SCmdUpdateTargetPointEvent` follows the
mouse during an order: neither of them measures actions.

## Decisions
1. **Definition [my choice]**: APM = number of the player's `SCmdEvent` from the start of the game
   (`loop_game_start`) ÷ game duration in minutes (`matches.length`). A player who left before the end
   is therefore under-estimated; the screen does not correct it. Heroes of the Storm has no official APM: the value
   is only comparable between games of this system.
2. **Storage**: `match_players.apm` (real), computed at ingestion (`PARSER_VERSION` 4). NULL = not
   re-processed or unknown duration.
3. **Average per player**: mean of the account's non-NULL `apm`, rounded to an integer; `apm_games` says
   how many games it rests on. No weighting by hero and no duration filter **[my choice]**.
4. **Display**: "Avg APM" tile on the player page, APM pip on each recent game, "APM" column
   in the match scoreboard.
5. **Outside the linked-accounts score**: an APM depends on the hero and the game; not an identity signal.

## Out of scope
Most-used keys, cadence per hero, keyboard footprint (the rest of batch D).
