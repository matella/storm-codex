//! Match list helpers of the session OBS sources (`/ticker`, `/widget`, `/queue`): wire types, mode badge,
//! awards, operator lookup, streak and the Jarvis line. Port of the helpers of `web/src/api.ts` and of the
//! computations of `Ticker.tsx`, `Widget.tsx` and `Queue.tsx`. No I/O: the operator names and the local day of
//! each game come from the caller.

use serde::Deserialize;

/// One player of `GET /api/matches` (`MatchPlayer` in api.ts, only what the sources read).
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct MatchPlayer {
    pub toon: Option<String>,
    pub name: Option<String>,
    pub hero: Option<String>,
    pub team: Option<i64>,
    pub win: Option<bool>,
    pub kills: Option<u32>,
    pub deaths: Option<u32>,
    pub takedowns: Option<u32>,
    pub award: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct MatchSummary {
    pub id: i64,
    pub map: Option<String>,
    pub mode: Option<i64>,
    pub played_at: Option<String>,
    /// Seconds.
    pub length: Option<f64>,
    pub winner: Option<i64>,
    pub players: Option<Vec<MatchPlayer>>,
    /// Player selected by the archive API for the account/result/MVP filters.
    pub perspective_toon: Option<String>,
}

/// `GET /api/settings`.
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub operator_names: Vec<String>,
    pub brief_voice: BriefVoice,
}

/// Voice of the Jarvis butler brief (`app_settings.brief_voice`, `PUT /api/admin/settings`):
/// `Fr` (default, zero behaviour change for an existing install) and `En` are separate
/// hand-written phrase banks (idiom, not a 1:1 translation); `Off` disables the brief line
/// entirely (`jarvis_phrase` returns `None`, `/queue` simply omits it).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum BriefVoice {
    #[default]
    Fr,
    En,
    Off,
}

/// `modeBadge`: short label and CSS class of a game mode (official storm-stats codes).
pub fn mode_badge(mode: Option<i64>) -> (&'static str, &'static str) {
    match mode {
        Some(-1) => ("CUSTOM", "b-qm"),
        Some(50001) => ("QM", "b-qm"),
        Some(50021) => ("VS IA", "b-qm"),
        Some(50031) => ("BRAWL", "b-qm"),
        Some(50041) => ("ENTR.", "b-qm"),
        Some(50051) => ("UD", "b-sl"),
        Some(50061) => ("HL", "b-sl"),
        Some(50071) => ("TL", "b-sl"),
        Some(50091) => ("SL", "b-sl"),
        Some(50101) => ("ARAM", "b-qm"),
        _ => ("—", "b-qm"),
    }
}

/// `fmtDur`: `m:ss`, a dash for a missing or zero duration.
pub fn fmt_dur(seconds: Option<f64>) -> String {
    match seconds {
        Some(s) if s != 0.0 && !s.is_nan() => {
            format!(
                "{}:{:02}",
                (s / 60.0).floor() as i64,
                (s % 60.0).floor() as i64
            )
        }
        _ => "—".into(),
    }
}

/// `mapSlug`: lower case, apostrophes dropped, whitespace runs -> `-`.
pub fn map_slug(map: &str) -> String {
    let mut out = String::new();
    let mut space = false;
    for c in map
        .to_lowercase()
        .chars()
        .filter(|c| *c != '\'' && *c != '’')
    {
        // same kept characters as the dataset's `Battleground.slug` ("Checkpoint: Hanamura")
        if !c.is_whitespace() && !crate::logic::slug_char(c) {
            continue;
        }
        if c.is_whitespace() {
            space = true;
        } else {
            if space {
                out.push('-');
                space = false;
            }
            out.push(c);
        }
    }
    if space {
        out.push('-');
    }
    out
}

/// `mapImage`: painted loading-screen art, `None` when there is no map.
pub fn map_image(map: Option<&str>) -> Option<String> {
    map.filter(|m| !m.is_empty())
        .map(|m| format!("/images/battlegrounds/{}.png", map_slug(m)))
}

#[derive(Clone, Debug, PartialEq)]
pub struct Award {
    pub label: String,
    pub icon: &'static str,
    pub mvp: bool,
}

fn award_icon(core: &str) -> &'static str {
    match core {
        "MVP" => "👑",
        "MostHeroDamageDone" => "🗡️",
        "MostTeamfightHeroDamageDone" => "⚔️",
        "MostSiegeDamageDone" => "🏰",
        "MostHealing" => "💚",
        "MostTeamfightHealingDone" => "💗",
        "ClutchHealer" => "🚑",
        "MostDamageTaken" => "🛡️",
        "MostTeamfightDamageTaken" => "🧱",
        "MostMercCampsCaptured" => "🏕️",
        "HighestKillStreak" => "🔥",
        "HatTrick" => "🎩",
        "MostKills" => "💀",
        "MostStuns" => "💫",
        "MostSilences" => "🤐",
        "MostRoots" => "🌿",
        "MostXPContribution" => "📈",
        "MostDamageDoneToZerg" => "🐛",
        "MostAltarDamageDone" => "⛩️",
        "MostImmortalDamage" => "👹",
        "MostCurseDamageDone" => "☠️",
        "MostGemsTurnedIn" => "💎",
        "MostTimeOnPoint" => "📍",
        "MostDamageToMinions" => "🧟",
        "MostEscapes" => "🏃",
        "MostDaredevilEscapes" => "🪂",
        "MostDragonShrinesCaptured" => "🐉",
        "MostTimeInTemple" => "🛕",
        "MostVengeancesPerformed" => "😤",
        "MostProtection" => "🤝",
        "0OutnumberedDeaths" => "🧊",
        "0Deaths" => "😇",
        "MostInterruptedCageUnlocks" => "🔓",
        "MostCoinsPaid" => "🪙",
        "MostNukeDamageDone" => "☢️",
        "MostSeedsCollected" => "🌱",
        _ => "🏅",
    }
}

