//! Faits de scouting (spec volet C) : calculés en Rust, jamais par le LLM. Module pur — entrée =
//! résumés de parties dont le côté cible est connu, sortie = sections structurées + `index` plat
//! `id → valeur affichable`. Chaque fait porte son effectif `n` ; `low` = `n < 3`.
//!
//! Tout est vu **depuis l'équipe cible** : « bans » = bans qu'elle fait, « bans subis » = bans de
//! ses adversaires, écart de niveau > 0 = la cible est devant.

use super::summary::{BanPhase, GameSummary, Player};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};

/// Effectif sous lequel un fait est « faible échantillon » (le prompt interdit d'en conclure).
pub const LOW_SAMPLE: u32 = 3;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rate {
    pub k: u32,
    pub n: u32,
}

impl Rate {
    pub fn pct(self) -> Option<f64> {
        (self.n > 0).then(|| f64::from(self.k) * 100.0 / f64::from(self.n))
    }
    fn text(self) -> String {
        match self.pct() {
            Some(p) => format!("{}/{} ({:.0}%)", self.k, self.n, p),
            None => "0/0".into(),
        }
    }
    fn add(&mut self, hit: bool) {
        self.n += 1;
        if hit {
            self.k += 1;
        }
    }
}

/// Intervalle de Wilson à 95 % (en %) — un 3-0 sur une carte ne dit presque rien, l'intervalle le
/// montre.
pub fn wilson(r: Rate) -> Option<(f64, f64)> {
    if r.n == 0 {
        return None;
    }
    let (n, z) = (f64::from(r.n), 1.96_f64);
    let p = f64::from(r.k) / n;
    let den = 1.0 + z * z / n;
    let centre = p + z * z / (2.0 * n);
    let marge = z * (p * (1.0 - p) / n + z * z / (4.0 * n * n)).sqrt();
    Some((
        ((centre - marge) / den * 100.0).max(0.0),
        ((centre + marge) / den * 100.0).min(100.0),
    ))
}

