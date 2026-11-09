//! Pure reducer of the live feed: consumes the decoded events (tracker + game) of a game in
//! progress and maintains the state; produces the view filtered by visibility level. Zero I/O.
//! Spec: `docs/specs/2026-10-24-feed-live-en-partie-design.md` (§ Reducer, § Fair play).

use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value as J};
use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};
use std::sync::OnceLock;
use storm_replay::Value;

/// Game loops per second of game time.
pub const LOOPS_PER_SEC: i64 = 16;
/// Two presses of the same ability less than 0.3 s apart = a single one (the viewer's rule).
const ABILITY_DEDUP_LOOPS: i64 = 5;
/// Window of the ability timeline served to the front.
const ABILITY_WINDOW_LOOPS: i64 = 180 * LOOPS_PER_SEC;
/// Before concluding "a single human" (game against the AI), the game events must cover at
/// least this duration: in a 10-human game, all of them emit events (camera, selection)
/// well before — a first block may contain only one.
const SINGLE_HUMAN_MIN_LOOPS: i64 = 60 * LOOPS_PER_SEC;
/// Maximum size of the feed served to the front (the most recent ones).
const FEED_MAX: usize = 200;
/// Maximum size of the kept feed: beyond it, the oldest are dropped (a real game
/// produces a few hundred; bounds the memory against a client that would send anything).
const FEED_KEEP: usize = 2_000;

/// Visibility level chosen by the operator (spec § Fair play). `self` by default.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Scope {
    /// Scoreboard + your abilities.
    #[default]
    #[serde(rename = "self")]
    Me,
    /// + your allies' abilities.
    Allies,
    /// + the opponents' abilities.
    All,
}

/// "Tracker unit → canonical name" table (`HeroMonk` → `Kharazim`), derived from the archive
/// by `examples/derive_hero_units.rs`.
fn hero_units() -> &'static HashMap<String, String> {
    static T: OnceLock<HashMap<String, String>> = OnceLock::new();
    T.get_or_init(|| serde_json::from_str(include_str!("hero_units.json")).unwrap_or_default())
}

/// Canonical name of a hero unit; failing that, the unit name without `Hero` (never empty).
#[must_use]
pub fn hero_name(unit: &str) -> String {
    hero_units()
        .get(unit)
        .cloned()
        .unwrap_or_else(|| unit.strip_prefix("Hero").unwrap_or(unit).to_owned())
}

#[derive(Debug, Clone, Default)]
struct AbilityStat {
    count: u32,
    loops: Vec<i64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct TalentPick {
    pub tree_id: String,
    #[serde(rename = "loop")]
    pub at: i64,
}

#[derive(Debug, Clone, Default)]
struct PlayerLive {
    slot: Option<i64>,
    user_id: Option<i64>,
    hero_unit: Option<String>,
    level: i64,
    talents: Vec<TalentPick>,
    deaths: u32,
    takedowns: u32,
}

#[derive(Debug, Clone, Serialize)]
pub struct FeedItem {
    #[serde(rename = "loop")]
    pub at: i64,
    /// `death`, `talent`, `level`, `camp`, `objective`.
    pub kind: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub player: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub team: Option<u8>,
    pub detail: J,
}

/// Complete state of a game in progress, fed event by event.
#[derive(Debug, Clone, Default)]
pub struct LiveState {
    gates_loop: Option<i64>,
    tracker_loop: i64,
    game_loop: i64,
    /// Gameloop of the first game event received.
    game_first_loop: Option<i64>,
    /// `_userid`s that emitted at least one game event (all types: camera, selection, orders).
    game_users: BTreeSet<i64>,
    players: BTreeMap<i64, PlayerLive>,
    user_to_player: HashMap<i64, i64>,
    team_levels: [i64; 2],
    feed: VecDeque<FeedItem>,
    /// Abilities per `_userid` (game events can precede the `SPlayerSetupEvent`).
    abilities: BTreeMap<i64, BTreeMap<(i64, i64), AbilityStat>>,
}

/// Team of a tracker player: 1–5 → 0, 6–10 → 1.
#[must_use]
pub fn team_of(player: i64) -> u8 {
    u8::from(player > 5)
}

fn s(v: &Value, k: &str) -> Option<String> {
    v.field(k).and_then(Value::as_str_lossy)
}

fn int(v: &Value, k: &str) -> Option<i64> {
    v.field(k).and_then(Value::as_int)
}

/// Key/value pairs of an `m_intData` / `m_stringData` array of an `SStatGameEvent`.
fn kv(ev: &Value, field: &str) -> Vec<(String, J)> {
    ev.field(field)
        .and_then(Value::as_array)
        .unwrap_or(&[])
        .iter()
        .filter_map(|e| {
            let key = s(e, "m_key")?;
            let val = match e.field("m_value")? {
                Value::Int(n) => J::from(*n),
                other => J::from(other.as_str_lossy()?),
            };
            Some((key, val))
        })
        .collect()
}

fn first_int(pairs: &[(String, J)], key: &str) -> Option<i64> {
    pairs
        .iter()
        .find(|(k, _)| k == key)
        .and_then(|(_, v)| v.as_i64())
}

fn first_str(pairs: &[(String, J)], key: &str) -> Option<String> {
    pairs
        .iter()
        .find(|(k, _)| k == key)
        .and_then(|(_, v)| v.as_str().map(str::to_owned))
}

/// `SStatGameEvent`s that do not go into the feed: noise (globes, periodic XP) or already handled.
fn feed_ignored(name: &str) -> bool {
    name.starts_with("EndOfGame")
        || matches!(
            name,
            "PeriodicXPBreakdown"
                | "RegenGlobePickedUp"
                | "PlayerInit"
                | "PlayerSpawned"
                | "GatesOpen"
                | "LevelUp"
                | "TalentChosen"
                | "PlayerDeath"
                | "JungleCampCapture"
        )
}

impl LiveState {
    /// Applies a tracker event.
    pub fn apply_tracker(&mut self, ev: &Value) {
        let at = int(ev, "_gameloop").unwrap_or(0);
        self.tracker_loop = self.tracker_loop.max(at);
        let name = s(ev, "_event").unwrap_or_default();
        if name.ends_with("SPlayerSetupEvent") {
            let Some(player) = int(ev, "m_playerId") else {
                return;
            };
            let p = self.players.entry(player).or_default();
            p.slot = int(ev, "m_slotId");
            p.user_id = int(ev, "m_userId");
            if let Some(u) = p.user_id {
                self.user_to_player.insert(u, player);
            }
        } else if name.ends_with("SStatGameEvent") {
            self.apply_stat(ev, at);
        }
    }