/// `awardLabel`: short label, emoji and MVP flag of an end-of-match award (`None` without award).
pub fn award_label(raw: Option<&str>) -> Option<Award> {
    let raw = raw.filter(|r| !r.is_empty())?;
    let core = raw.strip_prefix("EndOfMatchAward").unwrap_or(raw);
    let core = core.strip_suffix("Boolean").unwrap_or(core);
    let icon = award_icon(core);
    if core == "MVP" {
        return Some(Award {
            label: "MVP".into(),
            icon,
            mvp: true,
        });
    }
    // split camel case, then shorten the verbose labels
    let mut spaced = String::new();
    let mut prev: Option<char> = None;
    for c in core.chars() {
        if prev.is_some_and(|p| p.is_ascii_lowercase()) && c.is_ascii_uppercase() {
            spaced.push(' ');
        }
        spaced.push(c);
        prev = Some(c);
    }
    let label = spaced.strip_prefix("Most ").unwrap_or(&spaced);
    let label = label.strip_suffix(" Done").unwrap_or(label);
    Some(Award {
        label: label.into(),
        icon,
        mvp: false,
    })
}

fn same_name(p: &MatchPlayer, wanted: &str) -> bool {
    p.name.as_deref().unwrap_or("").to_lowercase() == wanted
}

/// `pickOperator`: the `?me=` override first, then any configured operator name, then the first player.
pub fn pick_operator<'a>(
    players: &'a [MatchPlayer],
    over: Option<&str>,
    names: &[String],
) -> Option<&'a MatchPlayer> {
    let candidates = over
        .into_iter()
        .chain(names.iter().map(String::as_str))
        .filter(|n| !n.is_empty())
        .map(str::to_lowercase);
    for c in candidates {
        if let Some(p) = players.iter().find(|p| same_name(p, &c)) {
            return Some(p);
        }
    }
    players.first()
}

/// `matchOperator`: strict, the operator only when a configured name matches (no fallback).
pub fn match_operator<'a>(players: &'a [MatchPlayer], names: &[String]) -> Option<&'a MatchPlayer> {
    let names: Vec<String> = names.iter().map(|n| n.to_lowercase()).collect();
    players.iter().find(|p| {
        let n = p.name.as_deref().unwrap_or("").to_lowercase();
        names.contains(&n)
    })
}

/// `jarvisPhrase`: deterministic butler line, one of 10 priority-ordered trigger buckets (first
/// match wins; a game can only match one, so the order encodes "most noteworthy wins/losses
/// first"), variant picked from `takedowns + deaths` (no RNG — same inputs always produce the
/// same line, so it's testable and replayable), no I/O. `None` when `voice` is `Off`.
///
/// Priority, highest first: MVP this game (win or loss) · beat the tracked nemesis (win) · win
/// streak >= 3 · session-best KDA tonight (win) · no-death win · >= 15-takedown win · plain win ·
/// loss streak <= -3 · high-death loss (>= 8) · plain loss. `map`, when known, is spliced into
/// roughly a third of each bucket's variants instead of being its own tier (16 maps x 10
/// categories would dilute the phrase bank for little payoff).
#[allow(clippy::too_many_arguments)]
pub fn jarvis_phrase(
    won: bool,
    hero: Option<&str>,
    deaths: u32,
    takedowns: u32,
    streak: i32,
    mvp: bool,
    beat_nemesis: bool,
    session_best_kda: bool,
    map: Option<&str>,
    voice: BriefVoice,
) -> Option<String> {
    if voice == BriefVoice::Off {
        return None;
    }
    let seed = (takedowns + deaths) as usize;
    let pick = |v: Vec<String>| v[seed % v.len()].clone();
    let fr = voice == BriefVoice::Fr;
    let h = hero.unwrap_or(if fr { "ce héros" } else { "that hero" });
    let m = map.filter(|m| !m.is_empty());

    if mvp {
        return Some(pick(if fr {
            jarvis_mvp_fr(won, h, m)
        } else {
            jarvis_mvp_en(won, h, m)
        }));
    }
    if won {
        if beat_nemesis {
            return Some(pick(if fr {
                jarvis_nemesis_fr(h, m)
            } else {
                jarvis_nemesis_en(h, m)
            }));
        }
        if streak >= 3 {
            return Some(pick(if fr {
                jarvis_streak_win_fr(streak, m)
            } else {
                jarvis_streak_win_en(streak, m)
            }));
        }
        if session_best_kda {
            return Some(pick(if fr {
                jarvis_best_kda_fr(h, m)
            } else {
                jarvis_best_kda_en(h, m)
            }));
        }
        if deaths == 0 {
            return Some(pick(if fr {
                jarvis_no_death_fr(m)
            } else {
                jarvis_no_death_en(m)
            }));
        }
        if takedowns >= 15 {
            return Some(pick(if fr {
                jarvis_big_takedowns_fr(h, takedowns, m)
            } else {
                jarvis_big_takedowns_en(h, takedowns, m)
            }));
        }
        return Some(pick(if fr {
            jarvis_plain_win_fr(h, m)
        } else {
            jarvis_plain_win_en(h, m)
        }));
    }
    if streak <= -3 {
        return Some(pick(if fr {
            jarvis_streak_loss_fr(streak)
        } else {
            jarvis_streak_loss_en(streak)
        }));
    }
    if deaths >= 8 {
        return Some(pick(if fr {
            jarvis_high_death_fr(deaths)
        } else {
            jarvis_high_death_en(deaths)
        }));
    }
    Some(pick(if fr {
        jarvis_plain_loss_fr(h)
    } else {
        jarvis_plain_loss_en(h)
    }))
}

