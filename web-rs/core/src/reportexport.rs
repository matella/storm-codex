//! Export HTML of a scouting report: one self-contained file to send to teammates. Port of `reportHtml.ts`.
//!
//! PURE rendering: every image arrives already as a `data:` URI through [`ExportAssets`], and all text coming from the
//! LLM or from the replays is escaped (the file travels outside the app). The browser part (fetching and shrinking the
//! portraits, saving the file) lives in the app crate.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use crate::scouting::{
    compact_evidence, day, fmt_len, humanize, kn, map_anchor, maps_with_plans, pct, pid_names,
    Count, Evidence, Facts, HeroCall, HeroRow, MapFacts, MapPlan, Point, Rate, ScoutingReport,
};

const CSS: &str = include_str!("report_style.css");

/// Images of the export, all as `data:` URIs.
#[derive(Clone, Debug, Default)]
pub struct ExportAssets {
    /// hero → portrait (absent → initials medallion)
    pub hero: BTreeMap<String, String>,
    /// map → minimap (absent → gradient)
    pub map: BTreeMap<String, String>,
    /// hero → hex colour of the universe (ring of the portrait)
    pub ring: BTreeMap<String, String>,
}

// ── pure helpers ────────────────────────────────────────────────────────────

/// HTML escaping of text content and attribute values.
pub fn esc(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => o.push_str("&amp;"),
            '<' => o.push_str("&lt;"),
            '>' => o.push_str("&gt;"),
            '"' => o.push_str("&quot;"),
            '\'' => o.push_str("&#39;"),
            c => o.push(c),
        }
    }
    o
}

/// `k/n`, `—` without a sample.
fn rate(r: Option<Rate>) -> String {
    kn(r)
}

/// Fallback medallion: 2 letters (`JO` for Johanna, `SH` for Sgt. Hammer).
fn initials(h: &str) -> String {
    let cleaned: String = h
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == ' ')
        .collect();
    let words: Vec<&str> = cleaned.split_whitespace().collect();
    let s: String = if words.len() > 1 {
        words.iter().filter_map(|w| w.chars().next()).collect()
    } else {
        words.first().map(|w| w.to_string()).unwrap_or_default()
    };
    let s: String = s.chars().take(2).collect::<String>().to_uppercase();
    if s.is_empty() {
        "?".to_string()
    } else {
        s
    }
}

/// Lowercase ASCII slug: runs of other characters become one dash, edges trimmed.
fn slug(name: &str) -> String {
    let mut out = String::new();
    let mut dash = false;
    for c in name.to_lowercase().chars() {
        if c.is_ascii_lowercase() || c.is_ascii_digit() {
            out.push(c);
            dash = false;
        } else if !dash {
            out.push('-');
            dash = true;
        }
    }
    out.trim_matches('-').to_string()
}

/// Latin letter without its accent (what `normalize("NFD")` + stripping combining marks leaves).
fn deaccent(c: char) -> char {
    match c {
        'à' | 'á' | 'â' | 'ã' | 'ä' | 'å' | 'ā' | 'ă' | 'ą' => 'a',
        'ç' | 'ć' | 'č' => 'c',
        'è' | 'é' | 'ê' | 'ë' | 'ē' | 'ě' | 'ę' => 'e',
        'ì' | 'í' | 'î' | 'ï' | 'ī' => 'i',
        'ñ' | 'ń' | 'ň' => 'n',
        'ò' | 'ó' | 'ô' | 'õ' | 'ö' | 'ō' | 'ő' => 'o',
        'ù' | 'ú' | 'û' | 'ü' | 'ū' | 'ů' | 'ű' => 'u',
        'ý' | 'ÿ' => 'y',
        'š' | 'ś' => 's',
        'ž' | 'ź' | 'ż' => 'z',
        c => c,
    }
}

/// File name: `scouting-<title>.html`.
pub fn export_file_name(title: &str) -> String {
    let plain: String = title.to_lowercase().chars().map(deaccent).collect();
    let s = slug(&plain);
    format!("scouting-{}.html", if s.is_empty() { "report" } else { &s })
}

/// Every hero the report mentions (facts + plan), sorted — to load their portraits.
pub fn heroes_of(r: &ScoutingReport) -> Vec<String> {
    let mut set = BTreeSet::new();
    let mut add = |h: &str| {
        if !h.is_empty() {
            set.insert(h.to_string());
        }
    };
    let f = r.snapshot.as_ref().map(|s| &s.facts);
    if let Some(f) = f {
        for p in &f.players {
            p.heroes.iter().for_each(|h| add(&h.hero));
        }
        for m in &f.maps {
            m.picks.iter().for_each(|h| add(&h.hero));
            m.bans.iter().for_each(|c| add(&c.key));
            m.bans_against.iter().for_each(|c| add(&c.key));
        }
        let d = &f.draft;
        for l in [
            &d.bans_first,
            &d.bans_mid,
            &d.bans_against,
            &d.openers,
            &d.last_picks,
        ] {
            l.iter().for_each(|c| add(&c.key));
        }
        d.faced.iter().for_each(|h| add(&h.hero));
        for g in &f.games {
            g.picks.iter().for_each(|p| add(&p.hero));
            g.opp_picks.iter().for_each(|p| add(&p.hero));
        }
    }
    if let Some(a) = &r.analysis {
        for m in &a.maps {
            for l in [&m.bans, &m.picks, &m.their_picks] {
                l.iter().for_each(|h| add(&h.hero));
            }
        }
        a.general.bans.iter().for_each(|h| add(&h.hero));
        a.general.picks.iter().for_each(|h| add(&h.hero));
    }
    set.into_iter().collect()
}