    fn apply_stat(&mut self, ev: &Value, at: i64) {
        let name = s(ev, "m_eventName").unwrap_or_default();
        let ints = kv(ev, "m_intData");
        let strs = kv(ev, "m_stringData");
        match name.as_str() {
            "GatesOpen" => self.gates_loop = Some(at),
            "PlayerSpawned" => {
                if let (Some(p), Some(h)) = (first_int(&ints, "PlayerID"), first_str(&strs, "Hero"))
                {
                    self.players.entry(p).or_default().hero_unit = Some(h);
                }
            }
            "LevelUp" => {
                let (Some(p), Some(level)) =
                    (first_int(&ints, "PlayerID"), first_int(&ints, "Level"))
                else {
                    return;
                };
                self.players.entry(p).or_default().level = level;
                let team = team_of(p);
                let t = &mut self.team_levels[usize::from(team)];
                if level > *t {
                    *t = level;
                    if level > 1 {
                        self.push_feed(at, "level", None, Some(team), json!({ "level": level }));
                    }
                }
            }
            "TalentChosen" => {
                let (Some(p), Some(tree_id)) = (
                    first_int(&ints, "PlayerID"),
                    first_str(&strs, "PurchaseName"),
                ) else {
                    return;
                };
                self.players.entry(p).or_default().talents.push(TalentPick {
                    tree_id: tree_id.clone(),
                    at,
                });
                self.push_feed(
                    at,
                    "talent",
                    Some(p),
                    Some(team_of(p)),
                    json!({ "tree_id": tree_id }),
                );
            }
            "PlayerDeath" => {
                let Some(victim) = first_int(&ints, "PlayerID") else {
                    return;
                };
                let killers: BTreeSet<i64> = ints
                    .iter()
                    .filter(|(k, _)| k == "KillingPlayer")
                    .filter_map(|(_, v)| v.as_i64())
                    .collect();
                self.players.entry(victim).or_default().deaths += 1;
                for k in &killers {
                    self.players.entry(*k).or_default().takedowns += 1;
                }
                self.push_feed(
                    at,
                    "death",
                    Some(victim),
                    Some(team_of(victim)),
                    json!({ "killers": killers }),
                );
            }
            "JungleCampCapture" => {
                let team = first_int(&ints, "TeamID")
                    .and_then(|t| u8::try_from(t - 1).ok())
                    .filter(|t| *t <= 1);
                let camp = first_str(&strs, "CampType").unwrap_or_default();
                self.push_feed(at, "camp", None, team, json!({ "camp": camp }));
            }
            other if !feed_ignored(other) => {
                let mut detail = Map::new();
                detail.insert("name".into(), J::from(other));
                for (k, v) in ints.into_iter().chain(strs) {
                    detail.insert(k, v);
                }
                self.push_feed(at, "objective", None, None, J::Object(detail));
            }
            _ => {}
        }
    }

