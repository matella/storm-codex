//! "Replay 2D" tab of a match: loads the viewer model (positions normalised to [0, 1]) once, then scrubs entirely on the
//! client (`sample_at(t)` is pure). Port of `web/src/components/Replay2D.tsx` and `TalentStrip2D.tsx`.
//! The `window.__cal` dev hook of the React viewer is not ported (calibration is done with the Calibrate map mode).

use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    rc::Rc,
    sync::{
        atomic::{AtomicU32, Ordering},
        Arc,
    },
};

use leptos::{
    ev, html,
    prelude::*,
    task::spawn_local,
    wasm_bindgen::{closure::Closure, JsCast},
    web_sys,
};
use serde_json::Value;
use storm_web_core::{
    net,
    replay2d::{
        advance, baked_anchor, cast_flash, clip_frames, deaths_near, feed_aria_label, feed_label,
        feed_rows, fmt_clock, highlight_idx, minions_near, objective_aria_label, objective_label,
        sample_at, solve_similarity, Anchor, FeedRow, HeroTrack, PlayerMeta, Replay2D as Model,
    },
    session::map_slug,
};

use crate::{
    clip,
    ui::{fetch, use_dim, use_talents, Avatar},
};

const CANVAS_SIZE: f64 = 640.0;
const HERO_R: f64 = 10.0;
const SPEEDS: [f64; 5] = [0.5, 1.0, 2.0, 4.0, 8.0];
/// A talent pick stays "recent" (badge on the portrait) this long after `t`.
const TALENT_MARKER_WINDOW_SEC: f64 = 3.0;
const TIER_LEVELS: [i64; 7] = [1, 4, 7, 10, 13, 16, 20];

/// Background of the viewer: the in-game minimap, else the painted art, drawn inside the canvas to share the transform.
struct Bg {
    img: web_sys::HtmlImageElement,
    slug: String,
    minimap: bool,
}

thread_local! {
    static ICONS: RefCell<HashMap<String, web_sys::HtmlImageElement>> = RefCell::default();
}

/// Cached portraits (no `Image()` per scrub frame); `on_load` redraws when one arrives.
fn load_icon(url: &str, on_load: impl Fn() + 'static) -> Option<web_sys::HtmlImageElement> {
    ICONS.with(|c| {
        if let Some(i) = c.borrow().get(url) {
            return Some(i.clone());
        }
        let img = web_sys::HtmlImageElement::new().ok()?;
        let cb = Closure::<dyn FnMut()>::new(on_load);
        img.set_onload(Some(cb.as_ref().unchecked_ref()));
        cb.forget();
        img.set_src(url);
        c.borrow_mut().insert(url.to_string(), img.clone());
        Some(img)
    })
}

/// A CSS `var(--x)` as a computed colour (the canvas does not understand custom properties); literal colours pass through.
fn resolve_color(v: &str) -> String {
    let Some(name) = v.strip_prefix("var(").and_then(|r| r.strip_suffix(')')) else {
        return v.to_string();
    };
    let value = web_sys::window()
        .and_then(|w| {
            let root = w.document()?.document_element()?;
            let style = w.get_computed_style(&root).ok()??;
            style.get_property_value(name.trim()).ok()
        })
        .map(|s| s.trim().to_string())
        .unwrap_or_default();
    if value.is_empty() {
        "#afa9ec".to_string()
    } else {
        value
    }
}

type RafClosure = Closure<dyn FnMut()>;

fn raf_loop(mut tick: impl FnMut(f64) -> bool + 'static) {
    let Some(win) = web_sys::window() else { return };
    let cb: Rc<RefCell<Option<RafClosure>>> = Rc::default();
    let cb2 = cb.clone();
    let win2 = win.clone();
    let last = Rc::new(Cell::new(net::now_ms() as f64));
    *cb.borrow_mut() = Some(Closure::new(move || {
        let now = net::now_ms() as f64;
        let dt = (now - last.get()) / 1000.0;
        last.set(now);
        if tick(dt) {
            if let Some(c) = cb2.borrow().as_ref() {
                let _ = win2.request_animation_frame(c.as_ref().unchecked_ref());
            }
        }
    }));
    let first = cb.borrow();
    if let Some(c) = first.as_ref() {
        let _ = win.request_animation_frame(c.as_ref().unchecked_ref());
    }
    drop(first);
}

fn cross(ctx: &web_sys::CanvasRenderingContext2d, cx: f64, cy: f64, s: f64) {
    ctx.move_to(cx - s, cy - s);
    ctx.line_to(cx + s, cy + s);
    ctx.move_to(cx + s, cy - s);
    ctx.line_to(cx - s, cy + s);
}

fn circle(ctx: &web_sys::CanvasRenderingContext2d, cx: f64, cy: f64, r: f64) {
    ctx.begin_path();
    let _ = ctx.arc(cx, cy, r, 0.0, std::f64::consts::TAU);
}

struct Scene<'a> {
    data: &'a Model,
    t: f64,
    bg: Option<&'a Bg>,
    anchor: Option<Anchor>,
    show_minions: bool,
    team_color: &'a [String; 2],
    players: &'a HashMap<i64, PlayerMeta>,
    icon: &'a dyn Fn(&str) -> Option<String>,
    ring: &'a dyn Fn(Option<&str>) -> String,
}