fn jarvis_mvp_fr(won: bool, h: &str, m: Option<&str>) -> Vec<String> {
    let mut v = if won {
        vec![
            format!("MVP avec {h}. Rien à ajouter, monsieur."),
            "MVP. Le trophée vous revient ce soir.".into(),
        ]
    } else {
        vec![
            "MVP malgré la défaite, mes hommages, monsieur.".into(),
            "La défaite n'enlève rien à votre MVP.".into(),
        ]
    };
    if let Some(map) = m {
        v.push(if won {
            format!("MVP sur {map}, monsieur. Une partie pour les annales.")
        } else {
            format!("MVP sur {map}, même dans la défaite. Chapeau, monsieur.")
        });
    }
    v
}

fn jarvis_mvp_en(won: bool, h: &str, m: Option<&str>) -> Vec<String> {
    let mut v = if won {
        vec![
            format!("MVP on {h}. Nothing more to say, sir."),
            "MVP. The crown's yours tonight.".into(),
        ]
    } else {
        vec![
            "MVP in a losing effort — my compliments, sir.".into(),
            "The loss takes nothing away from that MVP.".into(),
        ]
    };
    if let Some(map) = m {
        v.push(if won {
            format!("MVP on {map}, sir. One for the highlight reel.")
        } else {
            format!("MVP on {map}, even in defeat. Hats off, sir.")
        });
    }
    v
}

fn jarvis_nemesis_fr(h: &str, m: Option<&str>) -> Vec<String> {
    let mut v = vec![
        "Le compte est réglé, monsieur. Némésis vaincue.".into(),
        format!("Enfin. {h} a fait tomber la némésis ce soir."),
    ];
    if let Some(map) = m {
        v.push(format!(
            "Némésis vaincue sur {map}. Douce revanche, monsieur."
        ));
    }
    v
}

fn jarvis_nemesis_en(h: &str, m: Option<&str>) -> Vec<String> {
    let mut v = vec![
        "Score settled, sir. The nemesis falls.".into(),
        format!("Finally. {h} put the nemesis down tonight."),
    ];
    if let Some(map) = m {
        v.push(format!("Nemesis beaten on {map}. Sweet revenge, sir."));
    }
    v
}

fn jarvis_streak_win_fr(streak: i32, m: Option<&str>) -> Vec<String> {
    let mut v = vec![
        format!("Et de {streak}, monsieur. La soirée vous appartient."),
        format!("Série de {streak}. Difficile de faire mieux."),
        format!("{streak} d'affilée — je note la domination."),
    ];
    if let Some(map) = m {
        v.push(format!(
            "{streak} d'affilée, la dernière sur {map} — je note la domination."
        ));
    }
    v
}

fn jarvis_streak_win_en(streak: i32, m: Option<&str>) -> Vec<String> {
    let mut v = vec![
        format!("That's {streak}, sir. The night is yours."),
        format!("{streak}-game streak. Hard to top that."),
        format!("{streak} in a row — domination noted."),
    ];
    if let Some(map) = m {
        v.push(format!(
            "{streak} in a row, the latest on {map} — domination noted."
        ));
    }
    v
}

fn jarvis_best_kda_fr(h: &str, m: Option<&str>) -> Vec<String> {
    let mut v = vec![
        "Meilleure KDA de la soirée, monsieur. On progresse.".into(),
        format!("{h} signe le meilleur ratio de la session."),
        "Record personnel de la soirée, monsieur.".into(),
    ];
    if let Some(map) = m {
        v.push(format!(
            "Record de la soirée sur {map} — difficile de faire mieux."
        ));
    }
    v
}

fn jarvis_best_kda_en(h: &str, m: Option<&str>) -> Vec<String> {
    let mut v = vec![
        "Best KDA of the night, sir. We're trending up.".into(),
        format!("{h} just posted the session's best ratio."),
        "Tonight's personal best, sir.".into(),
    ];
    if let Some(map) = m {
        v.push(format!("Tonight's best, on {map} no less — hard to top."));
    }
    v
}

fn jarvis_no_death_fr(m: Option<&str>) -> Vec<String> {
    let mut v = vec![
        "Aucune mort. Une partie de maître, monsieur.".into(),
        "Zéro mort — proprement exécuté.".into(),
    ];
    if let Some(map) = m {
        v.push(format!("Zéro mort sur {map}. Une leçon, monsieur."));
    }
    v
}

fn jarvis_no_death_en(m: Option<&str>) -> Vec<String> {
    let mut v = vec![
        "No deaths. A masterclass, sir.".into(),
        "Zero deaths — cleanly done.".into(),
    ];
    if let Some(map) = m {
        v.push(format!("Not a single death on {map}. A lesson, sir."));
    }
    v
}

fn jarvis_big_takedowns_fr(h: &str, takedowns: u32, m: Option<&str>) -> Vec<String> {
    let mut v = vec![
        format!("Démonstration. {h} était intenable."),
        format!("{takedowns} participations — la carte vous a appartenu."),
    ];
    if let Some(map) = m {
        v.push(format!(
            "{takedowns} participations sur {map}. Une masterclass, monsieur."
        ));
    }
    v
}