    fn push_feed(
        &mut self,
        at: i64,
        kind: &'static str,
        player: Option<i64>,
        team: Option<u8>,
        detail: J,
    ) {
        if self.feed.len() >= FEED_KEEP {
            self.feed.pop_front();
        }
        self.feed.push_back(FeedItem {
            at,
            kind,
            player,
            team,
            detail,
        });
    }

    /// Applies a game event (only the ability orders interest us).
    pub fn apply_game(&mut self, ev: &Value) {
        let at = int(ev, "_gameloop").unwrap_or(0);
        self.game_loop = self.game_loop.max(at);
        self.game_first_loop.get_or_insert(at);
        if let Some(u) = ev.field("_userid").and_then(Value::first_field_int) {
            self.game_users.insert(u);
        }
        if !s(ev, "_event").is_some_and(|n| n.ends_with("SCmdEvent")) {
            return;
        }
        let Some(abil) = ev.field("m_abil").filter(|a| !matches!(a, Value::Null)) else {
            return;
        };
        let (Some(link), Some(cmd)) = (int(abil, "m_abilLink"), int(abil, "m_abilCmdIndex")) else {
            return;
        };
        let Some(user) = ev.field("_userid").and_then(Value::first_field_int) else {
            return;
        };
        let stat = self
            .abilities
            .entry(user)
            .or_default()
            .entry((link, cmd))
            .or_default();
        if stat
            .loops
            .last()
            .is_some_and(|&last| at - last < ABILITY_DEDUP_LOOPS)
        {
            return;
        }
        stat.count += 1;
        stat.loops.push(at);
        // Only the recent window is served: the rest is dropped (the counter, however, is total).
        let keep_from = at - ABILITY_WINDOW_LOOPS;
        if stat.loops.first().is_some_and(|&l| l < keep_from) {
            stat.loops.retain(|&l| l >= keep_from);
        }
    }

    /// Determines the operator. An undetermined "me" is safe (no ability shown in `self`);
    /// a wrong "me" would show another player's — hence careful rules:
    /// 1. `explicit` if it designates a known player;
    /// 2. the operator's slot in the lobby **of this game** (the caller passes the lobby
    ///    only if its hash is the session id);
    /// 3. if the lobby counts several humans: nothing;
    /// 4. otherwise, the only human player who emitted a game event, provided that the game events
    ///    cover at least one minute (game against the AI).
    #[must_use]
    pub fn resolve_me(
        &self,
        explicit: Option<i64>,
        lobby_slot: Option<i64>,
        lobby_humans: Option<usize>,
    ) -> (Option<i64>, Option<&'static str>) {
        if let Some(p) = explicit.filter(|p| self.players.contains_key(p)) {
            return (Some(p), Some("explicit"));
        }
        if let Some(slot) = lobby_slot {
            if let Some((p, _)) = self.players.iter().find(|(_, pl)| pl.slot == Some(slot)) {
                return (Some(*p), Some("lobby"));
            }
        }
        if lobby_humans.is_some_and(|n| n > 1) {
            return (None, None);
        }
        let covered = self
            .game_first_loop
            .is_some_and(|first| self.game_loop - first >= SINGLE_HUMAN_MIN_LOOPS);
        let humans: Vec<i64> = self
            .game_users
            .iter()
            .filter_map(|u| self.user_to_player.get(u).copied())
            .filter(|p| (1..=10).contains(p))
            .collect();
        if let ([only], true) = (&humans[..], covered) {
            return (Some(*only), Some("single_human"));
        }
        (None, None)
    }