/// Every map the report mentions (facts + plan), sorted — to load their minimaps.
pub fn maps_of(r: &ScoutingReport) -> Vec<String> {
    let mut set = BTreeSet::new();
    if let Some(s) = &r.snapshot {
        s.facts.maps.iter().for_each(|m| {
            set.insert(m.map.clone());
        });
    }
    if let Some(a) = &r.analysis {
        a.maps.iter().for_each(|m| {
            set.insert(m.map.clone());
        });
        a.map_choice.pick.iter().for_each(|m| {
            set.insert(m.map.clone());
        });
        a.map_choice.avoid.iter().for_each(|m| {
            set.insert(m.map.clone());
        });
    }
    set.into_iter().collect()
}

// ── visual bricks ───────────────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tone {
    Ban,
    Pick,
    Them,
    Plain,
}

impl Tone {
    fn name(self) -> &'static str {
        match self {
            Tone::Ban => "ban",
            Tone::Pick => "pick",
            Tone::Them => "them",
            Tone::Plain => "plain",
        }
    }
}

/// CSS class of a hero / a map: the image is embedded ONCE (asset sheet), every use references it.
fn cls(prefix: &str, name: &str) -> String {
    format!("{prefix}{}", slug(name))
}

type Names = HashMap<String, String>;

fn portrait(hero: &str, a: &ExportAssets, size: u32, tone: Tone) -> String {
    let ring = a.ring.get(hero).map_or("#afa9ec", String::as_str);
    let has = a.hero.get(hero).is_some_and(|u| !u.is_empty());
    let inner = if has {
        String::new()
    } else {
        format!(r#"<span class="ini">{}</span>"#, esc(&initials(hero)))
    };
    let img = if has {
        format!(" {}", cls("h-", hero))
    } else {
        String::new()
    };
    let slash = if tone == Tone::Ban {
        r#"<i class="slash"></i>"#
    } else {
        ""
    };
    format!(
        r#"<span class="pt pt-{}{img}" style="--s:{size}px;--ring:{ring}" role="img" aria-label="{h}" title="{h}">{inner}{slash}</span>"#,
        tone.name(),
        h = esc(hero),
    )
}

/// `data:image/…;base64,…` and nothing else (the URIs come from the export itself, never from the LLM).
fn safe_data_uri(u: &str) -> bool {
    let Some(rest) = u.strip_prefix("data:image/") else {
        return false;
    };
    let Some((kind, b64)) = rest.split_once(";base64,") else {
        return false;
    };
    !kind.is_empty()
        && kind.chars().all(|c| c.is_ascii_lowercase() || c == '+')
        && !b64.is_empty()
        && b64
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '/' | '='))
}

/// Asset sheet: one rule per portrait and per map.
fn asset_css(a: &ExportAssets) -> String {
    let heroes = a
        .hero
        .iter()
        .filter(|(_, u)| safe_data_uri(u))
        .map(|(h, u)| format!(".{}{{background-image:url('{u}')}}", cls("h-", h)));
    let maps = a
        .map
        .iter()
        .filter(|(_, u)| safe_data_uri(u))
        .map(|(m, u)| format!(".{}{{--img:url('{u}')}}", cls("m-", m)));
    heroes.chain(maps).collect::<Vec<_>>().join("\n")
}

#[derive(Clone, Copy)]
struct Ctx<'a> {
    names: &'a Names,
    map: Option<&'a str>,
}

