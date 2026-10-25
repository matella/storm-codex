//! Réducteur pur du feed live : consomme les événements décodés (tracker + game) d'une partie en
//! cours et maintient l'état ; produit la vue filtrée par niveau de visibilité. Zéro I/O.
//! Spec : `docs/specs/2026-10-24-feed-live-en-partie-design.md` (§ Réducteur, § Fair-play).

use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value as J};
use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};
use std::sync::OnceLock;
use storm_replay::Value;

/// Game loops par seconde de jeu.
pub const LOOPS_PER_SEC: i64 = 16;
/// Deux pressions de la même aptitude à moins de 0,3 s = une seule (règle de la visionneuse).
const ABILITY_DEDUP_LOOPS: i64 = 5;
/// Fenêtre de la timeline d'aptitudes servie au front.
const ABILITY_WINDOW_LOOPS: i64 = 180 * LOOPS_PER_SEC;
/// Avant de conclure « un seul humain » (partie contre l'IA), les game events doivent couvrir au
/// moins cette durée : en partie à 10 humains, tous émettent des événements (caméra, sélection)
/// bien avant — un premier bloc peut n'en contenir qu'un seul.
const SINGLE_HUMAN_MIN_LOOPS: i64 = 60 * LOOPS_PER_SEC;
/// Taille maximale du fil servi au front (les plus récents).
const FEED_MAX: usize = 200;
/// Taille maximale du fil conservé : au-delà, les plus anciens sont jetés (une partie réelle en
/// produit quelques centaines ; borne la mémoire face à un client qui enverrait n'importe quoi).
const FEED_KEEP: usize = 2_000;

/// Niveau de visibilité choisi par l'opérateur (spec § Fair-play). `self` par défaut.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Scope {
    /// Tableau des scores + tes aptitudes.
    #[default]
    #[serde(rename = "self")]
    Me,
    /// + les aptitudes de tes alliés.
    Allies,
    /// + les aptitudes des adversaires.
    All,
}

/// Table « unité du tracker → nom canonique » (`HeroMonk` → `Kharazim`), dérivée de l'archive
/// par `examples/derive_hero_units.rs`.
fn hero_units() -> &'static HashMap<String, String> {
    static T: OnceLock<HashMap<String, String>> = OnceLock::new();
    T.get_or_init(|| serde_json::from_str(include_str!("hero_units.json")).unwrap_or_default())
}

/// Nom canonique d'une unité de héros ; à défaut, le nom d'unité sans `Hero` (jamais vide).
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

/// État complet d'une partie en cours, alimenté événement par événement.
#[derive(Debug, Clone, Default)]
pub struct LiveState {
    gates_loop: Option<i64>,
    tracker_loop: i64,
    game_loop: i64,
    /// Gameloop du premier game event reçu.
    game_first_loop: Option<i64>,
    /// `_userid` ayant émis au moins un game event (tous types : caméra, sélection, ordres).
    game_users: BTreeSet<i64>,
    players: BTreeMap<i64, PlayerLive>,
    user_to_player: HashMap<i64, i64>,
    team_levels: [i64; 2],
    feed: VecDeque<FeedItem>,
    /// Aptitudes par `_userid` (les game events peuvent précéder le `SPlayerSetupEvent`).
    abilities: BTreeMap<i64, BTreeMap<(i64, i64), AbilityStat>>,
}

/// Équipe d'un joueur du tracker : 1–5 → 0, 6–10 → 1.
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

/// Paires clé/valeur d'un tableau `m_intData` / `m_stringData` d'un `SStatGameEvent`.
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

/// `SStatGameEvent` qui ne vont pas dans le fil : bruit (globes, XP périodique) ou déjà traités.
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
    /// Applique un événement du tracker.
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

    /// Applique un événement game (seuls les ordres d'aptitude nous intéressent).
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
        // Seule la fenêtre récente est servie : on jette le reste (le compteur, lui, est total).
        let keep_from = at - ABILITY_WINDOW_LOOPS;
        if stat.loops.first().is_some_and(|&l| l < keep_from) {
            stat.loops.retain(|&l| l >= keep_from);
        }
    }

    /// Détermine l'opérateur. Un « me » indéterminé est sûr (aucune aptitude affichée en `self`) ;
    /// un « me » faux montrerait celles d'un autre joueur — d'où des règles prudentes :
    /// 1. `explicit` s'il désigne un joueur connu ;
    /// 2. le slot de l'opérateur dans le lobby **de cette partie** (l'appelant ne passe le lobby
    ///    que si son hash est l'id de session) ;
    /// 3. si le lobby compte plusieurs humains : rien ;
    /// 4. sinon, l'unique joueur humain ayant émis un game event, à condition que les game events
    ///    couvrent au moins une minute (partie contre l'IA).
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

    /// Vue servie au front, filtrée par niveau : les aptitudes hors niveau ne sont jamais
    /// sérialisées (`abilities: null` = masquées, `[]` = visibles mais aucune).
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

    /// Rejoue un replay par blocs de 4 Ko, comme le feed live les reçoit.
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

    /// `PlayerInit` du tracker : id joueur → toon handle (clé des joueurs de storm-stats).
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

            // Héros : comparés à `details` (le héros réellement joué). L'attribut 4002 de
            // storm-stats porte le choix de lobby, faux en ARAM — d'où l'appariement par id.
            let details = Replay::open(&path).unwrap().details().unwrap();
            for lp in live_players {
                let id = lp["player_id"].as_i64().unwrap();
                let slot = live.players[&id].slot;
                let d = details
                    .players
                    .iter()
                    .find(|d| d.working_set_slot_id == slot)
                    .unwrap_or_else(|| panic!("{name}: P{id} sans joueur details"));
                if d.hero.is_ascii() {
                    assert_eq!(lp["hero"], d.hero.as_str(), "{name} P{id}: héros");
                }
            }

            // Morts, takedowns, talents : comparés à storm-stats, joueur par joueur.
            let out = storm_stats::process_replay(&path, &name);
            let players = out.players.expect("storm-stats sans joueurs");
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
        // Sans opérateur connu, seul `all` montre des aptitudes.
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
        assert_eq!(
            with_presses, 10,
            "10 humains → 10 joueurs avec des aptitudes"
        );
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
        // Lobby à plusieurs humains sans slot opérateur : jamais de déduction.
        assert_eq!(st.resolve_me(None, None, Some(10)), (None, None));
        // Moins d'une minute de game events : trop tôt pour conclure « un seul humain ».
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
            // À aucun moment de la partie la déduction « un seul humain » ne doit se déclencher.
            assert_eq!(st.resolve_me(None, None, None), (None, None), "bloc {n}");
        }
    }
}