    /// View served to the front, filtered by level: the abilities outside the level are never
    /// serialised (`abilities: null` = hidden, `[]` = visible but none).
    #[must_use]
    pub fn view(&self, scope: Scope, me: Option<i64>) -> J {
        let visible = |player: i64| -> bool {
            match (scope, me) {
                (Scope::All, _) => true,
                (Scope::Allies, Some(m)) => team_of(player) == team_of(m),
                (Scope::Me, Some(m)) => player == m,
                (_, None) => false,
            }
        };
        let since = self.game_loop - ABILITY_WINDOW_LOOPS;
        let players: Vec<J> = self
            .players
            .iter()
            .filter(|(id, _)| (1..=10).contains(*id))
            .map(|(id, p)| {
                let abilities = if visible(*id) {
                    let stats = p.user_id.and_then(|u| self.abilities.get(&u));
                    J::Array(
                        stats
                            .into_iter()
                            .flatten()
                            .map(|((link, cmd), st)| {
                                json!({
                                    "link": link,
                                    "cmd": cmd,
                                    "count": st.count,
                                    "recent": st.loops.iter().filter(|&&l| l >= since).collect::<Vec<_>>(),
                                })
                            })
                            .collect(),
                    )
                } else {
                    J::Null
                };
                json!({
                    "player_id": id,
                    "team": team_of(*id),
                    "hero": p.hero_unit.as_deref().map(hero_name),
                    "hero_unit": p.hero_unit,
                    "level": p.level,
                    "talents": p.talents,
                    "deaths": p.deaths,
                    "takedowns": p.takedowns,
                    "abilities": abilities,
                })
            })
            .collect();
        let feed: Vec<&FeedItem> = self.feed.iter().rev().take(FEED_MAX).collect();
        json!({
            "scope": scope,
            "me": me,
            "gates_loop": self.gates_loop,
            "tracker_loop": self.tracker_loop,
            "game_loop": self.game_loop,
            "team_levels": self.team_levels,
            "players": players,
            "feed": feed,
        })
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;
    use std::path::PathBuf;
    use storm_replay::{Replay, StreamDecoder, StreamKind};

    fn corpus() -> Vec<PathBuf> {
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../storm-replay/tests/data");
        let mut v: Vec<PathBuf> = std::fs::read_dir(dir)
            .unwrap()
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.extension().is_some_and(|x| x == "StormReplay"))
            .collect();
        v.sort();
        v
    }

    /// Replays a replay in 4 KB blocks, as the live feed receives them.
    fn replay_live(path: &PathBuf) -> LiveState {
        let r = Replay::open(path).unwrap();
        let mut st = LiveState::default();
        for (kind, raw) in [
            (StreamKind::Tracker, r.tracker_events_raw().unwrap()),
            (StreamKind::Game, r.game_events_raw().unwrap()),
        ] {
            let mut dec = StreamDecoder::new(kind, Some(r.header.base_build)).unwrap();
            for chunk in raw.chunks(4096) {
                for ev in dec.feed(chunk).unwrap() {
                    match kind {
                        StreamKind::Tracker => st.apply_tracker(&ev),
                        StreamKind::Game => st.apply_game(&ev),
                    }
                }
            }
        }
        st
    }

    /// Tracker `PlayerInit`: player id → toon handle (key of the storm-stats players).
    fn handles(path: &PathBuf) -> HashMap<i64, String> {
        let r = Replay::open(path).unwrap();
        let mut m = HashMap::new();
        for ev in r.tracker_events().unwrap() {
            if s(&ev, "m_eventName").as_deref() != Some("PlayerInit") {
                continue;
            }
            let ints = kv(&ev, "m_intData");
            let strs = kv(&ev, "m_stringData");
            if let (Some((_, id)), Some((_, handle))) = (ints.first(), strs.get(1)) {
                m.insert(id.as_i64().unwrap(), handle.as_str().unwrap().to_owned());
            }
        }
        m
    }