fn draw(ctx: &web_sys::CanvasRenderingContext2d, w: f64, h: f64, s: &Scene) {
    ctx.clear_rect(0.0, 0.0, w, h);
    let t = s.t;
    // Projection game → canvas. The map is drawn straight; the overlays are projected through a similarity fitted on
    // the two cores (the game's coordinate frame is rotated/stretched relative to the minimap). Default: identity + flip Y.
    let mut project: Box<dyn Fn(f64, f64) -> (f64, f64)> =
        Box::new(move |gx, gy| (gx * w, (1.0 - gy) * h));
    if let Some(bg) = s.bg {
        let core = |team| {
            s.data
                .structures
                .iter()
                .find(|st| st.kind == "core" && st.team == team)
        };
        if let (Some(cal), Some(blue), Some(red)) = (&s.anchor, core(0), core(1)) {
            // game (y up) → image (y down) BEFORE the similarity, then `vscale` stretches around the centre
            let vs = cal.vscale.unwrap_or(1.0);
            let vy = move |gy: f64| 0.5 + vs * (1.0 - gy - 0.5);
            let [a, b, c, d, e, f] = solve_similarity(
                (blue.x, vy(blue.y)),
                (red.x, vy(red.y)),
                (cal.blue[0], cal.blue[1]),
                (cal.red[0], cal.red[1]),
            );
            project = Box::new(move |gx, gy| {
                ((a * gx + c * vy(gy) + e) * w, (b * gx + d * vy(gy) + f) * h)
            });
        }
        let _ = ctx.draw_image_with_html_image_element_and_dw_and_dh(&bg.img, 0.0, 0.0, w, h);
        ctx.set_fill_style_str("rgba(12,14,22,0.30)");
        ctx.fill_rect(0.0, 0.0, w, h);
    }

    // minions and camps go below structures and heroes: discreet dots, ±5 s window
    if s.show_minions {
        let neutral = resolve_color("var(--muted-2)");
        ctx.set_global_alpha(0.55);
        for m in minions_near(&s.data.minions, t) {
            let (cx, cy) = project(m.x, m.y);
            circle(ctx, cx, cy, 3.5);
            let colour = match m.team {
                1 => &s.team_color[1],
                0 => &s.team_color[0],
                _ => &neutral,
            };
            ctx.set_fill_style_str(colour);
            ctx.fill();
        }
        ctx.set_global_alpha(1.0);
    }

    // structures first (under the heroes): team squares, the core larger; destroyed → grey with a cross
    for st in &s.data.structures {
        if st.kind == "other" {
            continue;
        }
        let (cx, cy) = project(st.x, st.y);
        let destroyed = st.destroyed_at.is_some_and(|d| t >= d);
        let size = if st.kind == "core" { 9.0 } else { 5.0 };
        ctx.set_fill_style_str(if destroyed {
            "#5a5d6b"
        } else {
            &s.team_color[usize::from(st.team == 1)]
        });
        ctx.fill_rect(cx - size / 2.0, cy - size / 2.0, size, size);
        if destroyed {
            ctx.set_stroke_style_str("#e8eaf2");
            ctx.set_line_width(1.0);
            ctx.begin_path();
            cross(ctx, cx, cy, size / 2.0 + 1.0);
            ctx.stroke();
        }
    }

    for track in &s.data.heroes {
        let Some((px, py, alive)) = sample_at(track, t) else {
            continue;
        };
        let meta = s.players.get(&track.player_id);
        let team = usize::from(meta.and_then(|m| m.team) == Some(1));
        let (cx, cy) = project(px, py);
        ctx.set_global_alpha(if alive { 1.0 } else { 0.4 });

        // team ring (outside: the team identity stays visible even with a portrait)
        circle(ctx, cx, cy, HERO_R + 3.0);
        ctx.set_line_width(2.0);
        ctx.set_stroke_style_str(&s.team_color[team]);
        ctx.stroke();

        // fill: portrait clipped to a circle when loaded, otherwise a team dot with initials
        let hero = meta.and_then(|m| m.hero.as_deref());
        let mut drew = false;
        if let Some(url) = hero.and_then(|h| (s.icon)(h)) {
            let ready = load_icon(&url, || {}).filter(|i| i.complete() && i.natural_width() > 0);
            if let Some(img) = ready {
                ctx.save();
                circle(ctx, cx, cy, HERO_R);
                ctx.clip();
                let _ = ctx.draw_image_with_html_image_element_and_dw_and_dh(
                    &img,
                    cx - HERO_R,
                    cy - HERO_R,
                    HERO_R * 2.0,
                    HERO_R * 2.0,
                );
                ctx.restore();
                drew = true;
            }
        }
        if !drew {
            circle(ctx, cx, cy, HERO_R);
            ctx.set_fill_style_str(&s.team_color[team]);
            ctx.fill();
            if let Some(h) = hero {
                ctx.set_fill_style_str("#0b0c11");
                ctx.set_font("bold 8px sans-serif");
                ctx.set_text_align("center");
                ctx.set_text_baseline("middle");
                let _ = ctx.fill_text(&crate::ui::initials(h), cx, cy);
            }
        }

        // hero universe ring (on top, like `<Avatar>`)
        circle(ctx, cx, cy, HERO_R);
        ctx.set_line_width(1.5);
        ctx.set_stroke_style_str(&(s.ring)(hero));
        ctx.stroke();

        // ability cast flash: a white ring that pulses and fades, about 0.6 s after each cast
        let flash = cast_flash(&track.casts, t);
        if flash > 0.0 {
            circle(ctx, cx, cy, HERO_R + 5.0);
            ctx.set_line_width(2.0);
            ctx.set_stroke_style_str(&format!("rgba(255, 255, 255, {flash})"));
            ctx.stroke();
        }
        if !alive {
            ctx.set_stroke_style_str("#e8eaf2");
            ctx.set_line_width(1.5);
            ctx.begin_path();
            cross(ctx, cx, cy, 5.0);
            ctx.stroke();
        }

        // recent talent pick badge: only AFTER the pick (0 ≤ dt ≤ 3 s)
        if track
            .talents
            .iter()
            .any(|tp| (0.0..=TALENT_MARKER_WINDOW_SEC).contains(&(t - tp.t)))
        {
            let (bx, by) = (cx + HERO_R + 2.0, cy - HERO_R - 2.0);
            circle(ctx, bx, by, 6.0);
            ctx.set_fill_style_str("#f5c542");
            ctx.fill();
            ctx.set_stroke_style_str("#1a1500");
            ctx.set_line_width(1.0);
            ctx.stroke();
            ctx.set_fill_style_str("#1a1500");
            ctx.set_font("bold 8px sans-serif");
            ctx.set_text_align("center");
            ctx.set_text_baseline("middle");
            let _ = ctx.fill_text("↑", bx, by + 0.5);
        }
    }
    ctx.set_global_alpha(1.0);

    for d in deaths_near(&s.data.deaths, t) {
        let (cx, cy) = project(d.x, d.y);
        ctx.set_stroke_style_str(&s.team_color[1]);
        ctx.set_line_width(2.0);
        ctx.begin_path();
        cross(ctx, cx, cy, 6.0);
        ctx.stroke();
    }
}