fn jarvis_big_takedowns_en(h: &str, takedowns: u32, m: Option<&str>) -> Vec<String> {
    let mut v = vec![
        format!("Dominant. {h} was unstoppable."),
        format!("{takedowns} takedowns — you owned the map."),
    ];
    if let Some(map) = m {
        v.push(format!(
            "{takedowns} takedowns on {map}. A masterclass, sir."
        ));
    }
    v
}

fn jarvis_plain_win_fr(h: &str, m: Option<&str>) -> Vec<String> {
    let mut v = vec![
        format!("Victoire nette. {h} a fait le travail."),
        "Bien joué, monsieur. On enchaîne ?".into(),
        "Une de plus au compteur.".into(),
    ];
    if let Some(map) = m {
        v.push(format!("Une victoire de plus sur {map}, monsieur."));
    }
    v
}

fn jarvis_plain_win_en(h: &str, m: Option<&str>) -> Vec<String> {
    let mut v = vec![
        format!("Clean win. {h} did the job."),
        "Well played, sir. Shall we go again?".into(),
        "One more for the tally.".into(),
    ];
    if let Some(map) = m {
        v.push(format!("Another win on {map}, sir."));
    }
    v
}

fn jarvis_streak_loss_fr(streak: i32) -> Vec<String> {
    vec![
        "Série difficile, monsieur. Gardons la tête froide.".into(),
        format!("{} revers d'affilée — une pause, peut-être ?", streak.abs()),
    ]
}

fn jarvis_streak_loss_en(streak: i32) -> Vec<String> {
    vec![
        "A rough stretch, sir. Let's keep a clear head.".into(),
        format!("{} losses in a row — maybe a short break?", streak.abs()),
    ]
}

fn jarvis_high_death_fr(deaths: u32) -> Vec<String> {
    vec![
        "Trop de morts, monsieur. On respire au prochain.".into(),
        format!("{deaths} morts — la prudence paiera la prochaine fois."),
    ]
}

fn jarvis_high_death_en(deaths: u32) -> Vec<String> {
    vec![
        "Too many deaths, sir. Let's breathe before the next one.".into(),
        format!("{deaths} deaths — caution will pay off next time."),
    ]
}

fn jarvis_plain_loss_fr(h: &str) -> Vec<String> {
    vec![
        format!("Défaite serrée. {h} méritait mieux."),
        "On rebondit à la prochaine, monsieur.".into(),
        "Pas cette fois — mais la nuit est jeune.".into(),
    ]
}

fn jarvis_plain_loss_en(h: &str) -> Vec<String> {
    vec![
        format!("Close loss. {h} deserved better."),
        "We bounce back next one, sir.".into(),
        "Not this time — but the night is young.".into(),
    ]
}

/// True when the most recent game (`games[0]`) has personal KDA (same formula as
/// `session_totals`: assists = takedowns - kills, KDA = (K+A)/D, else K+A when deathless)
/// strictly better than or equal to every other game already played this session — "session-best
/// KDA tonight" for the Jarvis brief. Requires at least one earlier game to compare against
/// (`false` for a session's first game: nothing yet to call it "best" against).
#[must_use]
pub fn is_session_best_kda(games: &[OpGame]) -> bool {
    if games.len() < 2 {
        return false;
    }
    let Some(first) = games.first() else {
        return false;
    };
    fn kda_of(me: &MatchPlayer) -> f64 {
        let (k, a, d) = kad(me);
        let ka = (k + a) as f64;
        if d > 0 {
            ka / d as f64
        } else {
            ka
        }
    }
    let best = kda_of(first.me);
    games[1..].iter().all(|g| kda_of(g.me) <= best)
}

/// Toon-set intersection: did the operator just beat the tracked nemesis this game — the same
/// opponent-resolution logic as `nemesis::enemy_toons` (did the enemy roster contain the one toon
/// currently flagged as a grudge match), applied to an already-finished game instead of a live
/// lobby. `false` without a win, without a tracked nemesis, or when the nemesis wasn't in this
/// game's enemy roster.
#[must_use]
pub fn beat_nemesis(won: bool, enemy_toons: &[String], nemesis_toon: Option<&str>) -> bool {
    let Some(nemesis) = nemesis_toon else {
        return false;
    };
    won && enemy_toons.iter().any(|t| t == nemesis)
}

/// One game of the operator, most recent first in every list.
#[derive(Clone, Debug, PartialEq)]
pub struct OpGame<'a> {
    pub m: &'a MatchSummary,
    pub me: &'a MatchPlayer,
    pub won: bool,
    /// Local calendar day `YYYY-MM-DD`.
    pub day: String,
}

/// The operator's games (any configured account), dropping matches without a date or without the operator.
/// `day_of` turns the ISO date into the local day (browser time zone).
pub fn operator_games<'a>(
    matches: &'a [MatchSummary],
    names: &[String],
    day_of: impl Fn(&str) -> String,
) -> Vec<OpGame<'a>> {
    matches
        .iter()
        .filter_map(|m| {
            let me = match_operator(m.players.as_deref().unwrap_or(&[]), names)?;
            let at = m.played_at.as_deref()?;
            Some(OpGame {
                m,
                me,
                won: me.team.is_some() && m.winner == me.team,
                day: day_of(at),
            })
        })
        .collect()
}