/// Identifiant stable pour les ids de faits : `Braxis Holdout` → `braxis_holdout`,
/// `Anub'arak` → `anubarak`, `E.T.C.` → `etc`, `Sgt. Hammer` → `sgt_hammer`.
pub fn slug(s: &str) -> String {
    let mut out = String::new();
    for c in s.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        } else if c == '\'' || c == '.' {
            continue;
        } else if !out.ends_with('_') && !out.is_empty() {
            out.push('_');
        }
    }
    out.trim_end_matches('_').to_string()
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FactRef {
    pub label: String,
    pub text: String,
    pub n: u32,
    pub low: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Overview {
    pub games: u32,
    pub record: Rate,
    pub excluded: u32,
    pub first_date: Option<String>,
    pub last_date: Option<String>,
    pub builds: Vec<i64>,
    pub avg_length_s: f64,
}

/// Un héros compté : `picks` = parties où il est pris (sur l'effectif de la section),
/// `record` = victoires quand il est pris.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HeroRow {
    pub id: String,
    pub hero: String,
    pub picks: Rate,
    pub record: Rate,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Count {
    pub id: String,
    pub key: String,
    pub count: Rate,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct AvgStats {
    pub kills: f64,
    pub deaths: f64,
    pub assists: f64,
    pub kill_participation_pct: f64,
    pub hero_damage_pm: f64,
    pub siege_damage_pm: f64,
    pub healing_pm: f64,
    pub damage_taken_pm: f64,
    pub xp_pm: f64,
    pub time_dead_pct: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PlayerFacts {
    pub pid: String,
    pub toon: String,
    pub name: String,
    /// Dans le roster (sinon remplaçant).
    pub core: bool,
    pub record: Rate,
    pub heroes: Vec<HeroRow>,
    pub roles: Vec<Count>,
    pub stats: AvgStats,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MapFacts {
    pub id: String,
    pub map: String,
    pub record: Rate,
    pub wilson: Option<(f64, f64)>,
    pub picks: Vec<HeroRow>,
    pub bans: Vec<Count>,
    pub bans_against: Vec<Count>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct DraftFacts {
    /// Parties avec une draft exploitable (bans ou 5 picks).
    pub games: u32,
    pub first_pick: Rate,
    pub first_pick_record: Rate,
    pub second_pick_record: Rate,
    pub bans_first: Vec<Count>,
    pub bans_mid: Vec<Count>,
    pub bans_against: Vec<Count>,
    pub openers: Vec<Count>,
    pub opener_roles: Vec<Count>,
    pub last_picks: Vec<Count>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LevelDiffAt {
    pub id: String,
    pub minute: u32,
    pub avg: f64,
    pub n: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Bucket {
    pub id: String,
    pub label: String,
    pub record: Rate,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct FlowFacts {
    /// La cible atteint le niveau 10 la première (sur les parties où quelqu'un l'atteint).
    pub first_to_10: Rate,
    pub first_to_10_record: Rate,
    pub first_fort: Rate,
    pub first_fort_record: Rate,
    pub first_objective: Rate,
    pub first_objective_record: Rate,
    pub level_diff: Vec<LevelDiffAt>,
    pub length: Vec<Bucket>,
    /// Victoires après avoir été menée de 2 niveaux ou plus (sur les parties où c'est arrivé).
    pub comebacks: Rate,
    /// Défaites après avoir mené de 2 niveaux ou plus (sur les parties où c'est arrivé).
    pub throws: Rate,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GamePick {
    pub hero: String,
    pub player: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GameRow {
    pub id: String,
    pub gid: i64,
    pub date: Option<String>,
    pub map: String,
    pub won: bool,
    pub length_s: f64,
    pub first_pick: Option<bool>,
    pub opponents: Vec<String>,
    pub picks: Vec<GamePick>,
    pub opp_picks: Vec<GamePick>,
    pub bans: Vec<String>,
    pub bans_against: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Facts {
    pub overview: Overview,
    pub players: Vec<PlayerFacts>,
    pub maps: Vec<MapFacts>,
    pub draft: DraftFacts,
    pub flow: FlowFacts,
    pub games: Vec<GameRow>,
    pub index: BTreeMap<String, FactRef>,
}

/// Une partie dont le côté cible est connu.
pub struct GameInput<'a> {
    pub gid: i64,
    pub summary: &'a GameSummary,
    pub side: u8,
}

/// Comptage générique « clé → Rate » trié (effectif décroissant, puis clé).
#[derive(Default)]
struct Tally(BTreeMap<String, (u32, u32)>); // clé → (occurrences, victoires)

impl Tally {
    fn hit(&mut self, key: &str, won: bool) {
        let e = self.0.entry(key.to_string()).or_default();
        e.0 += 1;
        if won {
            e.1 += 1;
        }
    }
    fn sorted(&self) -> Vec<(&String, u32, u32)> {
        let mut v: Vec<_> = self.0.iter().map(|(k, (n, w))| (k, *n, *w)).collect();
        v.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(b.0)));
        v
    }
    fn counts(&self, prefix: &str, of: u32) -> Vec<Count> {
        self.sorted()
            .into_iter()
            .map(|(k, n, _)| Count {
                id: format!("{prefix}.{}", slug(k)),
                key: k.clone(),
                count: Rate { k: n, n: of },
            })
            .collect()
    }
    fn heroes(&self, prefix: &str, of: u32) -> Vec<HeroRow> {
        self.sorted()
            .into_iter()
            .map(|(k, n, w)| HeroRow {
                id: format!("{prefix}.{}", slug(k)),
                hero: k.clone(),
                picks: Rate { k: n, n: of },
                record: Rate { k: w, n },
            })
            .collect()
    }
}

/// Écart de niveau (point de vue cible) à l'instant `t`, si la partie dure jusque-là.
fn diff_at(g: &GameSummary, side: u8, t: f64) -> Option<f64> {
    if g.length_s < t {
        return None;
    }
    let seg = g.level_diff.iter().find(|s| s.start <= t && t < s.end)?;
    Some(if side == 0 { seg.diff } else { -seg.diff })
}

fn role_of<'a>(roles: &'a HashMap<String, String>, hero: &str) -> &'a str {
    roles.get(&slug(hero)).map(String::as_str).unwrap_or("Unknown")
}

fn mine<'a>(g: &GameInput<'a>) -> &'a [Player] {
    &g.summary.teams[g.side as usize]
}
fn theirs<'a>(g: &GameInput<'a>) -> &'a [Player] {
    &g.summary.teams[1 - g.side as usize]
}

/// `roles` : slug du héros → rôle (référentiel `dim_heroes`).
pub fn compute(
    games: &[GameInput],
    roster: &[String],
    excluded: u32,
    roles: &HashMap<String, String>,
) -> Facts {
    let n_games = games.len() as u32;
    let won = |g: &GameInput| g.summary.winner == Some(g.side);

    // ── vue d'ensemble ──
    let mut record = Rate::default();
    let mut dates: Vec<&str> = Vec::new();
    let mut builds: Vec<i64> = Vec::new();
    let mut total_len = 0.0;
    for g in games {
        record.add(won(g));
        if let Some(d) = &g.summary.date {
            dates.push(d);
        }
        if let Some(b) = g.summary.build {
            if !builds.contains(&b) {
                builds.push(b);
            }
        }
        total_len += g.summary.length_s;
    }
    dates.sort_unstable();
    builds.sort_unstable();
    let overview = Overview {
        games: n_games,
        record,
        excluded,
        first_date: dates.first().map(|s| s.to_string()),
        last_date: dates.last().map(|s| s.to_string()),
        builds,
        avg_length_s: if n_games > 0 { total_len / f64::from(n_games) } else { 0.0 },
    };

    // ── joueurs : p1…pN, roster d'abord puis remplaçants, par nombre de parties ──
    struct Acc<'a> {
        name: &'a str,
        record: Rate,
        heroes: Tally,
        roles: Tally,
        sums: AvgStats,
        minutes: f64,
        seconds_dead: f64,
        seconds: f64,
    }
    let mut accs: BTreeMap<&str, Acc> = BTreeMap::new();
    for g in games {
        let w = won(g);
        let minutes = (g.summary.length_s / 60.0).max(1.0 / 60.0);
        for p in mine(g) {
            let a = accs.entry(p.toon.as_str()).or_insert_with(|| Acc {
                name: p.name.as_str(),
                record: Rate::default(),
                heroes: Tally::default(),
                roles: Tally::default(),
                sums: AvgStats::default(),
                minutes: 0.0,
                seconds_dead: 0.0,
                seconds: 0.0,
            });
            a.name = p.name.as_str(); // dernier nom vu
            a.record.add(w);
            a.heroes.hit(&p.hero, w);
            a.roles.hit(role_of(roles, &p.hero), w);
            let s = &p.stats;
            a.sums.kills += s.kills;
            a.sums.deaths += s.deaths;
            a.sums.assists += s.assists;
            a.sums.kill_participation_pct += s.kill_participation * 100.0;
            a.sums.hero_damage_pm += s.hero_damage;
            a.sums.siege_damage_pm += s.siege_damage;
            a.sums.healing_pm += s.healing;
            a.sums.damage_taken_pm += s.damage_taken;
            a.sums.xp_pm += s.xp_contribution;
            a.minutes += minutes;
            a.seconds_dead += s.time_dead;
            a.seconds += g.summary.length_s;
        }
    }
    let mut order: Vec<(&str, bool, u32, &str)> = accs
        .iter()
        .map(|(t, a)| (*t, roster.iter().any(|r| r == t), a.record.n, a.name))
        .collect();
    order.sort_by(|a, b| b.1.cmp(&a.1).then(b.2.cmp(&a.2)).then_with(|| a.3.cmp(b.3)));
    let mut pid_of: HashMap<&str, String> = HashMap::new();
    let mut players = Vec::new();
    for (i, (toon, core, _, _)) in order.iter().enumerate() {
        let Some(a) = accs.get(toon) else { continue };
        let pid = format!("p{}", i + 1);
        pid_of.insert(toon, pid.clone());
        let games_n = f64::from(a.record.n.max(1));
        let per_min = |v: f64| if a.minutes > 0.0 { v / a.minutes } else { 0.0 };
        players.push(PlayerFacts {
            pid: pid.clone(),
            toon: toon.to_string(),
            name: a.name.to_string(),
            core: *core,
            record: a.record,
            heroes: a.heroes.heroes(&format!("{pid}.hero"), a.record.n),
            roles: a.roles.counts(&format!("{pid}.role"), a.record.n),
            stats: AvgStats {
                kills: a.sums.kills / games_n,
                deaths: a.sums.deaths / games_n,
                assists: a.sums.assists / games_n,
                kill_participation_pct: a.sums.kill_participation_pct / games_n,
                hero_damage_pm: per_min(a.sums.hero_damage_pm),
                siege_damage_pm: per_min(a.sums.siege_damage_pm),
                healing_pm: per_min(a.sums.healing_pm),
                damage_taken_pm: per_min(a.sums.damage_taken_pm),
                xp_pm: per_min(a.sums.xp_pm),
                time_dead_pct: if a.seconds > 0.0 { a.seconds_dead * 100.0 / a.seconds } else { 0.0 },
            },
        });
    }

    // ── cartes ──
    let mut by_map: BTreeMap<&str, Vec<&GameInput>> = BTreeMap::new();
    for g in games {
        by_map.entry(g.summary.map.as_str()).or_default().push(g);
    }
    let mut maps: Vec<MapFacts> = by_map
        .into_iter()
        .map(|(map, gs)| {
            let n = gs.len() as u32;
            let (mut record, mut picks, mut bans, mut against) =
                (Rate::default(), Tally::default(), Tally::default(), Tally::default());
            for g in &gs {
                let w = won(g);
                record.add(w);
                for p in mine(g) {
                    picks.hit(&p.hero, w);
                }
                for b in &g.summary.bans[g.side as usize] {
                    bans.hit(&b.hero, w);
                }
                for b in &g.summary.bans[1 - g.side as usize] {
                    against.hit(&b.hero, w);
                }
            }
            let id = format!("map.{}", slug(map));
            MapFacts {
                wilson: wilson(record),
                picks: picks.heroes(&format!("{id}.pick"), n),
                bans: bans.counts(&format!("{id}.ban"), n),
                bans_against: against.counts(&format!("{id}.ban_against"), n),
                id,
                map: map.to_string(),
                record,
            }
        })
        .collect();
    maps.sort_by(|a, b| b.record.n.cmp(&a.record.n).then_with(|| a.map.cmp(&b.map)));

    // ── draft ──
    let mut draft = DraftFacts::default();
    let (mut b1, mut bm, mut ba, mut open, mut open_role, mut last) = (
        Tally::default(),
        Tally::default(),
        Tally::default(),
        Tally::default(),
        Tally::default(),
        Tally::default(),
    );
    for g in games {
        let s = g.summary;
        let me = g.side as usize;
        let has_draft = !s.bans[me].is_empty() || s.picks[me].len() == 5;
        if !has_draft {
            continue;
        }
        draft.games += 1;
        let w = won(g);
        if let Some(fp) = s.first_pick {
            let first = fp == g.side;
            draft.first_pick.add(first);
            if first {
                draft.first_pick_record.add(w);
            } else {
                draft.second_pick_record.add(w);
            }
        }
        for b in &s.bans[me] {
            match b.phase {
                BanPhase::First => b1.hit(&b.hero, w),
                BanPhase::Mid => bm.hit(&b.hero, w),
            }
        }
        for b in &s.bans[1 - me] {
            ba.hit(&b.hero, w);
        }
        if let Some(h) = s.picks[me].first() {
            open.hit(h, w);
            open_role.hit(role_of(roles, h), w);
        }
        if s.picks[me].len() == 5 {
            if let Some(h) = s.picks[me].last() {
                last.hit(h, w);
            }
        }
    }
    let dn = draft.games;
    draft.bans_first = b1.counts("draft.ban_first", dn);
    draft.bans_mid = bm.counts("draft.ban_mid", dn);
    draft.bans_against = ba.counts("draft.ban_against", dn);
    draft.openers = open.counts("draft.opener", dn);
    draft.opener_roles = open_role.counts("draft.opener_role", dn);
    draft.last_picks = last.counts("draft.last_pick", dn);

    // ── déroulé ──
    let mut flow = FlowFacts::default();
    for g in games {
        let s = g.summary;
        let w = won(g);
        let (me, other) = (s.level10[g.side as usize], s.level10[1 - g.side as usize]);
        let first10 = match (me, other) {
            (Some(a), Some(b)) => Some(a <= b),
            (Some(_), None) => Some(true),
            (None, Some(_)) => Some(false),
            (None, None) => None,
        };
        if let Some(f) = first10 {
            flow.first_to_10.add(f);
            if f {
                flow.first_to_10_record.add(w);
            }
        }
        if let Some(t) = s.first_fort {
            let f = t == g.side;
            flow.first_fort.add(f);
            if f {
                flow.first_fort_record.add(w);
            }
        }
        if let Some(t) = s.first_objective {
            let f = t == g.side;
            flow.first_objective.add(f);
            if f {
                flow.first_objective_record.add(w);
            }
        }
        let sign = if g.side == 0 { 1.0 } else { -1.0 };
        let max_ahead = s.level_diff.iter().map(|x| x.diff * sign).fold(0.0, f64::max);
        let max_behind = s.level_diff.iter().map(|x| -x.diff * sign).fold(0.0, f64::max);
        if max_behind >= 2.0 {
            flow.comebacks.add(w);
        }
        if max_ahead >= 2.0 {
            flow.throws.add(!w);
        }
    }
    for minute in [10u32, 15, 20] {
        let vals: Vec<f64> = games
            .iter()
            .filter_map(|g| diff_at(g.summary, g.side, f64::from(minute) * 60.0))
            .collect();
        let n = vals.len() as u32;
        flow.level_diff.push(LevelDiffAt {
            id: format!("flow.level_diff.{minute}"),
            minute,
            avg: if n > 0 { vals.iter().sum::<f64>() / f64::from(n) } else { 0.0 },
            n,
        });
    }
    let buckets: [(&str, &str, f64, f64); 4] = [
        ("lt15", "< 15 min", 0.0, 900.0),
        ("15_20", "15–20 min", 900.0, 1200.0),
        ("20_25", "20–25 min", 1200.0, 1500.0),
        ("ge25", "≥ 25 min", 1500.0, f64::INFINITY),
    ];
    for (key, label, lo, hi) in buckets {
        let mut r = Rate::default();
        for g in games.iter().filter(|g| g.summary.length_s >= lo && g.summary.length_s < hi) {
            r.add(won(g));
        }
        flow.length.push(Bucket { id: format!("flow.length.{key}"), label: label.into(), record: r });
    }

    // ── parties ──
    let mut game_rows: Vec<GameRow> = games
        .iter()
        .map(|g| {
            let s = g.summary;
            let (me, other) = (g.side as usize, 1 - g.side as usize);
            let picks_of = |side: usize| -> Vec<GamePick> {
                // ordre de pick lu dans le replay quand il est complet, sinon ordre du résumé
                let team = &s.teams[side];
                let ordered: Vec<&str> = if s.picks[side].len() == team.len() {
                    s.picks[side].iter().map(String::as_str).collect()
                } else {
                    team.iter().map(|p| p.hero.as_str()).collect()
                };
                ordered
                    .into_iter()
                    .map(|h| GamePick {
                        hero: h.to_string(),
                        player: team
                            .iter()
                            .find(|p| p.hero == h)
                            .map(|p| pid_of.get(p.toon.as_str()).cloned().unwrap_or_else(|| p.name.clone()))
                            .unwrap_or_default(),
                    })
                    .collect()
            };
            GameRow {
                id: format!("game.{}", g.gid),
                gid: g.gid,
                date: s.date.clone(),
                map: s.map.clone(),
                won: won(g),
                length_s: s.length_s,
                first_pick: s.first_pick.map(|f| f == g.side),
                opponents: theirs(g).iter().map(|p| p.name.clone()).collect(),
                picks: picks_of(me),
                opp_picks: picks_of(other),
                bans: s.bans[me].iter().map(|b| b.hero.clone()).collect(),
                bans_against: s.bans[other].iter().map(|b| b.hero.clone()).collect(),
            }
        })
        .collect();
    game_rows.sort_by(|a, b| a.date.cmp(&b.date).then(a.gid.cmp(&b.gid)));

    let mut facts = Facts {
        overview,
        players,
        maps,
        draft,
        flow,
        games: game_rows,
        index: BTreeMap::new(),
    };
    facts.index = build_index(&facts);
    facts
}

fn rate_ref(label: String, r: Rate) -> FactRef {
    FactRef { label, text: r.text(), n: r.n, low: r.n < LOW_SAMPLE }
}

fn fmt_len(s: f64) -> String {
    format!("{}:{:02}", (s / 60.0) as u32, (s % 60.0) as u32)
}

/// Index plat de tous les faits citables : c'est ce que l'import résout (`evidence`).
pub fn build_index(f: &Facts) -> BTreeMap<String, FactRef> {
    ordered_index(f).into_iter().collect()
}

/// Même contenu que l'index, dans l'ordre de lecture (vue d'ensemble, joueurs, cartes, draft,
/// déroulé, parties) — l'ordre du pack et du classeur.
pub fn ordered_index(f: &Facts) -> Vec<(String, FactRef)> {
    let mut ix = Vec::new();
    let mut put = |id: &str, r: FactRef| {
        ix.push((id.to_string(), r));
    };
    let o = &f.overview;
    put("overview.record", rate_ref("Overall record (wins/games)".into(), o.record));
    put(
        "overview.avg_length",
        FactRef { label: "Average game length".into(), text: fmt_len(o.avg_length_s), n: o.games, low: o.games < LOW_SAMPLE },
    );
    for p in &f.players {
        let who = format!("{} ({})", p.name, p.pid);
        put(&format!("{}.record", p.pid), rate_ref(format!("{who} — record"), p.record));
        for h in &p.heroes {
            let r = h.record;
            put(
                &h.id,
                FactRef {
                    label: format!("{who} — {}: games played, record", h.hero),
                    text: format!("{} games, {}", h.picks.k, r.text()),
                    n: h.picks.k,
                    low: h.picks.k < LOW_SAMPLE,
                },
            );
        }
        for r in &p.roles {
            put(&r.id, rate_ref(format!("{who} — games as {}", r.key), r.count));
        }
        let s = &p.stats;
        put(
            &format!("{}.stats", p.pid),
            FactRef {
                label: format!("{who} — averages"),
                text: format!(
                    "K/D/A {:.1}/{:.1}/{:.1}, KP {:.0}%, hero dmg {:.0}/min, siege {:.0}/min, healing {:.0}/min, taken {:.0}/min, XP {:.0}/min, dead {:.1}% of the game",
                    s.kills, s.deaths, s.assists, s.kill_participation_pct, s.hero_damage_pm,
                    s.siege_damage_pm, s.healing_pm, s.damage_taken_pm, s.xp_pm, s.time_dead_pct
                ),
                n: p.record.n,
                low: p.record.n < LOW_SAMPLE,
            },
        );
    }
    for m in &f.maps {
        let wil = m.wilson.map(|(a, b)| format!(", 95% CI {a:.0}–{b:.0}%")).unwrap_or_default();
        put(
            &format!("{}.record", m.id),
            FactRef {
                label: format!("{} — record", m.map),
                text: format!("{}{wil}", m.record.text()),
                n: m.record.n,
                low: m.record.n < LOW_SAMPLE,
            },
        );
        for h in &m.picks {
            put(
                &h.id,
                FactRef {
                    label: format!("{} — {} picked (games, record)", m.map, h.hero),
                    text: format!("{} of {} games, {}", h.picks.k, h.picks.n, h.record.text()),
                    n: h.picks.n,
                    low: h.picks.n < LOW_SAMPLE,
                },
            );
        }
        for c in &m.bans {
            put(&c.id, rate_ref(format!("{} — {} banned by them", m.map, c.key), c.count));
        }
        for c in &m.bans_against {
            put(&c.id, rate_ref(format!("{} — {} banned against them", m.map, c.key), c.count));
        }
    }
    let d = &f.draft;
    put("draft.first_pick", rate_ref("Had first pick".into(), d.first_pick));
    put("draft.first_pick.record", rate_ref("Record with first pick".into(), d.first_pick_record));
    put("draft.second_pick.record", rate_ref("Record with second pick".into(), d.second_pick_record));
    for (list, what) in [
        (&d.bans_first, "first-phase ban"),
        (&d.bans_mid, "mid-draft ban"),
        (&d.bans_against, "banned against them"),
        (&d.openers, "their first pick"),
        (&d.opener_roles, "role of their first pick"),
        (&d.last_picks, "their last pick"),
    ] {
        for c in list {
            put(&c.id, rate_ref(format!("{} — {what}", c.key), c.count));
        }
    }
    let fl = &f.flow;
    put("flow.first_to_10", rate_ref("Reached level 10 first".into(), fl.first_to_10));
    put("flow.first_to_10.record", rate_ref("Record when first to level 10".into(), fl.first_to_10_record));
    put("flow.first_fort", rate_ref("Took the first fort".into(), fl.first_fort));
    put("flow.first_fort.record", rate_ref("Record when taking the first fort".into(), fl.first_fort_record));
    put("flow.first_objective", rate_ref("Won the first map objective".into(), fl.first_objective));
    put(
        "flow.first_objective.record",
        rate_ref("Record when winning the first objective".into(), fl.first_objective_record),
    );
    for l in &fl.level_diff {
        put(
            &l.id,
            FactRef {
                label: format!("Average level lead at {} min (negative = behind)", l.minute),
                text: format!("{:+.1} levels over {} games", l.avg, l.n),
                n: l.n,
                low: l.n < LOW_SAMPLE,
            },
        );
    }
    for b in &fl.length {
        put(&b.id, rate_ref(format!("Record in games lasting {}", b.label), b.record));
    }
    put("flow.comeback", rate_ref("Wins after trailing by 2+ levels".into(), fl.comebacks));
    put("flow.throw", rate_ref("Losses after leading by 2+ levels".into(), fl.throws));
    for g in &f.games {
        put(
            &g.id,
            FactRef {
                label: format!("Game on {} ({})", g.map, g.date.as_deref().unwrap_or("?").get(..10).unwrap_or("?")),
                text: format!("{} in {}", if g.won { "win" } else { "loss" }, fmt_len(g.length_s)),
                n: 1,
                low: false,
            },
        );
    }
    ix
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use crate::scouting::side::tests::game;
    use crate::scouting::summary::{Ban, LevelSegment};

    const US: [&str; 5] = ["r1", "r2", "r3", "r4", "r5"];

    fn roster() -> Vec<String> {
        US.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn slugs() {
        assert_eq!(slug("Braxis Holdout"), "braxis_holdout");
        assert_eq!(slug("Anub'arak"), "anubarak");
        assert_eq!(slug("E.T.C."), "etc");
        assert_eq!(slug("Sgt. Hammer"), "sgt_hammer");
        assert_eq!(slug("Li-Ming"), "li_ming");
    }

    #[test]
    fn wilson_valeurs_de_reference() {
        // 3/3 → [43.8 ; 100] ; 5/10 → [23.7 ; 76.3]
        let (lo, hi) = wilson(Rate { k: 3, n: 3 }).unwrap();
        assert!((lo - 43.85).abs() < 0.1 && (hi - 100.0).abs() < 1e-9, "{lo} {hi}");
        let (lo, hi) = wilson(Rate { k: 5, n: 10 }).unwrap();
        assert!((lo - 23.66).abs() < 0.1 && (hi - 76.34).abs() < 0.1, "{lo} {hi}");
        assert_eq!(wilson(Rate { k: 0, n: 0 }), None);
    }

    fn inputs(games: &[GameSummary], sides: &[u8]) -> Vec<(i64, u8)> {
        games.iter().zip(sides).enumerate().map(|(i, (_, s))| (i as i64 + 1, *s)).collect()
    }

    fn run(games: &[GameSummary], sides: &[u8]) -> Facts {
        let ix = inputs(games, sides);
        let gi: Vec<GameInput> = ix
            .iter()
            .map(|(gid, side)| GameInput { gid: *gid, summary: &games[(*gid - 1) as usize], side: *side })
            .collect();
        let roles: HashMap<String, String> = [("h_r1".to_string(), "Tank".to_string())].into();
        compute(&gi, &roster(), 1, &roles)
    }

    fn sample() -> Vec<GameSummary> {
        let them = ["o1", "o2", "o3", "o4", "o5"];
        let mut g1 = game(&US, &them); // cible côté 0, gagne
        g1.winner = Some(0);
        g1.first_pick = Some(0);
        g1.bans = [
            vec![
                Ban { hero: "Johanna".into(), phase: BanPhase::First },
                Ban { hero: "Anduin".into(), phase: BanPhase::Mid },
            ],
            vec![Ban { hero: "Garrosh".into(), phase: BanPhase::First }],
        ];
        g1.picks = [US.iter().map(|t| format!("H_{t}")).collect(), Vec::new()];
        g1.level10 = [Some(400.0), Some(450.0)];
        g1.first_fort = Some(0);
        g1.level_diff = vec![
            LevelSegment { start: 0.0, end: 700.0, diff: -2.0 },
            LevelSegment { start: 700.0, end: 900.0, diff: 1.0 },
        ];
        let mut g2 = game(&them, &["r1", "r2", "r3", "r4", "sub"]); // cible côté 1, perd
        g2.winner = Some(0);
        g2.map = "Cursed Hollow".into();
        g2.length_s = 1300.0;
        g2.first_pick = Some(0);
        g2.level10 = [Some(400.0), None];
        g2.level_diff = vec![LevelSegment { start: 0.0, end: 1300.0, diff: -3.0 }];
        vec![g1, g2]
    }

    #[test]
    fn bilans_joueurs_et_remplacants() {
        let f = run(&sample(), &[0, 1]);
        assert_eq!(f.overview.games, 2);
        assert_eq!(f.overview.record, Rate { k: 1, n: 2 });
        assert_eq!(f.overview.excluded, 1);
        // roster d'abord (r1..r4 : 2 parties, r5 : 1), le remplaçant en dernier
        assert_eq!(f.players.len(), 6);
        assert!(f.players[..5].iter().all(|p| p.core));
        let sub = f.players.last().unwrap();
        assert_eq!((sub.toon.as_str(), sub.core, sub.pid.as_str()), ("sub", false, "p6"));
        let p1 = &f.players[0];
        assert_eq!(p1.record, Rate { k: 1, n: 2 });
        assert_eq!(p1.roles[0].key, "Tank");
    }

    #[test]
    fn draft_vue_depuis_la_cible() {
        let f = run(&sample(), &[0, 1]);
        let d = &f.draft;
        // g1 : bans + 5 picks ; g2 : aucun ban ni pick → hors draft
        assert_eq!(d.games, 1);
        assert_eq!(d.first_pick, Rate { k: 1, n: 1 });
        assert_eq!(d.bans_first[0].id, "draft.ban_first.johanna");
        assert_eq!(d.bans_mid[0].key, "Anduin");
        assert_eq!(d.bans_against[0].key, "Garrosh");
        assert_eq!(d.openers[0].key, "H_r1");
        assert_eq!(d.opener_roles[0].key, "Tank");
    }

    #[test]
    fn deroule_depuis_la_cible() {
        let f = run(&sample(), &[0, 1]);
        let fl = &f.flow;
        // g1 : cible (0) à 10 en premier ; g2 : l'adversaire (côté 0) seul à 10
        assert_eq!(fl.first_to_10, Rate { k: 1, n: 2 });
        assert_eq!(fl.first_fort, Rate { k: 1, n: 1 });
        // g1 : cible côté 0, diff -2 → menée de 2, puis gagne → remontée.
        // g2 : diff (équipe 0 − équipe 1) = -3 et la cible est côté 1 → elle MÈNE de 3 et perd.
        assert_eq!(fl.comebacks, Rate { k: 1, n: 1 });
        assert_eq!(fl.throws, Rate { k: 1, n: 1 });
        // écart à 10 min : g1 → -2 (côté 0) ; g2 → +3 (côté 1, signe inversé) ; moyenne +0,5
        let at10 = &fl.level_diff[0];
        assert_eq!(at10.n, 2);
        assert!((at10.avg - 0.5).abs() < 1e-9, "{}", at10.avg);
        // tranches : g1 900 s → 15–20 ; g2 1300 s → 20–25
        assert_eq!(fl.length[1].record, Rate { k: 1, n: 1 });
        assert_eq!(fl.length[2].record, Rate { k: 0, n: 1 });
    }

    #[test]
    fn index_complet_ids_bien_formes_et_n_partout() {
        let f = run(&sample(), &[0, 1]);
        let ok = |id: &str| id.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '.');
        for (id, r) in &f.index {
            assert!(ok(id), "id mal formé : {id}");
            // une référence de partie désigne UNE partie : pas un taux, donc pas « faible échantillon »
            if !id.starts_with("game.") {
                assert_eq!(r.low, r.n < LOW_SAMPLE, "{id}");
            }
        }
        for id in [
            "overview.record",
            "p1.record",
            "p1.stats",
            "map.braxis_holdout.record",
            "map.cursed_hollow.record",
            "draft.first_pick",
            "draft.ban_first.johanna",
            "flow.first_to_10",
            "flow.level_diff.10",
            "flow.length.lt15",
            "game.1",
        ] {
            assert!(f.index.contains_key(id), "id absent : {id}");
        }
        // parties : picks attribués aux pN
        assert_eq!(f.games[0].picks[0].player, "p1");
    }

    #[test]
    fn aucune_partie_ne_panique_pas() {
        let f = compute(&[], &[], 3, &HashMap::new());
        assert_eq!(f.overview.games, 0);
        assert_eq!(f.overview.excluded, 3);
        assert!(f.index.contains_key("overview.record"));
    }
}