    #[test]
    fn final_live_state_matches_full_parse() {
        for path in corpus() {
            let name = path.file_name().unwrap().to_string_lossy().into_owned();
            let live = replay_live(&path);
            let view = live.view(Scope::All, None);
            let live_players = view["players"].as_array().unwrap();
            assert_eq!(live_players.len(), 10, "{name}");

            // Heroes: compared to `details` (the hero really played). storm-stats' attribute 4002
            // carries the lobby choice, wrong in ARAM — hence the pairing by id.
            let details = Replay::open(&path).unwrap().details().unwrap();
            for lp in live_players {
                let id = lp["player_id"].as_i64().unwrap();
                let slot = live.players[&id].slot;
                let d = details
                    .players
                    .iter()
                    .find(|d| d.working_set_slot_id == slot)
                    .unwrap_or_else(|| panic!("{name}: P{id} without a details player"));
                if d.hero.is_ascii() {
                    assert_eq!(lp["hero"], d.hero.as_str(), "{name} P{id}: hero");
                }
            }

            // Deaths, takedowns, talents: compared to storm-stats, player by player.
            let out = storm_stats::process_replay(&path, &name);
            let players = out.players.expect("storm-stats without players");
            let handles = handles(&path);
            for lp in live_players {
                let id = lp["player_id"].as_i64().unwrap();
                let pdoc = &players[&handles[&id]];
                assert_eq!(
                    lp["deaths"].as_f64(),
                    pdoc["gameStats"]["Deaths"].as_f64(),
                    "{name} P{id}: morts"
                );
                assert_eq!(
                    lp["takedowns"].as_f64(),
                    pdoc["gameStats"]["Takedowns"].as_f64(),
                    "{name} P{id}: takedowns"
                );
                let mut want: Vec<&str> = pdoc["talents"]
                    .as_object()
                    .unwrap()
                    .values()
                    .filter_map(J::as_str)
                    .collect();
                let mut got: Vec<&str> = lp["talents"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .filter_map(|t| t["tree_id"].as_str())
                    .collect();
                want.sort_unstable();
                got.sort_unstable();
                assert_eq!(got, want, "{name} P{id}: talents");
            }
        }
    }

    #[test]
    fn scope_filters_abilities() {
        let live = replay_live(&corpus()[0]);
        let me = 3;
        let shown = |scope| -> Vec<i64> {
            live.view(scope, Some(me))["players"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|p| !p["abilities"].is_null())
                .map(|p| p["player_id"].as_i64().unwrap())
                .collect()
        };
        assert_eq!(shown(Scope::Me), vec![3]);
        assert_eq!(shown(Scope::Allies), vec![1, 2, 3, 4, 5]);
        assert_eq!(shown(Scope::All), (1..=10).collect::<Vec<_>>());
        // Without a known operator, only `all` shows abilities.
        assert!(live.view(Scope::Allies, None)["players"]
            .as_array()
            .unwrap()
            .iter()
            .all(|p| p["abilities"].is_null()));
    }

    #[test]
    fn abilities_are_recorded_for_humans() {
        let live = replay_live(&corpus()[0]);
        let view = live.view(Scope::All, None);
        let with_presses = view["players"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|p| p["abilities"].as_array().is_some_and(|a| !a.is_empty()))
            .count();
        assert_eq!(with_presses, 10, "10 humans → 10 players with abilities");
    }

    #[test]
    fn hero_names_resolve() {
        assert_eq!(hero_name("HeroMonk"), "Kharazim");
        assert_eq!(hero_name("HeroFaerieDragon"), "Brightwing");
        assert_eq!(hero_name("HeroNotInTable"), "NotInTable");
    }

    fn one_human_state(game_span_loops: i64) -> LiveState {
        let mut st = LiveState::default();
        for (player, slot, user) in [(1, 0, Some(0)), (2, 1, None)] {
            st.players.insert(
                player,
                PlayerLive {
                    slot: Some(slot),
                    user_id: user,
                    ..Default::default()
                },
            );
        }
        st.user_to_player.insert(0, 1);
        st.game_users.insert(0);
        st.game_first_loop = Some(0);
        st.game_loop = game_span_loops;
        st
    }

    #[test]
    fn resolve_me_rules() {
        let st = one_human_state(SINGLE_HUMAN_MIN_LOOPS);
        assert_eq!(
            st.resolve_me(None, None, None),
            (Some(1), Some("single_human"))
        );
        assert_eq!(
            st.resolve_me(None, None, Some(1)),
            (Some(1), Some("single_human"))
        );
        assert_eq!(st.resolve_me(None, Some(1), None), (Some(2), Some("lobby")));
        assert_eq!(
            st.resolve_me(Some(2), Some(0), None),
            (Some(2), Some("explicit"))
        );
        // Lobby with several humans without an operator slot: never a deduction.
        assert_eq!(st.resolve_me(None, None, Some(10)), (None, None));
        // Less than a minute of game events: too early to conclude "a single human".
        let early = one_human_state(SINGLE_HUMAN_MIN_LOOPS - 1);
        assert_eq!(early.resolve_me(None, None, None), (None, None));
    }

    #[test]
    fn first_game_block_of_a_10_human_game_never_guesses_me() {
        let r = Replay::open(&corpus()[0]).unwrap();
        let mut st = LiveState::default();
        let mut tdec = StreamDecoder::new(StreamKind::Tracker, Some(r.header.base_build)).unwrap();
        for ev in tdec.feed(&r.tracker_events_raw().unwrap()).unwrap() {
            st.apply_tracker(&ev);
        }
        let game = r.game_events_raw().unwrap();
        let mut gdec = StreamDecoder::new(StreamKind::Game, Some(r.header.base_build)).unwrap();
        for (n, block) in game.chunks(4096).enumerate() {
            for ev in gdec.feed(block).unwrap() {
                st.apply_game(&ev);
            }
            // At no point of the game must the "a single human" deduction trigger.
            assert_eq!(st.resolve_me(None, None, None), (None, None), "bloc {n}");
        }
    }
}