/// Talent progression strip (10 hero rows × 7 tiers) under the scrubber. A cell is filled once the pick of its tier is at
/// or before `t`. The talent name is best-effort (falls back to "Tier N").
#[component]
fn TalentStrip2D(
    data: Arc<Model>,
    t: RwSignal<f64>,
    talent_name_for: Callback<(Option<String>, i64), Option<String>>,
) -> impl IntoView {
    if data.heroes.is_empty() {
        return None;
    }
    let by_player: HashMap<i64, PlayerMeta> = data
        .players
        .iter()
        .map(|p| (p.player_id, p.clone()))
        .collect();
    let mut rows: Vec<(HeroTrack, Option<PlayerMeta>)> = data
        .heroes
        .iter()
        .map(|h| (h.clone(), by_player.get(&h.player_id).cloned()))
        .collect();
    rows.sort_by_key(|(_, p)| u8::from(p.as_ref().and_then(|p| p.team) == Some(1)));
    let levels = data.levels.clone();
    let team_level = move |team: i64| {
        let now = t.get();
        levels
            .iter()
            .filter(|l| l.team == team && l.t <= now)
            .map(|l| l.level)
            .max()
            .unwrap_or(0)
    };
    let team_level2 = team_level.clone();
    Some(view! {
        <div style="margin-top: 10px">
            <div style="display: flex; justify-content: space-between; align-items: baseline; margin-bottom: 4px">
                <p class="cap" style="margin: 0">"Talents"</p>
                <span style="font-size: 11px">
                    <span class="tm-blue">{move || format!("Blue lvl {}", team_level(0))}</span>
                    <span class="muted">" · "</span>
                    <span class="tm-red">{move || format!("Red lvl {}", team_level2(1))}</span>
                </span>
            </div>
            <div style="display: grid; grid-template-columns: 110px repeat(7, 1fr); gap: 3px; font-size: 10px; align-items: center">
                {rows.into_iter().map(|(h, p)| {
                    let team = usize::from(p.as_ref().and_then(|p| p.team) == Some(1));
                    let team_class = if team == 0 { "tm-blue" } else { "tm-red" };
                    let team_var = if team == 0 { "var(--tm-blue)" } else { "var(--tm-red)" };
                    let hero = p.as_ref().and_then(|p| p.hero.clone());
                    let name = p.as_ref().and_then(|p| p.name.clone());
                    let hero_label = hero.clone().unwrap_or_else(|| "—".to_string());
                    view! {
                        <div style="display: contents">
                            <div style="display: flex; align-items: center; gap: 4px; overflow: hidden; min-width: 0">
                                <Avatar hero=hero.clone() size=14 />
                                <span class=team_class style="white-space: nowrap; overflow: hidden; text-overflow: ellipsis">{hero_label}</span>
                            </div>
                            {TIER_LEVELS.iter().enumerate().map(|(i, lvl)| {
                                let tier = i as i64 + 1;
                                let (h2, hero2, name2) = (h.clone(), hero.clone(), name.clone());
                                let taken = Arc::new(move || h2.talents.iter().any(|tp| tp.tier == tier && tp.t <= t.get()));
                                let (taken_l, taken_b, taken_o) = (taken.clone(), taken.clone(), taken);
                                let lvl = *lvl;
                                let label = move || if taken_l() {
                                    talent_name_for.run((name2.clone(), tier)).unwrap_or_else(|| format!("Tier {tier}"))
                                } else {
                                    format!("Tier {tier} not taken yet (level {lvl})")
                                };
                                let label2 = label.clone();
                                let hero_txt = hero2.unwrap_or_else(|| "hero".to_string());
                                view! {
                                    <div role="img" aria-label=move || format!("{hero_txt}, tier {tier}: {}", label()) title=label2
                                        style=move || format!("height: 14px; border-radius: 3px; background: {}; opacity: {}", if taken_b() { team_var } else { "var(--hairline-strong)" }, if taken_o() { "1" } else { "0.35" })></div>
                                }
                            }).collect_view()}
                        </div>
                    }
                }).collect_view()}
            </div>
        </div>
    })
}