/// The session: today's games, else the last day played. Returns `(games, is_today)`.
pub fn session_games<'a>(all: &[OpGame<'a>], today: &str) -> (Vec<OpGame<'a>>, bool) {
    let todays: Vec<OpGame> = all.iter().filter(|g| g.day == today).cloned().collect();
    if !todays.is_empty() {
        return (todays, true);
    }
    match all.first() {
        Some(first) => (
            all.iter().filter(|g| g.day == first.day).cloned().collect(),
            false,
        ),
        None => (vec![], false),
    }
}

/// `currentStreak`: +n wins / -n losses in a row from the most recent game.
pub fn current_streak(won: &[bool]) -> i32 {
    let Some(&first) = won.first() else { return 0 };
    let n = won.iter().take_while(|w| **w == first).count() as i32;
    if first {
        n
    } else {
        -n
    }
}

/// `streakLabel` of the queue panel.
pub fn streak_label(s: i32) -> String {
    match s {
        0 => "no streak".into(),
        s if s > 0 => format!("W{s} streak ▲"),
        s => format!("L{} streak ▼", -s),
    }
}

/// The milestones the ticker's streak flash celebrates (absolute length).
pub const STREAK_MILESTONES: [i32; 4] = [3, 5, 7, 10];

/// Milestone just reached by `streak` (its absolute value exactly matches one), if any — the ticker flashes a
/// caption on these exact lengths, not every game of a longer streak.
pub fn streak_milestone(streak: i32) -> Option<i32> {
    STREAK_MILESTONES.into_iter().find(|m| streak.abs() == *m)
}

/// Caption of the milestone flash: `"5-game win streak"` / `"3-game loss streak"`.
pub fn streak_milestone_caption(streak: i32) -> Option<String> {
    streak_milestone(streak).map(|m| {
        format!(
            "{m}-game {} streak",
            if streak > 0 { "win" } else { "loss" }
        )
    })
}

/// JS `toFixed(1)` for a non-negative number. Rust rounds an exact tie to even (`2.25 -> "2.2"`), JS rounds it
/// up (`"2.3"`). With one decimal an exact tie is a number whose fraction is `.25` or `.75`: the only ones
/// exactly representable in binary (a x4 is exact, so the test has no rounding of its own).
pub fn to_fixed1(x: f64) -> String {
    let tie = (x * 4.0).fract() == 0.0 && (x * 2.0).fract() != 0.0;
    if tie {
        format!("{:.1}", (x * 10.0).round() / 10.0)
    } else {
        format!("{x:.1}")
    }
}

/// K/A/D of a player: assists = takedowns - kills (never negative).
pub fn kad(p: &MatchPlayer) -> (u32, u32, u32) {
    let k = p.kills.unwrap_or(0);
    let td = p.takedowns.unwrap_or(0);
    (k, td.saturating_sub(k), p.deaths.unwrap_or(0))
}

/// Kill participation of `me` in `m`: takedowns / opposing deaths, capped at 100. `None` without opponent deaths.
pub fn kill_participation(players: &[MatchPlayer], me: &MatchPlayer) -> Option<u32> {
    let team_kills: u32 = players
        .iter()
        .filter(|p| me.team.is_some() && p.team.is_some() && p.team != me.team)
        .map(|p| p.deaths.unwrap_or(0))
        .sum();
    if team_kills == 0 {
        return None;
    }
    let td = me.takedowns.unwrap_or(0) as f64;
    Some(((td / team_kills as f64 * 100.0).round() as u32).min(100))
}

/// Win-loss record per key, in order of first appearance sorted by games played (stable, descending).
pub fn record_by<K: Clone + PartialEq>(
    games: &[OpGame],
    key: impl Fn(&OpGame) -> K,
) -> Vec<(K, u32, u32)> {
    let mut out: Vec<(K, u32, u32)> = vec![];
    for g in games {
        let k = key(g);
        let i = match out.iter().position(|(x, _, _)| *x == k) {
            Some(i) => i,
            None => {
                out.push((k, 0, 0));
                out.len() - 1
            }
        };
        if g.won {
            out[i].1 += 1;
        } else {
            out[i].2 += 1;
        }
    }
    out.sort_by_key(|(_, w, l)| std::cmp::Reverse(w + l));
    out
}

/// Aggregates of the session block of the queue panel.
#[derive(Clone, Debug, PartialEq)]
pub struct SessionTotals {
    pub kills: u32,
    pub assists: u32,
    pub deaths: u32,
    pub takedowns: u32,
    pub kda: f64,
    pub avg_len: Option<f64>,
}