fn evidence_line(ev: &[Evidence], unsupported: bool, ctx: Ctx) -> String {
    if unsupported {
        return r#"<div class="ev ev-bad">⚠ unverified — no supporting fact</div>"#.to_string();
    }
    let known: Vec<&Evidence> = ev.iter().filter(|e| e.known).take(2).collect();
    if known.is_empty() {
        return String::new();
    }
    let spans: String = known
        .iter()
        .map(|e| {
            let (label, text) = compact_evidence(e, ctx.names, ctx.map);
            format!(
                "<span{}>{} <b>{}</b></span>",
                if e.low.unwrap_or(false) {
                    r#" class="low""#
                } else {
                    ""
                },
                esc(&label),
                esc(&text)
            )
        })
        .collect();
    format!(r#"<div class="ev">{spans}</div>"#)
}

fn pips(games: &[&crate::scouting::GameRow]) -> String {
    let items: String = games
        .iter()
        .map(|g| format!(r#"<i class="{}"></i>"#, if g.won { "w" } else { "l" }))
        .collect();
    format!(r#"<span class="pips">{items}</span>"#)
}

fn meter(conf: Option<&str>) -> String {
    let lv = match conf {
        Some("high") => 3,
        Some("medium") => 2,
        Some("low") => 1,
        _ => 0,
    };
    if lv == 0 {
        return String::new();
    }
    let c = esc(conf.unwrap_or_default());
    let on = |n: i32| if lv >= n { "on" } else { "" };
    format!(
        r#"<span class="meter" title="confidence: {c}"><i class="{}"></i><i class="{}"></i><i class="{}"></i><em>{c} confidence</em></span>"#,
        on(1),
        on(2),
        on(3)
    )
}

fn call_card(
    h: &HeroCall,
    tone: Tone,
    a: &ExportAssets,
    names: &Names,
    map: Option<&str>,
) -> String {
    let tag = if tone == Tone::Ban {
        let t = match h.phase.as_deref() {
            Some("mid") => "MID BAN",
            Some("first") => "1ST PHASE",
            _ => "BAN",
        };
        format!(r#"<span class="tag tag-ban">{t}</span>"#)
    } else if tone == Tone::Them && h.player.as_deref().is_some_and(|p| !p.is_empty()) {
        let p = h.player.as_deref().unwrap_or_default();
        format!(
            r#"<span class="tag tag-them">{}</span>"#,
            esc(names.get(p).map_or(p, String::as_str))
        )
    } else {
        String::new()
    };
    format!(
        r#"<div class="call call-{tone}">
    {portrait}
    <div class="call-body">
      <div class="call-hd"><strong>{hero}</strong>{tag}</div>
      <p>{why}</p>
      {ev}
    </div>
  </div>"#,
        tone = tone.name(),
        portrait = portrait(&h.hero, a, 64, tone),
        hero = esc(&h.hero),
        why = esc(&humanize(&h.why, names)),
        ev = evidence_line(&h.evidence, h.unsupported, Ctx { names, map }),
    )
}

fn lane(title: &str, class: &str, items: &[String], empty: &str) -> String {
    let body = if items.is_empty() {
        format!(r#"<div class="lane-empty">{empty}</div>"#)
    } else {
        items.concat()
    };
    format!(r#"<div class="lane {class}"><div class="lane-hd">{title}</div>{body}</div>"#)
}

fn points(ps: &[Point], ctx: Ctx) -> String {
    if ps.is_empty() {
        return String::new();
    }
    let items: String = ps
        .iter()
        .map(|p| {
            format!(
                "<li><span>{}</span>{}</li>",
                esc(&humanize(&p.point, ctx.names)),
                evidence_line(&p.evidence, p.unsupported, ctx)
            )
        })
        .collect();
    format!(r#"<ul class="consider">{items}</ul>"#)
}

fn chips(list: &[Count], a: &ExportAssets, tone: Tone) -> String {
    if list.is_empty() {
        return r#"<span class="none">—</span>"#.to_string();
    }
    list.iter()
        .take(6)
        .map(|c| {
            format!(
                r#"<span class="chip">{}<span>{}</span><b>{}×</b></span>"#,
                portrait(&c.key, a, 26, tone),
                esc(&c.key),
                c.count.k
            )
        })
        .collect()
}

fn pick_chips(list: &[HeroRow], a: &ExportAssets, names: &Names) -> String {
    if list.is_empty() {
        return r#"<span class="none">—</span>"#.to_string();
    }
    list.iter()
        .map(|h| {
            let mut who: Vec<String> = Vec::new();
            for p in &h.by {
                let n = names.get(p).cloned().unwrap_or_else(|| p.clone());
                if !who.contains(&n) {
                    who.push(n);
                }
            }
            let by = if who.is_empty() {
                String::new()
            } else {
                format!(" <em>{}</em>", esc(&who.join(", ")))
            };
            format!(
                r#"<span class="chip">{}<span>{}{by}</span><b>{}×</b></span>"#,
                portrait(&h.hero, a, 26, Tone::Plain),
                esc(&h.hero),
                h.picks.k
            )
        })
        .collect()
}

// ── sections ────────────────────────────────────────────────────────────────

fn cover(r: &ScoutingReport, f: Option<&Facts>, a: &ExportAssets, generated_at: &str) -> String {
    let o = f.map(|f| &f.overview);
    let bg_map = f.and_then(|f| f.maps.first()).map(|m| m.map.as_str());
    let bg = match bg_map {
        Some(m) if a.map.get(m).is_some_and(|u| !u.is_empty()) => cls("m-", m),
        _ => String::new(),
    };
    let games: Vec<&crate::scouting::GameRow> =
        f.map(|f| f.games.iter().collect()).unwrap_or_default();
    let n_games = o.map_or(0, |o| o.games);
    let low = n_games < 5;
    let wl = o.map_or("—".to_string(), |o| {
        format!("{}<span>–</span>{}", o.record.k, o.record.n - o.record.k)
    });
    let first_pick = f.map(|f| f.draft.first_pick);
    let fp_pct = pct(first_pick);
    let range = o.map_or(String::new(), |o| {
        format!(
            "{} → {}",
            day(o.first_date.as_deref()),
            day(o.last_date.as_deref())
        )
    });
    let builds = o
        .map(|o| {
            o.builds
                .iter()
                .map(i64::to_string)
                .collect::<Vec<_>>()
                .join(", ")
        })
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "?".to_string());
    let sample = if low {
        format!(
            r#"<div class="sample">⚠ Small sample — {n_games} game{}. Treat trends as hints, not certainties.</div>"#,
            if n_games == 1 { "" } else { "s" }
        )
    } else {
        String::new()
    };
    let roster: Vec<&crate::scouting::PlayerFacts> = f
        .map(|f| f.players.iter().filter(|p| p.core).collect())
        .unwrap_or_default();
    let roster_html = if roster.is_empty() {
        String::new()
    } else {
        let items: String = roster
            .iter()
            .map(|p| {
                let top = p.heroes.first().map(|h| h.hero.as_str());
                let role = p.roles.first().map_or("", |c| c.key.as_str());
                let heroes = p
                    .heroes
                    .iter()
                    .take(3)
                    .map(|h| esc(&h.hero))
                    .collect::<Vec<_>>()
                    .join(" · ");
                format!(
                    r#"<div class="rm">{}<div><strong>{}</strong><span>{}</span><em>{heroes}</em></div></div>"#,
                    top.map_or(String::new(), |t| portrait(t, a, 58, Tone::Plain)),
                    esc(&p.name),
                    esc(role)
                )
            })
            .collect();
        format!(r#"<div class="roster">{items}</div>"#)
    };
    format!(
        r#"<header class="cover {bg}">
    <div class="cover-grain"></div>
    <div class="cover-in">
      <div class="kicker">Scouting dossier · report #{id}</div>
      <h1>{target}</h1>
      <div class="subtitle">{title}</div>
      <div class="statline">
        <div class="stat big"><label>Record</label><strong>{wl}</strong>{pips}</div>
        <div class="stat"><label>Games</label><strong>{n_games}</strong><small>{range}</small></div>
        <div class="stat"><label>Had first pick</label><strong>{fp}{fp_pct_sign}</strong><small>{fp_rate}</small></div>
        <div class="stat"><label>Avg length</label><strong>{avg}</strong><small>builds {builds}</small></div>
        {sample}
      </div>
      {roster_html}
    </div>
    <div class="cover-meta">Generated {generated} · facts v{fv}{model}</div>
  </header>"#,
        id = r.id,
        target = esc(r.target_name.as_deref().unwrap_or("Unnamed team")),
        title = esc(&r.title),
        pips = pips(&games),
        fp = fp_pct.map_or("—".to_string(), |p| p.to_string()),
        fp_pct_sign = if fp_pct.is_some() {
            "<span>%</span>"
        } else {
            ""
        },
        fp_rate = rate(first_pick),
        avg = o.map_or("—".to_string(), |o| fmt_len(Some(o.avg_length_s))),
        builds = esc(&builds),
        generated = esc(generated_at),
        fv = r.facts_version,
        model = r
            .analysis_model
            .as_deref()
            .filter(|m| !m.is_empty())
            .map_or(String::new(), |m| format!(" · plan by {}", esc(m))),
    )
}

fn identity(r: &ScoutingReport, f: &Facts, a: &ExportAssets, names: &Names) -> String {
    let d = &f.draft;
    let summary = r
        .analysis
        .as_ref()
        .map(|x| x.summary.as_str())
        .filter(|s| !s.is_empty());
    let quote = match summary {
        Some(s) => format!("<blockquote>{}</blockquote>", esc(&humanize(s, names))),
        None => r#"<blockquote class="muted">No draft plan imported yet — the numbers below come straight from the replays.</blockquote>"#.to_string(),
    };
    format!(
        r#"<section class="sec reveal" id="identity">
    <div class="sec-hd"><span class="num">01</span><h2>Draft identity</h2></div>
    <div class="identity">
      {quote}
      <div class="tells">
        <div class="tell"><label>They ban first</label><div class="chips">{c1}</div></div>
        <div class="tell"><label>They ban mid-draft</label><div class="chips">{c2}</div></div>
        <div class="tell"><label>Their first pick</label><div class="chips">{c3}</div></div>
        <div class="tell"><label>Banned against them</label><div class="chips">{c4}</div></div>
      </div>
    </div>
  </section>"#,
        c1 = chips(&d.bans_first, a, Tone::Ban),
        c2 = chips(&d.bans_mid, a, Tone::Ban),
        c3 = chips(&d.openers, a, Tone::Them),
        c4 = chips(&d.bans_against, a, Tone::Plain),
    )
}

fn map_choice(r: &ScoutingReport, a: &ExportAssets, names: &Names) -> String {
    let Some(c) = r.analysis.as_ref().map(|x| &x.map_choice) else {
        return String::new();
    };
    if c.pick.is_empty() && c.avoid.is_empty() {
        return String::new();
    }
    let tile = |m: &crate::scouting::MapCall, go: bool| {
        let has = a.map.get(&m.map).is_some_and(|u| !u.is_empty());
        format!(
            r##"<a class="mtile mtile-{kind}{img}" href="#{anchor}">
      <span class="mtile-badge">{badge}</span>
      <strong>{map}</strong>
      <p>{why}</p>
      {ev}
    </a>"##,
            kind = if go { "go" } else { "no" },
            img = if has {
                format!(" {}", cls("m-", &m.map))
            } else {
                String::new()
            },
            anchor = map_anchor(&m.map),
            badge = if go { "PICK" } else { "AVOID" },
            map = esc(&m.map),
            why = esc(&humanize(&m.why, names)),
            ev = evidence_line(
                &m.evidence,
                m.unsupported,
                Ctx {
                    names,
                    map: Some(&m.map)
                }
            ),
        )
    };
    let tiles: String = c
        .pick
        .iter()
        .map(|m| tile(m, true))
        .chain(c.avoid.iter().map(|m| tile(m, false)))
        .collect();
    format!(
        r#"<section class="sec reveal" id="map-choice">
    <div class="sec-hd"><span class="num">02</span><h2>Map choice</h2><small>when we get to pick the map</small></div>
    <div class="mtiles">{tiles}</div>
  </section>"#
    )
}

fn map_board(
    map: &str,
    m: Option<&MapFacts>,
    plan: Option<&MapPlan>,
    f: &Facts,
    a: &ExportAssets,
    names: &Names,
    i: usize,
) -> String {
    let games: Vec<&crate::scouting::GameRow> = f.games.iter().filter(|g| g.map == map).collect();
    let img = if a.map.get(map).is_some_and(|u| !u.is_empty()) {
        cls("m-", map)
    } else {
        String::new()
    };
    let record = match m {
        Some(m) => format!(
            r#"<span class="rec">{}–{}</span>{}"#,
            m.record.k,
            m.record.n - m.record.k,
            pips(&games)
        ),
        None => r#"<span class="rec none">not in these replays</span>"#.to_string(),
    };
    let wil = m.and_then(|m| m.wilson).map_or(String::new(), |w| {
        format!(
            r#"<span class="ci">95% CI {}–{}%</span>"#,
            js_round(w[0]),
            js_round(w[1])
        )
    });
    let last_time = m.map_or(String::new(), |m| {
        format!(
            r#"<div class="lasttime">
      <div><label>They played</label><div class="chips">{}</div></div>
      <div><label>They banned</label><div class="chips">{}</div></div>
      <div><label>Banned against them</label><div class="chips">{}</div></div>
    </div>"#,
            pick_chips(&m.picks, a, names),
            chips(&m.bans, a, Tone::Ban),
            chips(&m.bans_against, a, Tone::Plain),
        )
    });
    let board = match plan {
        Some(p) => {
            let cards = |l: &[HeroCall], t: Tone| -> Vec<String> {
                l.iter().map(|h| call_card(h, t, a, names, Some(map))).collect()
            };
            let consider = if p.considerations.is_empty() {
                String::new()
            } else {
                format!(
                    r#"<div class="consider-wrap"><div class="lane-hd">Consider</div>{}</div>"#,
                    points(&p.considerations, Ctx { names, map: Some(map) })
                )
            };
            format!(
                r#"<div class="board">
        {}
        {}
        {}
      </div>
      {consider}"#,
                lane("We ban", "l-ban", &cards(&p.bans, Tone::Ban), "no ban suggested"),
                lane("We pick", "l-pick", &cards(&p.picks, Tone::Pick), "no pick suggested"),
                lane(
                    "They will likely pick",
                    "l-them",
                    &cards(&p.their_picks, Tone::Them),
                    "—"
                ),
            )
        }
        None => r##"<div class="noplan">No plan for this map — see <a href="#general">Any map</a>.</div>"##.to_string(),
    };
    let overview = match plan {
        Some(p) if !p.overview.is_empty() => format!(
            r#"<p class="overview">{}</p>{}"#,
            esc(&humanize(&p.overview, names)),
            evidence_line(
                &p.evidence,
                p.unsupported,
                Ctx {
                    names,
                    map: Some(map)
                }
            )
        ),
        _ => String::new(),
    };
    format!(
        r#"<article class="mboard reveal" id="{anchor}">
    <div class="mboard-hd {img}">
      <div class="mboard-title">
        <span class="idx">MAP {idx:02}</span>
        <h3>{name}</h3>
        <div class="mboard-rec">{record}{wil}</div>
      </div>
      {meter}
    </div>
    {overview}
    {board}
    {last_time}
  </article>"#,
        anchor = map_anchor(map),
        idx = i + 1,
        name = esc(map),
        meter = plan.map_or(String::new(), |p| meter(p.confidence.as_deref())),
    )
}

fn js_round(x: f64) -> i64 {
    (x + 0.5).floor() as i64
}

fn general(r: &ScoutingReport, a: &ExportAssets, names: &Names) -> String {
    let Some(g) = r.analysis.as_ref().map(|x| &x.general) else {
        return String::new();
    };
    if g.bans.len() + g.picks.len() + g.considerations.len() == 0 {
        return String::new();
    }
    let cards = |l: &[HeroCall], t: Tone| -> Vec<String> {
        l.iter().map(|h| call_card(h, t, a, names, None)).collect()
    };
    let consider = if g.considerations.is_empty() {
        String::new()
    } else {
        format!(
            r#"<div class="consider-wrap"><div class="lane-hd">Consider</div>{}</div>"#,
            points(&g.considerations, Ctx { names, map: None })
        )
    };
    format!(
        r#"<section class="sec reveal" id="general">
    <div class="sec-hd"><span class="num">04</span><h2>Any map</h2><small>applies everywhere, including maps missing from the replays</small></div>
    <div class="board board-2">
      {}
      {}
    </div>
    {consider}
  </section>"#,
        lane("We ban", "l-ban", &cards(&g.bans, Tone::Ban), "—"),
        lane("We pick", "l-pick", &cards(&g.picks, Tone::Pick), "—"),
    )
}

fn players(f: &Facts, a: &ExportAssets) -> String {
    if f.players.is_empty() {
        return String::new();
    }
    let cards: String = f
        .players
        .iter()
        .map(|p| {
            let max = p.heroes.iter().map(|h| h.picks.k).max().unwrap_or(0).max(1);
            let pool: String = p
                .heroes
                .iter()
                .map(|h| {
                    format!(
                        r#"<div class="pool-row">{}<span>{}</span>
            <i class="bar"><b style="width:{}%"></b></i><em>{}g · {}%</em></div>"#,
                        portrait(&h.hero, a, 24, Tone::Plain),
                        esc(&h.hero),
                        js_round(h.picks.k as f64 * 100.0 / max as f64),
                        h.picks.k,
                        pct(Some(h.record)).map_or("null".to_string(), |v| v.to_string()),
                    )
                })
                .collect();
            format!(
                r#"<div class="pcard">
        <div class="pcard-hd">{}
          <div><strong>{}</strong><span>{}{}</span><em>{} wins</em></div>
        </div>
        <div class="pool">{pool}</div>
        <div class="pstats">K/D/A <b>{:.1}/{:.1}/{:.1}</b> · dmg <b>{:.0}</b>/min · dead <b>{:.1}%</b></div>
      </div>"#,
                p.heroes
                    .first()
                    .map_or(String::new(), |h| portrait(&h.hero, a, 72, Tone::Plain)),
                esc(&p.name),
                esc(&p.roles.iter().map(|c| c.key.as_str()).collect::<Vec<_>>().join(" / ")),
                if p.core { "" } else { " · substitute" },
                rate(Some(p.record)),
                p.stats.kills,
                p.stats.deaths,
                p.stats.assists,
                p.stats.hero_damage_pm,
                p.stats.time_dead_pct,
            )
        })
        .collect();
    format!(
        r#"<section class="sec reveal" id="players">
    <div class="sec-hd"><span class="num">05</span><h2>Their players</h2></div>
    <div class="pcards">{cards}</div>
  </section>"#
    )
}

fn faced(f: &Facts, a: &ExportAssets) -> String {
    let mut list: Vec<&HeroRow> = f.draft.faced.iter().collect();
    // stable: lowest win rate against first, then most picked
    list.sort_by(|x, y| {
        let px = pct(Some(x.record)).unwrap_or(0);
        let py = pct(Some(y.record)).unwrap_or(0);
        px.cmp(&py).then(y.picks.k.cmp(&x.picks.k))
    });
    if list.is_empty() {
        return String::new();
    }
    let items: String = list
        .iter()
        .map(|h| {
            let p = pct(Some(h.record)).unwrap_or(0);
            let class = match p {
                0 => "fc-hot",
                100 => "fc-cold",
                _ => "",
            };
            format!(
                r#"<div class="fc {class}">{}<span>{}</span><em>{}</em></div>"#,
                portrait(&h.hero, a, 44, Tone::Plain),
                esc(&h.hero),
                rate(Some(h.record))
            )
        })
        .collect();
    format!(
        r#"<section class="sec reveal" id="faced">
    <div class="sec-hd"><span class="num">06</span><h2>What they faced</h2><small>heroes picked against them · their record in those games · <b class="lg-hot">green</b> = they never beat it</small></div>
    <div class="faced">{items}</div>
  </section>"#
    )
}

fn games_log(f: &Facts, a: &ExportAssets, names: &Names) -> String {
    if f.games.is_empty() {
        return String::new();
    }
    let row = |ps: &[crate::scouting::GamePick], mine: bool| {
        let items: String = ps
            .iter()
            .map(|p| {
                let who = if mine {
                    names
                        .get(&p.player)
                        .map_or(p.player.as_str(), String::as_str)
                } else {
                    p.player.as_str()
                };
                format!(
                    "<span>{}<em>{}</em></span>",
                    portrait(&p.hero, a, 30, Tone::Plain),
                    esc(who)
                )
            })
            .collect();
        format!(r#"<div class="gl-picks">{items}</div>"#)
    };
    let bans =
        |l: &[String]| -> String { l.iter().map(|b| portrait(b, a, 22, Tone::Ban)).collect() };
    let items: String = f
        .games
        .iter()
        .map(|g| {
            let first = match g.first_pick {
                Some(true) => "they had first pick",
                Some(false) => "opponent had first pick",
                None => "",
            };
            format!(
                r#"<div class="gl {cls}">
      <div class="gl-meta"><strong>{res}</strong><span>{map}</span><em>{date} · {len} · {first}</em></div>
      <div class="gl-side"><label>Them</label>{mine}<div class="gl-bans"><label>bans</label>{b1}</div></div>
      <div class="gl-side"><label>Opponent</label>{theirs}<div class="gl-bans"><label>bans</label>{b2}</div></div>
    </div>"#,
                cls = if g.won { "gl-w" } else { "gl-l" },
                res = if g.won { "WIN" } else { "LOSS" },
                map = esc(&g.map),
                date = day(g.date.as_deref()),
                len = fmt_len(Some(g.length_s)),
                mine = row(&g.picks, true),
                b1 = bans(&g.bans),
                theirs = row(&g.opp_picks, false),
                b2 = bans(&g.bans_against),
            )
        })
        .collect();
    format!(
        r#"<section class="sec reveal" id="games">
    <div class="sec-hd"><span class="num">07</span><h2>Game log</h2></div>
    <div class="glog">{items}</div>
  </section>"#
    )
}

// ── document ────────────────────────────────────────────────────────────────

/// The whole self-contained HTML file.
pub fn render_report_html(r: &ScoutingReport, a: &ExportAssets, generated_at: &str) -> String {
    let f = r.snapshot.as_ref().map(|s| &s.facts);
    let names = pid_names(f);
    let plans: &[MapPlan] = r.analysis.as_ref().map_or(&[], |x| x.maps.as_slice());
    let rows = maps_with_plans(f, plans);
    let nav: String = rows
        .iter()
        .map(|x| format!(r##"<a href="#{}">{}</a>"##, map_anchor(&x.map), esc(&x.map)))
        .collect();
    let body = match f {
        Some(f) if f.overview.games > 0 => {
            let boards: String = rows
                .iter()
                .enumerate()
                .map(|(i, x)| map_board(&x.map, x.facts.as_ref(), x.plan.as_ref(), f, a, &names, i))
                .collect();
            format!(
                r#"{}
       {}
       <section class="sec" id="maps">
         <div class="sec-hd reveal"><span class="num">03</span><h2>Map by map</h2><small>bans · picks · what they will play</small></div>
         {boards}
       </section>
       {}
       {}
       {}
       {}"#,
                identity(r, f, a, &names),
                map_choice(r, a, &names),
                general(r, a, &names),
                players(f, a),
                faced(f, a),
                games_log(f, a, &names),
            )
        }
        _ => r#"<section class="sec"><p class="muted">This report has no analysed game yet.</p></section>"#.to_string(),
    };
    let n = f.map_or(0, |f| f.overview.games);
    let plan_note = if r.analysis.is_some() {
        format!(
            " · draft plan {}{}",
            match r.analysis_model.as_deref().filter(|m| !m.is_empty()) {
                Some(m) => format!("by {}", esc(m)),
                None => "imported".to_string(),
            },
            match r.analysis_imported_at.as_deref().filter(|m| !m.is_empty()) {
                Some(d) => format!(" on {}", day(Some(d))),
                None => String::new(),
            }
        )
    } else {
        String::new()
    };
    format!(
        r#"<!doctype html>
<html lang="en"><head>
<meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1">
<title>{target} — {title}</title>
<link rel="preconnect" href="https://fonts.googleapis.com"><link rel="preconnect" href="https://fonts.gstatic.com" crossorigin>
<link href="https://fonts.googleapis.com/css2?family=Big+Shoulders+Display:wght@600;800;900&family=Figtree:wght@400;500;600&family=JetBrains+Mono:wght@400;600&display=swap" rel="stylesheet">
<style>
{css}</style>
<style>{assets}</style>
</head><body>
<nav class="top"><span class="brand">STORM CODEX <i>·</i> SCOUTING</span><div class="top-maps">{nav}</div></nav>
{cover}
<main>{body}</main>
<footer>Numbers computed by Storm Codex from {n} replay{s} · report #{id} · facts v{fv}{plan_note}.<br>Recommendations marked ⚠ cite no supporting fact. Dashed facts rest on fewer than 3 games.</footer>
</body></html>"#,
        target = esc(r.target_name.as_deref().unwrap_or("Scouting")),
        title = esc(&r.title),
        css = CSS,
        assets = asset_css(a),
        cover = cover(r, f, a, generated_at),
        s = if n == 1 { "" } else { "s" },
        id = r.id,
        fv = r.facts_version,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scouting::*;

    fn report() -> ScoutingReport {
        let json = r#"{
          "id": 7, "title": "Wk 4 — Les Ours", "target_name": "Bears <script>", "anchors": [], "facts_version": 3,
          "analysis_model": "gpt", "analysis_imported_at": "2026-11-02T10:00:00Z",
          "snapshot": {"facts": {
            "overview": {"games": 2, "record": {"k": 1, "n": 2}, "excluded": 0, "first_date": "2026-10-01T10:00:00Z", "last_date": "2026-10-09T10:00:00Z", "builds": [95000], "avg_length_s": 754.0},
            "players": [{"pid":"p1","toon":"t","name":"Solo","core":true,"record":{"k":1,"n":2},
               "heroes":[{"id":"h","hero":"Sgt. Hammer","picks":{"k":2,"n":2},"record":{"k":1,"n":2},"by":[]}],
               "roles":[{"id":"r","key":"Tank","count":{"k":2,"n":2}}],
               "stats":{"kills":1.25,"deaths":2.0,"assists":9.5,"kill_participation_pct":0,"hero_damage_pm":1234.4,"healing_pm":0,"time_dead_pct":12.34}}],
            "maps": [{"map":"Braxis Holdout","record":{"k":1,"n":2},"wilson":[12.4,87.5],"picks":[{"id":"a","hero":"Johanna","picks":{"k":1,"n":1},"record":{"k":1,"n":1},"by":["p1","p1"]}],"bans":[{"id":"b","key":"Muradin","count":{"k":1,"n":2}}],"bans_against":[]}],
            "draft": {"first_pick":{"k":1,"n":2},"bans_first":[{"id":"x","key":"Muradin","count":{"k":2,"n":2}}],"bans_mid":[],"bans_against":[],"openers":[],"opener_roles":[],"last_picks":[],"faced":[{"id":"f","hero":"Zarya","picks":{"k":2,"n":2},"record":{"k":0,"n":2},"by":[]}]},
            "flow": {"first_to_10":{"k":0,"n":0},"first_to_10_record":{"k":0,"n":0},"first_fort":{"k":0,"n":0},"first_fort_record":{"k":0,"n":0},"first_objective":{"k":0,"n":0},"first_objective_record":{"k":0,"n":0},"level_diff":[],"length":[],"comebacks":{"k":0,"n":0},"throws":{"k":0,"n":0}},
            "games": [{"gid":1,"date":"2026-10-01T10:00:00Z","map":"Braxis Holdout","won":true,"length_s":754.0,"first_pick":true,"picks":[{"hero":"Johanna","player":"p1"}],"opp_picks":[{"hero":"Zarya","player":"Foe"}],"bans":["Muradin"],"bans_against":[]}]
          }, "detection": {"candidates": [], "ambiguous": false}, "roster": [], "roster_auto": false},
          "analysis": {"summary": "Solo (p1) loves tanks", "map_choice": {"pick": [], "avoid": []},
            "maps": [{"map":"Braxis Holdout","confidence":"high","overview":"o","bans":[{"hero":"Muradin","phase":"mid","player":null,"why":"p1 stacks it","evidence":[{"id":"e","known":true,"label":"Braxis Holdout — bans (3)","text":"p1 2/2","low":true}],"unsupported":false}],"picks":[],"their_picks":[{"hero":"Zarya","phase":null,"player":"p1","why":"w","evidence":[],"unsupported":true}],"considerations":[],"evidence":[],"unsupported":false}],
            "general": {"bans": [], "picks": [], "considerations": []}},
          "created_at":"","updated_at":"","status":"ready","games":[]
        }"#;
        serde_json::from_str(json).unwrap()
    }

    #[test]
    fn escapes_the_five_html_characters() {
        assert_eq!(
            esc(r#"<a href="x">&'"#),
            "&lt;a href=&quot;x&quot;&gt;&amp;&#39;"
        );
    }

    #[test]
    fn file_name_is_a_plain_slug() {
        assert_eq!(
            export_file_name("Wk 4 — Les Ours"),
            "scouting-wk-4-les-ours.html"
        );
        assert_eq!(export_file_name("Équipe Été"), "scouting-equipe-ete.html");
        assert_eq!(export_file_name("???"), "scouting-report.html");
    }

    #[test]
    fn initials_take_two_letters() {
        assert_eq!(initials("Sgt. Hammer"), "SH");
        assert_eq!(initials("Johanna"), "JO");
        assert_eq!(initials("..."), "?");
    }

    #[test]
    fn lists_heroes_and_maps_sorted() {
        let r = report();
        assert_eq!(
            heroes_of(&r),
            ["Johanna", "Muradin", "Sgt. Hammer", "Zarya"]
        );
        assert_eq!(maps_of(&r), ["Braxis Holdout"]);
    }

    #[test]
    fn renders_a_self_contained_escaped_document() {
        let mut a = ExportAssets::default();
        a.hero
            .insert("Johanna".into(), "data:image/webp;base64,AAAA".into());
        a.hero.insert("Zarya".into(), "javascript:alert(1)".into());
        a.map.insert(
            "Braxis Holdout".into(),
            "data:image/webp;base64,BBBB".into(),
        );
        let html = render_report_html(&report(), &a, "11 Nov 2026, 10:00");
        assert!(html.starts_with("<!doctype html>"));
        assert!(html.contains("<title>Bears &lt;script&gt; — Wk 4 — Les Ours</title>"));
        assert!(!html.contains("<script>"));
        // image embedded once, as a class
        assert!(html.contains(".h-johanna{background-image:url('data:image/webp;base64,AAAA')}"));
        assert!(html.contains(".m-braxis-holdout{--img:url('data:image/webp;base64,BBBB')}"));
        assert!(!html.contains("javascript:"));
        // pN → names, parenthesised ids dropped
        assert!(html.contains("Solo loves tanks"));
        assert!(html.contains("Braxis Holdout"));
        assert!(html.contains("<span class=\"low\">bans <b>Solo 2/2</b></span>"));
        assert!(html.contains("⚠ unverified — no supporting fact"));
        assert!(html.contains("MID BAN"));
        assert!(html.contains("high confidence"));
        assert!(html.contains("95% CI 12–88%"));
        assert!(
            html.contains("K/D/A <b>1.3/2.0/9.5</b>") || html.contains("K/D/A <b>1.2/2.0/9.5</b>")
        );
        assert!(html.contains("Generated 11 Nov 2026, 10:00 · facts v3 · plan by gpt"));
        assert!(html.contains("draft plan by gpt on 2026-11-02"));
        assert!(html.contains("2 replays"));
    }

    #[test]
    fn a_report_without_games_says_so() {
        let mut r = report();
        r.snapshot = None;
        r.analysis = None;
        let html = render_report_html(&r, &ExportAssets::default(), "x");
        assert!(html.contains("This report has no analysed game yet."));
        assert!(html.contains("0 replays"));
    }
}