#[component]
pub fn Replay2D(#[prop(into)] id: String) -> impl IntoView {
    let model = {
        let url = format!("/api/matches/{id}/replay2d");
        fetch::<Model>(move || url.clone())
    };
    let match_data = {
        let url = format!("/api/matches/{id}");
        fetch::<Value>(move || url.clone())
    };
    let settings = fetch::<Value>(|| "/api/settings".to_string());
    let talents = use_talents();
    let dim = use_dim();

    move || {
        match model.get() {
        None => view! { <div class="empty">"loading…"</div> }.into_any(),
        Some(Err(_)) => view! { <div class="empty">"replay unavailable"</div> }.into_any(),
        Some(Ok(data)) => {
            view! { <Viewer id=id.clone() data=Arc::new(data) match_data=match_data settings=settings talents=talents dim=dim /> }.into_any()
        }
    }
    }
}

type MatchRes = LocalResource<Result<Value, String>>;

#[component]
fn Viewer(
    id: String,
    data: Arc<Model>,
    match_data: MatchRes,
    settings: MatchRes,
    talents: RwSignal<HashMap<String, storm_web_core::herodetail::DimTalent>>,
    dim: RwSignal<storm_web_core::heroes::Dim>,
) -> impl IntoView {
    let duration = data.meta.duration_sec;
    let map = data.meta.map_name.clone();
    let slug = map_slug(&map);

    // saved calibrations (settings) override the baked defaults
    let saved_anchor = {
        let slug = slug.clone();
        move || -> Option<Anchor> {
            settings
                .get()
                .and_then(Result::ok)
                .and_then(|s| s.get("minimap_anchors").and_then(|a| a.get(&slug)).cloned())
                .and_then(|v| serde_json::from_value(v).ok())
        }
    };
    let calibrating = RwSignal::new(false);
    let edit_anchors = RwSignal::new(None::<Anchor>);
    let dragging = RwSignal::new(None::<&'static str>);
    let cal_saved = RwSignal::new(String::new());
    let cal_ref = NodeRef::<html::Div>::new();

    let enter_calibration = {
        let (slug, saved) = (slug.clone(), saved_anchor.clone());
        move |_| {
            let cur = saved().or_else(|| baked_anchor(&slug));
            edit_anchors.set(Some(match cur {
                Some(c) => Anchor {
                    blue: c.blue,
                    red: c.red,
                    vscale: Some(c.vscale.unwrap_or(1.0)),
                },
                None => Anchor {
                    blue: [0.1, 0.5],
                    red: [0.9, 0.5],
                    vscale: Some(1.0),
                },
            }));
            calibrating.set(true);
            cal_saved.set(String::new());
        }
    };
    let on_cal_move = move |e: ev::PointerEvent| {
        let (Some(color), Some(el)) = (dragging.get_untracked(), cal_ref.get_untracked()) else {
            return;
        };
        if !calibrating.get_untracked() {
            return;
        }
        let rect = el.get_bounding_client_rect();
        let r3 = |n: f64| (n.clamp(0.0, 1.0) * 1000.0).round() / 1000.0;
        let pos = [
            r3((f64::from(e.client_x()) - rect.left()) / rect.width()),
            r3((f64::from(e.client_y()) - rect.top()) / rect.height()),
        ];
        edit_anchors.update(|a| {
            if let Some(a) = a {
                if color == "blue" {
                    a.blue = pos
                } else {
                    a.red = pos
                }
            }
        });
    };
    let save_calibration = {
        let slug = slug.clone();
        move |_| {
            let Some(edit) = edit_anchors.get_untracked() else {
                return;
            };
            let mut all: serde_json::Map<String, Value> = settings
                .get_untracked()
                .and_then(Result::ok)
                .and_then(|s| s.get("minimap_anchors").and_then(Value::as_object).cloned())
                .unwrap_or_default();
            all.insert(
                slug.clone(),
                serde_json::to_value(&edit).unwrap_or(Value::Null),
            );
            let body = Value::Object(all).to_string();
            spawn_local(async move {
                match net::write("PUT", "/api/admin/minimap-anchors", Some(body), None).await {
                    Ok((s, _)) if (200..300).contains(&s) => {
                        settings.refetch();
                        calibrating.set(false);
                        dragging.set(None);
                        cal_saved.set("saved ✓".to_string());
                    }
                    _ => cal_saved.set("save failed (admin token?)".to_string()),
                }
            });
        }
    };

    // talent names are keyed by PLAYER NAME (unique per match), not by hero: an ARAM mirror has the same hero twice
    let talent_name_for = {
        Callback::new(
            move |(name, tier): (Option<String>, i64)| -> Option<String> {
                let name = name?;
                let tree = match_data.get().and_then(Result::ok).and_then(|m| {
                    m.get("players")?
                        .as_object()?
                        .values()
                        .find(|p| {
                            p.get("name").and_then(Value::as_str) == Some(name.as_str())
                                && p.get("talents").is_some()
                        })?
                        .get("talents")?
                        .get(format!("Tier{tier}Choice"))?
                        .as_str()
                        .map(str::to_string)
                })?;
                talents.with(|m| m.get(&tree).map(|t| t.name.clone()))
            },
        )
    };

    let t = RwSignal::new(0.0_f64);
    let show_minions = RwSignal::new(false);
    let canvas_ref = NodeRef::<html::Canvas>::new();
    let redraw = RwSignal::new(0u32); // bumped when a portrait or the background finishes loading
    let playing = RwSignal::new(false);
    let speed = RwSignal::new(1.0_f64);
    let clip_start = RwSignal::new(None::<f64>);
    let clip_end = RwSignal::new(None::<f64>);
    let recording = RwSignal::new(false);

    let team_color: [String; 2] = [
        resolve_color("var(--tm-blue)"),
        resolve_color("var(--tm-red)"),
    ];
    let players: HashMap<i64, PlayerMeta> = data
        .players
        .iter()
        .map(|p| (p.player_id, p.clone()))
        .collect();
    let players = Arc::new(players);

    // background: in-game minimap first, painted art second, the container's gradient last
    let bg_aspect = RwSignal::new(1.0_f64);
    let bg: StoredValue<Option<Bg>, LocalStorage> = StoredValue::new_local(None);
    {
        let (map, slug) = (map.clone(), slug.clone());
        let img = web_sys::HtmlImageElement::new().ok();
        if let Some(img) = img {
            let stage = Rc::new(Cell::new(0u8));
            let urls = [
                format!("/images/minimaps/{slug}.jpg"),
                format!("/images/battlegrounds/{slug}.png"),
            ];
            let (img2, stage2) = (img.clone(), stage.clone());
            let slug2 = slug.clone();
            let on_load = Closure::<dyn FnMut()>::new(move || {
                bg.set_value(Some(Bg {
                    img: img2.clone(),
                    slug: slug2.clone(),
                    minimap: stage2.get() == 0,
                }));
                if img2.natural_width() > 0 && img2.natural_height() > 0 {
                    bg_aspect
                        .set(f64::from(img2.natural_width()) / f64::from(img2.natural_height()));
                }
                redraw.update(|n| *n += 1);
            });
            let (img3, stage3, urls3) = (img.clone(), stage.clone(), urls.clone());
            let on_error = Closure::<dyn FnMut()>::new(move || {
                if stage3.get() == 0 {
                    stage3.set(1);
                    img3.set_src(&urls3[1]);
                } else {
                    bg.set_value(None);
                    redraw.update(|n| *n += 1);
                }
            });
            img.set_onload(Some(on_load.as_ref().unchecked_ref()));
            img.set_onerror(Some(on_error.as_ref().unchecked_ref()));
            on_load.forget();
            on_error.forget();
            let _ = map;
            img.set_src(&urls[0]);
        }
    }

    let rows = Arc::new(feed_rows(&data));
    let hl = {
        let rows = rows.clone();
        Memo::new(move |_| highlight_idx(&rows, t.get()))
    };
    let feed_ref = NodeRef::<html::Div>::new();
    {
        Effect::new(move |_| {
            let idx = hl.get();
            let (Some(container), Some(idx)) = (feed_ref.get(), idx) else {
                return;
            };
            let Some(row) = container
                .children()
                .item(idx as u32)
                .and_then(|r| r.dyn_into::<web_sys::HtmlElement>().ok())
            else {
                return;
            };
            // scroll confined to the list container (never scrollIntoView, which would move the page during playback)
            let (top, bottom) = (row.offset_top(), row.offset_top() + row.offset_height());
            if top < container.scroll_top() {
                container.set_scroll_top(top);
            } else if bottom > container.scroll_top() + container.client_height() {
                container.set_scroll_top(bottom - container.client_height());
            }
        });
    }
    let hero_for = {
        let players = players.clone();
        move |pid: Option<i64>| pid.and_then(|p| players.get(&p).and_then(|m| m.hero.clone()))
    };
    let row_color = {
        let (players, tc) = (players.clone(), team_color.clone());
        move |row: &FeedRow| -> String {
            let by_team = |team: Option<i64>| match team {
                Some(1) => tc[1].clone(),
                Some(0) => tc[0].clone(),
                _ => "var(--muted-2)".to_string(),
            };
            match row {
                FeedRow::Objective(o) => by_team(o.team),
                FeedRow::Event(e) if e.kind == "takedown" => by_team(
                    players
                        .get(&e.victim_player_id.unwrap_or(-1))
                        .and_then(|p| p.team),
                ),
                FeedRow::Event(e) if e.kind == "structure" => by_team(e.team),
                FeedRow::Event(_) => "var(--muted-2)".to_string(),
            }
        }
    };

    // playback: an rAF loop while `playing`, advancing `t` by real dt × speed
    let play_gen = Arc::new(AtomicU32::new(0));
    {
        let play_gen = play_gen.clone();
        Effect::new(move |_| {
            if !playing.get() {
                return;
            }
            let my = play_gen.load(Ordering::Relaxed).wrapping_add(1);
            play_gen.store(my, Ordering::Relaxed);
            let gen = play_gen.clone();
            raf_loop(move |dt| {
                if gen.load(Ordering::Relaxed) != my || !playing.get_untracked() {
                    return false;
                }
                let (nt, going) = advance(t.get_untracked(), dt, speed.get_untracked(), duration);
                t.set(nt);
                if !going {
                    playing.set(false);
                }
                going
            });
        });
    }
    let export_gen = Arc::new(AtomicU32::new(0));
    let rec_slot: StoredValue<Option<clip::Recording>, LocalStorage> = StoredValue::new_local(None);
    {
        let (play_gen, export_gen) = (play_gen.clone(), export_gen.clone());
        on_cleanup(move || {
            play_gen.fetch_add(1, Ordering::Relaxed);
            export_gen.fetch_add(1, Ordering::Relaxed);
        });
    }
    let can_export = move || {
        clip::supported()
            && matches!((clip_start.get(), clip_end.get()), (Some(a), Some(b)) if a < b)
            && !recording.get()
    };
    let export_clip = {
        let export_gen = export_gen.clone();
        let id = id.clone();
        move |_| {
            let (Some(cs), Some(ce)) = (clip_start.get_untracked(), clip_end.get_untracked())
            else {
                return;
            };
            if recording.get_untracked() || cs >= ce || !clip::supported() {
                return;
            }
            let Some(canvas) = canvas_ref.get_untracked() else {
                return;
            };
            playing.set(false); // stop any playback before taking over `t`
            recording.set(true);
            let Ok(rec) = clip::Recording::start(&canvas, 30.0) else {
                recording.set(false);
                return;
            };
            rec_slot.set_value(Some(rec));
            t.set(cs);
            let my = export_gen.load(Ordering::Relaxed).wrapping_add(1);
            export_gen.store(my, Ordering::Relaxed);
            let gen = export_gen.clone();
            let id = id.clone();
            raf_loop(move |dt| {
                if gen.load(Ordering::Relaxed) != my {
                    return false;
                }
                let next = t.get_untracked() + dt * speed.get_untracked();
                if next >= ce {
                    t.set(ce);
                    let rec = rec_slot.try_update_value(Option::take).flatten();
                    let id = id.clone();
                    spawn_local(async move {
                        if let Some(rec) = rec {
                            if let Ok(blob) = rec.stop().await {
                                clip::download_blob(
                                    &blob,
                                    &format!(
                                        "replay-{id}-{}-{}.webm",
                                        cs.round() as i64,
                                        ce.round() as i64
                                    ),
                                );
                            }
                        }
                        recording.set(false);
                    });
                    return false;
                }
                t.set(next);
                true
            });
        }
    };

    // drawing
    {
        let (data, players, slug) = (data.clone(), players.clone(), slug.clone());
        let team_color = team_color.clone();
        let saved = saved_anchor.clone();
        Effect::new(move |_| {
            let now = t.get();
            let (_, _) = (redraw.get(), show_minions.get());
            let (cal_on, edit) = (calibrating.get(), edit_anchors.get());
            let saved_a = saved();
            dim.track();
            let Some(canvas) = canvas_ref.get() else {
                return;
            };
            let Some(ctx) = canvas
                .get_context("2d")
                .ok()
                .flatten()
                .and_then(|c| c.dyn_into::<web_sys::CanvasRenderingContext2d>().ok())
            else {
                return;
            };
            let icon = |h: &str| dim.with_untracked(|d| d.icon(h).map(str::to_string));
            let ring = |h: Option<&str>| resolve_color(dim.with_untracked(|d| d.ring_var(h)));
            bg.with_value(|bg| {
                let anchor = bg.as_ref().filter(|b| b.minimap).and_then(|b| {
                    if cal_on && edit.is_some() {
                        edit.clone()
                    } else {
                        saved_a.clone().or_else(|| baked_anchor(&b.slug))
                    }
                });
                let _ = &slug;
                draw(
                    &ctx,
                    f64::from(canvas.width()),
                    f64::from(canvas.height()),
                    &Scene {
                        data: &data,
                        t: now,
                        bg: bg.as_ref(),
                        anchor,
                        show_minions: show_minions.get_untracked(),
                        team_color: &team_color,
                        players: &players,
                        icon: &icon,
                        ring: &ring,
                    },
                );
            });
        });
    }
    // a portrait that finishes loading after the first draw: redraw once the icons had time (the cache reports through onload)
    {
        let data = data.clone();
        Effect::new(move |_| {
            dim.track();
            for p in &data.players {
                if let Some(url) = p
                    .hero
                    .as_deref()
                    .and_then(|h| dim.with_untracked(|d| d.icon(h).map(str::to_string)))
                {
                    let _ = load_icon(&url, move || redraw.update(|n| *n += 1));
                }
            }
        });
    }

    let canvas_dims = move || {
        let a = bg_aspect.get();
        if a >= 1.0 {
            (CANVAS_SIZE as u32, (CANVAS_SIZE / a).round() as u32)
        } else {
            ((CANVAS_SIZE * a).round() as u32, CANVAS_SIZE as u32)
        }
    };
    let warnings = data.warnings.clone();
    let player_rows = |team: i64| {
        data.players
            .iter()
            .filter(|p| p.team == Some(team))
            .map(|p| view! {
                <div style="display: flex; align-items: center; gap: 6px; padding: 3px 0">
                    <Avatar hero=p.hero.clone() size=18 />
                    <span style="font-size: 11px">{p.name.clone().unwrap_or_else(|| "—".to_string())}</span>
                    <span class="muted" style="font-size: 10px">{p.hero.clone()}</span>
                </div>
            })
            .collect_view()
    };
    let (blue_rows, red_rows) = (player_rows(0), player_rows(1));
    let feed_buttons = rows
        .iter()
        .enumerate()
        .map(|(i, row)| {
            let aria = match row {
                FeedRow::Objective(o) => objective_aria_label(o),
                FeedRow::Event(e) => feed_aria_label(e, &hero_for),
            };
            let label = match row {
                FeedRow::Objective(o) => objective_label(o),
                FeedRow::Event(e) => feed_label(e, &hero_for),
            };
            let color = row_color(row);
            let (c2, tt) = (color.clone(), row.t());
            let style = move || {
                let on = hl.get() == Some(i);
                format!(
                    "display: flex; justify-content: space-between; align-items: center; gap: 8px; text-align: left; font-family: inherit; font-size: 11px; padding: 4px 6px; border-radius: 6px; border: 1px solid transparent; border-color: {}; background: {}; color: inherit; cursor: pointer",
                    if on { c2.as_str() } else { "transparent" },
                    if on { "var(--hairline-strong)" } else { "transparent" }
                )
            };
            view! {
                <button type="button" aria-label=aria style=style on:click=move |_| { playing.set(false); t.set(tt); }>
                    <span style=format!("color: {color}")>{label}</span>
                    <span class="mono muted" style="font-size: 10px; flex-shrink: 0">{fmt_clock(tt)}</span>
                </button>
            }
        })
        .collect_view();

    view! {
        <div class="card" style="padding: 14px">
            {(!warnings.is_empty()).then(|| view! {
                <div style="margin-bottom: 10px">
                    {warnings.iter().map(|w| view! {
                        <div style="font-size: 11px; color: var(--muted-2); background: var(--hairline-strong); border-radius: 6px; padding: 5px 8px; margin-bottom: 4px">{format!("⚠️ {w}")}</div>
                    }).collect_view()}
                </div>
            })}
            <div style="display: flex; gap: 18px; flex-wrap: wrap">
                <div style="flex: 1 1 360px">
                    <div node_ref=cal_ref on:pointermove=on_cal_move on:pointerup=move |_| dragging.set(None) on:pointerleave=move |_| dragging.set(None)
                        style=move || { let (w, h) = canvas_dims(); format!("position: relative; width: min(100%, 560px); aspect-ratio: {w} / {h}; border-radius: 8px; overflow: hidden; background: linear-gradient(135deg, #1a1d2a, #232636)") }>
                        <canvas node_ref=canvas_ref width=move || canvas_dims().0 height=move || canvas_dims().1 style="position: absolute; inset: 0; width: 100%; height: 100%"></canvas>
                        {move || (calibrating.get() && edit_anchors.get().is_some()).then(|| ["blue", "red"].into_iter().map(|color| {
                            let title = format!("core {color} — glisser sur la base {}", if color == "blue" { "bleue" } else { "rouge" });
                            let pos = move || edit_anchors.get().map(|a| if color == "blue" { a.blue } else { a.red }).unwrap_or([0.0, 0.0]);
                            view! {
                                <div title=title on:pointerdown=move |e: ev::PointerEvent| { e.prevent_default(); dragging.set(Some(color)); }
                                    style=move || format!("position: absolute; left: {}%; top: {}%; transform: translate(-50%, -50%); width: 20px; height: 20px; border-radius: 50%; border: 2px solid #fff; background: {}; box-shadow: 0 0 0 2px rgba(0,0,0,.55); cursor: {}; z-index: 5; touch-action: none",
                                        pos()[0] * 100.0, pos()[1] * 100.0, if color == "blue" { "var(--tm-blue)" } else { "var(--tm-red)" },
                                        if dragging.get() == Some(color) { "grabbing" } else { "grab" })></div>
                            }
                        }).collect_view())}
                    </div>
                    <div style="margin-top: 6px; display: flex; gap: 8px; align-items: center; font-size: 11px; flex-wrap: wrap">
                        {move || if !calibrating.get() {
                            view! { <button type="button" class="pill" aria-label="calibrer la minimap" on:click=enter_calibration.clone()>"Calibrate map"</button> }.into_any()
                        } else {
                            let save = save_calibration.clone();
                            view! {
                                <span class="muted">"Drag the blue/red dots onto the two bases; adjust vertical to fix stretch."</span>
                                <label style="display: flex; align-items: center; gap: 5px">
                                    "vertical"
                                    <input type="range" min="0.5" max="1.6" step="0.02" aria-label="échelle verticale de la carte" style="width: 100px"
                                        prop:value=move || edit_anchors.get().and_then(|a| a.vscale).unwrap_or(1.0).to_string()
                                        on:input=move |e| { let v = event_target_value(&e).parse::<f64>().unwrap_or(1.0); edit_anchors.update(|a| if let Some(a) = a { a.vscale = Some(v) }); } />
                                    <span class="mono">{move || format!("{:.2}", edit_anchors.get().and_then(|a| a.vscale).unwrap_or(1.0))}</span>
                                </label>
                                <button type="button" class="pill on" aria-label="enregistrer la calibration" on:click=save>"Save"</button>
                                <button type="button" class="pill" aria-label="annuler la calibration" on:click=move |_| { calibrating.set(false); dragging.set(None); }>"Cancel"</button>
                            }.into_any()
                        }}
                        {move || { let s = cal_saved.get(); (!s.is_empty()).then(|| view! { <span class="muted">{s}</span> }) }}
                    </div>
                </div>
                <div style="min-width: 180px; flex: 0 0 200px">
                    <p class="cap" style="margin: 0 0 8px">"Players"</p>
                    <div style="margin-bottom: 10px"><span class="tm-blue" style="font-size: 10px">"Blue team"</span>{blue_rows}</div>
                    <div style="margin-bottom: 10px"><span class="tm-red" style="font-size: 10px">"Red team"</span>{red_rows}</div>
                </div>
                <div style="min-width: 200px; flex: 1 1 220px; max-width: 280px">
                    <p class="cap" style="margin: 0 0 8px">"Kill feed"</p>
                    <div node_ref=feed_ref style="max-height: 260px; overflow-y: auto; display: flex; flex-direction: column; gap: 2px">{feed_buttons}</div>
                </div>
            </div>
            <div style="display: flex; align-items: center; gap: 10px; margin-top: 10px">
                <button type="button" class="pill on" aria-label=move || if playing.get() { "pause" } else { "play" } disabled=move || recording.get()
                    on:click=move |_| playing.update(|p| *p = !*p)
                    style="min-width: 28px; text-align: center; font-family: inherit; line-height: inherit">{move || if playing.get() { "⏸" } else { "▶" }}</button>
                <select aria-label="playback speed" disabled=move || recording.get()
                    style="font-size: 11px; background: transparent; color: var(--muted-2); border: 1px solid var(--hairline-strong); border-radius: 12px; padding: 3px 6px"
                    on:change=move |e| speed.set(event_target_value(&e).parse().unwrap_or(1.0))>
                    {SPEEDS.iter().map(|s| { let s = *s; view! { <option value=s.to_string() selected=move || speed.get() == s>{format!("{s}×")}</option> } }).collect_view()}
                </select>
                <input type="range" min="0" max=duration.to_string() step="0.1" aria-label="replay time" style="flex: 1" disabled=move || recording.get()
                    aria-valuetext=move || fmt_clock(t.get())
                    prop:value=move || t.get().to_string()
                    on:input=move |e| { playing.set(false); t.set(event_target_value(&e).parse().unwrap_or(0.0)); } />
                <span class="mono muted" style="font-size: 11px; min-width: 90px; text-align: right">{move || format!("{} / {}", fmt_clock(t.get()), fmt_clock(duration))}</span>
                <label style="display: flex; align-items: center; gap: 5px; font-size: 11px; color: var(--muted-2); cursor: pointer">
                    <input type="checkbox" aria-label="Minions / camps" disabled=move || recording.get() prop:checked=move || show_minions.get()
                        on:change=move |e| show_minions.set(event_target_checked(&e)) />
                    "Minions / camps"
                </label>
            </div>
            <div style="display: flex; align-items: center; gap: 10px; margin-top: 8px; flex-wrap: wrap">
                <span class="cap" style="font-size: 10px">"Clip export"</span>
                <button type="button" class="pill" aria-label="Set clip in point to current time" disabled=move || recording.get() on:click=move |_| clip_start.set(Some(t.get_untracked()))>"Set in"</button>
                <button type="button" class="pill" aria-label="Set clip out point to current time" disabled=move || recording.get() on:click=move |_| clip_end.set(Some(t.get_untracked()))>"Set out"</button>
                <span class="mono muted" style="font-size: 11px">
                    {move || match (clip_start.get(), clip_end.get()) { (Some(a), Some(b)) => format!("{}–{}", fmt_clock(a), fmt_clock(b)), _ => "no range selected".to_string() }}
                </span>
                {move || match (clip_start.get(), clip_end.get()) {
                    (Some(a), Some(b)) if a < b => Some(view! { <span class="muted" style="font-size: 10px">{format!("{} frames @30fps", clip_frames(a, b, 30.0))}</span> }),
                    _ => None,
                }}
                <button type="button" class="pill on" aria-label="Export clip to webm" disabled=move || !can_export() on:click=export_clip
                    title=if clip::supported() { None } else { Some("Clip export isn't supported in this browser (missing MediaRecorder / captureStream)") }>"Export clip"</button>
                {move || recording.get().then(|| view! {
                    <span role="status" aria-label="Recording clip" style="display: flex; align-items: center; gap: 4px; font-size: 11px; color: #e5484d">
                        <span aria-hidden="true">"●"</span>" Recording…"
                    </span>
                })}
            </div>
            <TalentStrip2D data=data.clone() t=t talent_name_for=talent_name_for />
        </div>
    }
}