pub fn session_totals(games: &[OpGame]) -> SessionTotals {
    let sum = |f: fn(&MatchPlayer) -> u32| games.iter().map(|g| f(g.me)).sum::<u32>();
    let kills = sum(|p| p.kills.unwrap_or(0));
    let deaths = sum(|p| p.deaths.unwrap_or(0));
    let takedowns = sum(|p| p.takedowns.unwrap_or(0));
    let assists = takedowns.saturating_sub(kills);
    let ka = (kills + assists) as f64;
    let kda = if deaths > 0 { ka / deaths as f64 } else { ka };
    let lengths: Vec<f64> = games.iter().filter_map(|g| g.m.length).collect();
    let avg_len = (!lengths.is_empty()).then(|| lengths.iter().sum::<f64>() / lengths.len() as f64);
    SessionTotals {
        kills,
        assists,
        deaths,
        takedowns,
        kda,
        avg_len,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(name: &str, team: i64, hero: &str, k: u32, td: u32, d: u32) -> MatchPlayer {
        MatchPlayer {
            toon: None,
            name: Some(name.into()),
            hero: Some(hero.into()),
            team: Some(team),
            kills: Some(k),
            takedowns: Some(td),
            deaths: Some(d),
            award: None,
            win: None,
        }
    }

    fn m(id: i64, day: &str, winner: i64, me: MatchPlayer) -> MatchSummary {
        MatchSummary {
            id,
            map: Some("Cursed Hollow".into()),
            mode: Some(50091),
            played_at: Some(day.into()),
            length: Some(600.0),
            winner: Some(winner),
            players: Some(vec![me, p("enemy", 1, "Zeratul", 3, 5, 6)]),
            perspective_toon: None,
        }
    }

    #[test]
    fn small_formatters_match_the_react_ones() {
        assert_eq!(fmt_dur(Some(754.4)), "12:34");
        assert_eq!(fmt_dur(Some(0.0)), "—");
        assert_eq!(fmt_dur(None), "—");
        assert_eq!(mode_badge(Some(50091)), ("SL", "b-sl"));
        assert_eq!(mode_badge(Some(7)), ("—", "b-qm"));
        assert_eq!(mode_badge(None), ("—", "b-qm"));
        assert_eq!(
            map_image(Some("Garden of Terror")).as_deref(),
            Some("/images/battlegrounds/garden-of-terror.png")
        );
        assert_eq!(map_slug("Dragon  Shire's"), "dragon-shires");
        assert_eq!(map_slug("Checkpoint: Hanamura"), "checkpoint-hanamura");
        assert_eq!(map_image(None), None);
    }

    #[test]
    fn awards_are_decamelised_and_mvp_is_flagged() {
        let mvp = award_label(Some("EndOfMatchAwardMVPBoolean")).unwrap();
        assert!(mvp.mvp && mvp.label == "MVP" && mvp.icon == "👑");
        let a = award_label(Some("EndOfMatchAwardMostHeroDamageDoneBoolean")).unwrap();
        assert_eq!(
            (a.label.as_str(), a.icon, a.mvp),
            ("Hero Damage", "🗡️", false)
        );
        let z = award_label(Some("EndOfMatchAward0DeathsBoolean")).unwrap();
        assert_eq!((z.label.as_str(), z.icon), ("0Deaths", "😇"));
        assert_eq!(award_label(None), None);
        assert_eq!(award_label(Some("")), None);
        assert_eq!(
            award_label(Some("EndOfMatchAwardNewThingBoolean"))
                .unwrap()
                .icon,
            "🏅"
        );
    }

    #[test]
    fn operator_lookup_prefers_the_override_then_names_then_first() {
        let ps = vec![p("Alpha", 0, "A", 0, 0, 0), p("Matella", 1, "B", 0, 0, 0)];
        let names = vec!["matella".to_string()];
        assert_eq!(
            pick_operator(&ps, Some("alpha"), &names)
                .unwrap()
                .hero
                .as_deref(),
            Some("A")
        );
        assert_eq!(
            pick_operator(&ps, None, &names).unwrap().hero.as_deref(),
            Some("B")
        );
        assert_eq!(
            pick_operator(&ps, Some("nobody"), &[])
                .unwrap()
                .hero
                .as_deref(),
            Some("A")
        );
        assert_eq!(match_operator(&ps, &[]), None); // strict: no fallback
        assert!(match_operator(&ps, &names).is_some());
    }

    #[test]
    fn jarvis_is_deterministic_and_follows_the_context_fr() {
        // Same priority order and text as before the trigger expansion (mvp/nemesis/best_kda
        // false, no map, Fr voice) — regression check that the old buckets are unchanged.
        assert_eq!(
            jarvis_phrase(
                true,
                Some("Valla"),
                0,
                4,
                0,
                false,
                false,
                false,
                None,
                BriefVoice::Fr
            ),
            Some("Aucune mort. Une partie de maître, monsieur.".to_string())
        );
        assert_eq!(
            jarvis_phrase(
                true,
                Some("Valla"),
                0,
                5,
                0,
                false,
                false,
                false,
                None,
                BriefVoice::Fr
            ),
            Some("Zéro mort — proprement exécuté.".to_string())
        );
        assert_eq!(
            jarvis_phrase(
                true,
                None,
                3,
                15,
                0,
                false,
                false,
                false,
                None,
                BriefVoice::Fr
            ),
            Some("Démonstration. ce héros était intenable.".to_string())
        );
        assert_eq!(
            jarvis_phrase(
                true,
                None,
                3,
                4,
                4,
                false,
                false,
                false,
                None,
                BriefVoice::Fr
            ),
            Some("Série de 4. Difficile de faire mieux.".to_string())
        );
        assert_eq!(
            jarvis_phrase(
                false,
                None,
                9,
                3,
                0,
                false,
                false,
                false,
                None,
                BriefVoice::Fr
            ),
            Some("Trop de morts, monsieur. On respire au prochain.".to_string())
        );
        assert_eq!(
            jarvis_phrase(
                false,
                Some("Valla"),
                1,
                3,
                -3,
                false,
                false,
                false,
                None,
                BriefVoice::Fr
            ),
            Some("Série difficile, monsieur. Gardons la tête froide.".to_string())
        );
        assert_eq!(
            jarvis_phrase(
                false,
                Some("Valla"),
                1,
                2,
                0,
                false,
                false,
                false,
                None,
                BriefVoice::Fr
            ),
            Some("Défaite serrée. Valla méritait mieux.".to_string())
        );
    }

    #[test]
    fn jarvis_mvp_outranks_every_other_win_or_loss_trigger() {
        // win, streak >= 3 AND mvp: mvp wins
        let won = jarvis_phrase(
            true,
            Some("Valla"),
            0,
            4,
            5,
            true,
            false,
            false,
            None,
            BriefVoice::Fr,
        );
        assert!(won.unwrap().contains("MVP"));
        // loss, mvp still produces a line (worth noting even in defeat)
        let lost = jarvis_phrase(
            false,
            Some("Valla"),
            1,
            2,
            0,
            true,
            false,
            false,
            None,
            BriefVoice::Fr,
        );
        assert!(lost.unwrap().to_lowercase().contains("mvp"));
    }

    #[test]
    fn jarvis_beat_nemesis_outranks_streak_but_needs_a_win() {
        let line = jarvis_phrase(
            true,
            Some("Valla"),
            1,
            4,
            5,
            false,
            true,
            false,
            None,
            BriefVoice::Fr,
        )
        .unwrap();
        assert!(
            line.to_lowercase().contains("némésis") || line.to_lowercase().contains("revanche")
        );
        // beat_nemesis ignored on a loss: falls through to the usual loss buckets
        assert_eq!(
            jarvis_phrase(
                false,
                Some("Valla"),
                1,
                2,
                0,
                false,
                true,
                false,
                None,
                BriefVoice::Fr
            ),
            Some("Défaite serrée. Valla méritait mieux.".to_string())
        );
    }

    #[test]
    fn jarvis_session_best_kda_outranks_no_death_and_takedowns_but_needs_a_win() {
        let line = jarvis_phrase(
            true,
            Some("Valla"),
            1,
            4,
            0,
            false,
            false,
            true,
            None,
            BriefVoice::Fr,
        )
        .unwrap();
        assert!(line.contains("session") || line.contains("soirée") || line.contains("Record"));
        // ignored on a loss
        assert_eq!(
            jarvis_phrase(
                false,
                Some("Valla"),
                1,
                2,
                0,
                false,
                false,
                true,
                None,
                BriefVoice::Fr
            ),
            Some("Défaite serrée. Valla méritait mieux.".to_string())
        );
    }

    #[test]
    fn jarvis_map_is_spliced_into_one_variant_per_bucket_when_known() {
        // plain win: 3 variants with no map, a 4th (map-aware) appended when the map is known —
        // seed 3 lands on that appended slot (`3 % 4 == 3`) without disturbing the no-map indices
        // (`3 % 3 == 0`, still the original first variant).
        let line = jarvis_phrase(
            true,
            Some("Valla"),
            3,
            0,
            0,
            false,
            false,
            false,
            Some("Cursed Hollow"),
            BriefVoice::Fr,
        )
        .unwrap();
        assert!(line.contains("Cursed Hollow"));
        let no_map = jarvis_phrase(
            true,
            Some("Valla"),
            3,
            0,
            0,
            false,
            false,
            false,
            None,
            BriefVoice::Fr,
        )
        .unwrap();
        assert!(!no_map.contains("Cursed Hollow"));
        // empty map string treated as absent (same as no map: 3 variants, index 0)
        let empty_map = jarvis_phrase(
            true,
            Some("Valla"),
            3,
            0,
            0,
            false,
            false,
            false,
            Some(""),
            BriefVoice::Fr,
        )
        .unwrap();
        assert_eq!(empty_map, no_map);
    }

    #[test]
    fn jarvis_off_voice_is_always_none() {
        assert_eq!(
            jarvis_phrase(
                true,
                Some("Valla"),
                0,
                20,
                5,
                true,
                true,
                true,
                Some("Braxis"),
                BriefVoice::Off
            ),
            None
        );
        assert_eq!(
            jarvis_phrase(
                false,
                None,
                10,
                0,
                -5,
                false,
                false,
                false,
                None,
                BriefVoice::Off
            ),
            None
        );
    }

    #[test]
    fn jarvis_en_voice_is_a_fresh_phrase_bank_not_a_translation() {
        let line = jarvis_phrase(
            true,
            Some("Valla"),
            0,
            5,
            0,
            false,
            false,
            false,
            None,
            BriefVoice::En,
        )
        .unwrap();
        // no French words leak into the English bank
        assert!(!line.contains("monsieur") && !line.contains("Zéro"));
        assert_eq!(line, "Zero deaths — cleanly done.");
    }

    #[test]
    fn session_best_kda_needs_a_comparison_and_the_top_game() {
        let names = vec!["me".to_string()];
        // first game of the session: nothing to compare against yet
        let ms = vec![m(1, "2026-11-10", 0, p("me", 0, "A", 10, 20, 0))];
        let all = operator_games(&ms, &names, |s| s.to_string());
        assert!(!is_session_best_kda(&all));

        // two games, the most recent (first in the list) has the better KDA
        let ms2 = vec![
            m(2, "2026-11-10", 0, p("me", 0, "A", 10, 20, 1)), // KDA 20
            m(1, "2026-11-10", 0, p("me", 0, "A", 1, 2, 1)),   // KDA 2
        ];
        let all2 = operator_games(&ms2, &names, |s| s.to_string());
        assert!(is_session_best_kda(&all2));

        // the most recent is worse than an earlier game: not the session best
        let ms3 = vec![
            m(2, "2026-11-10", 0, p("me", 0, "A", 1, 2, 1)), // KDA 2
            m(1, "2026-11-10", 0, p("me", 0, "A", 10, 20, 1)), // KDA 20
        ];
        let all3 = operator_games(&ms3, &names, |s| s.to_string());
        assert!(!is_session_best_kda(&all3));
    }

    #[test]
    fn beat_nemesis_needs_a_win_and_a_tracked_toon_in_the_enemy_roster() {
        let enemies = vec!["foe-a".to_string(), "foe-b".to_string()];
        assert!(beat_nemesis(true, &enemies, Some("foe-a")));
        assert!(!beat_nemesis(false, &enemies, Some("foe-a"))); // loss: no credit
        assert!(!beat_nemesis(true, &enemies, Some("foe-z"))); // not in this roster
        assert!(!beat_nemesis(true, &enemies, None)); // no tracked nemesis
    }

    #[test]
    fn session_is_today_else_the_last_day_played() {
        let names = vec!["me".to_string()];
        let ms = vec![
            m(3, "2026-11-10T20:00:00Z", 0, p("me", 0, "A", 5, 8, 2)),
            m(2, "2026-11-10T19:00:00Z", 1, p("me", 0, "B", 1, 2, 6)),
            m(1, "2026-11-09T19:00:00Z", 0, p("me", 0, "C", 0, 0, 0)),
            m(0, "2026-11-09T18:00:00Z", 0, p("someone", 0, "C", 0, 0, 0)), // not the operator
        ];
        let day = |s: &str| s[..10].to_string();
        let all = operator_games(&ms, &names, day);
        assert_eq!(all.len(), 3);
        assert!(all[0].won && !all[1].won);
        let (today, is_today) = session_games(&all, "2026-11-10");
        assert!(is_today && today.len() == 2);
        let (last, is_today) = session_games(&all, "2026-11-12");
        assert!(!is_today && last.len() == 2 && last[0].day == "2026-11-10");
        assert_eq!(session_games(&[], "2026-11-12"), (vec![], false));
    }

    #[test]
    fn streaks_records_and_totals() {
        assert_eq!(current_streak(&[]), 0);
        assert_eq!(current_streak(&[true, true, false, true]), 2);
        assert_eq!(current_streak(&[false, false, false, true]), -3);
        assert_eq!(streak_label(0), "no streak");
        assert_eq!(streak_label(2), "W2 streak ▲");
        assert_eq!(streak_label(-3), "L3 streak ▼");

        let names = vec!["me".to_string()];
        let ms = vec![
            m(3, "2026-11-10", 0, p("me", 0, "A", 5, 8, 2)),
            m(2, "2026-11-10", 1, p("me", 0, "B", 1, 2, 6)),
            m(1, "2026-11-10", 0, p("me", 0, "A", 4, 6, 0)),
        ];
        let all = operator_games(&ms, &names, |s| s.to_string());
        let heroes = record_by(&all, |g| g.me.hero.clone().unwrap_or_default());
        assert_eq!(
            heroes,
            vec![("A".to_string(), 2, 0), ("B".to_string(), 0, 1)]
        );
        let t = session_totals(&all);
        assert_eq!((t.kills, t.assists, t.deaths, t.takedowns), (10, 6, 8, 16));
        assert!((t.kda - 2.0).abs() < 1e-9);
        assert_eq!(t.avg_len, Some(600.0));
    }

    #[test]
    fn to_fixed1_rounds_ties_up_like_javascript() {
        assert_eq!(to_fixed1(2.25), "2.3");
        assert_eq!(to_fixed1(0.15), "0.1"); // below the tie in binary
        assert_eq!(to_fixed1(2.0), "2.0");
        assert_eq!(to_fixed1(2.5), "2.5");
        assert_eq!(to_fixed1(3.46), "3.5");
        assert_eq!(to_fixed1(0.0), "0.0");
        assert_eq!(to_fixed1(12.75), "12.8");
        assert_eq!(to_fixed1(0.125), "0.1"); // not a tie at one decimal
    }

    #[test]
    fn kad_and_kill_participation() {
        let me = p("me", 0, "A", 3, 10, 2);
        assert_eq!(kad(&me), (3, 7, 2));
        assert_eq!(kad(&p("x", 0, "A", 5, 3, 1)), (5, 0, 1)); // never negative
        let ps = vec![
            me.clone(),
            p("e1", 1, "B", 0, 0, 8),
            p("e2", 1, "C", 0, 0, 4),
        ];
        assert_eq!(kill_participation(&ps, &me), Some(83));
        assert_eq!(kill_participation(&ps[..1], &me), None);
        let big = p("me", 0, "A", 3, 40, 2);
        assert_eq!(kill_participation(&ps, &big), Some(100));
    }

    #[test]
    fn streak_milestones_fire_only_on_the_exact_lengths() {
        assert_eq!(streak_milestone(0), None);
        assert_eq!(streak_milestone(2), None);
        assert_eq!(streak_milestone(3), Some(3));
        assert_eq!(streak_milestone(-3), Some(3));
        assert_eq!(streak_milestone(5), Some(5));
        assert_eq!(streak_milestone(7), Some(7));
        assert_eq!(streak_milestone(10), Some(10));
        assert_eq!(streak_milestone(-10), Some(10));
        // past the last milestone and between milestones: no flash
        assert_eq!(streak_milestone(4), None);
        assert_eq!(streak_milestone(11), None);
        assert_eq!(
            streak_milestone_caption(5).as_deref(),
            Some("5-game win streak")
        );
        assert_eq!(
            streak_milestone_caption(-7).as_deref(),
            Some("7-game loss streak")
        );
        assert_eq!(streak_milestone_caption(4), None);
    }
}
