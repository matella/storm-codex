//! `ds` pieces for games: result pips, mode and award tags, K/D/A, the game card, the search field.
//! Port of `components/ds/match.tsx`, `Pips` of `ds/index.tsx` and `SearchSelect.tsx`.

use leptos::prelude::*;
use storm_web_core::{
    dashboard::kda,
    net,
    session::{award_label, fmt_dur, mode_badge, MatchPlayer, MatchSummary},
};

use crate::ui::{Link, Portrait, Tone};

/// One pip per game: `w` for a win, `l` for a loss.
#[component]
pub fn Pips(games: Vec<bool>) -> impl IntoView {
    view! {
        <span class="ds-pips">
            {games.into_iter().map(|w| view! { <i class=if w { "w" } else { "l" }></i> }).collect_view()}
        </span>
    }
}

/// Mode label (Storm League, ARAM...) coloured by family; `short` shows the code instead.
#[component]
pub fn ModeTag(mode: Option<i64>, #[prop(optional)] short: bool) -> impl IntoView {
    let (code, cls) = mode_badge(mode);
    let fam = if cls == "b-sl" {
        "sl"
    } else if code == "ARAM" {
        "aram"
    } else {
        "qm"
    };
    let long = match code {
        "SL" => "Storm League",
        "HL" => "Hero League",
        "TL" => "Team League",
        "UD" => "Unranked",
        "QM" => "Quick Match",
        "ARAM" => "ARAM",
        "CUSTOM" => "Custom",
        c => c,
    };
    view! { <span class=format!("ds-mode {fam}") title=long>{if short { code } else { long }}</span> }
}

/// Award tag: MVP in gold, the others outlined.
#[component]
pub fn AwardTag(raw: Option<String>) -> impl IntoView {
    award_label(raw.as_deref()).map(|aw| {
        if aw.mvp {
            view! { <span class="ds-tag mvp">"👑 MVP"</span> }.into_any()
        } else {
            view! { <span class="ds-tag award" title=aw.label.clone()>{format!("{} {}", aw.icon, aw.label)}</span> }.into_any()
        }
    })
}

#[component]
pub fn Kda(p: MatchPlayer) -> impl IntoView {
    let (k, d, a) = kda(&p);
    view! { <span class="ds-kda">{k}<i>"/"</i>{d}<i>"/"</i>{a}</span> }
}

/// Game of the operator (Session): map banner, hero, K/D/A, result, award.
#[component]
pub fn GameCard(m: MatchSummary, me: MatchPlayer, won: bool) -> impl IntoView {
    let map = m.map.clone().unwrap_or_default();
    let art = storm_web_core::aggregates::has_map_art(&map);
    let t = m.played_at.as_deref().map(net::hhmm).unwrap_or_default();
    let aw = award_label(me.award.as_deref());
    let hero = me.hero.clone();
    view! {
        <Link to=format!("/match/{}", m.id) class=format!("ds-gcard {}", if won { "w" } else { "l" })>
            <div class=if art { "ds-gcard-map" } else { "ds-gcard-map noart" } style=if art { crate::ui::map_art(&map) } else { String::new() }>
                <ModeTag mode=m.mode /><span class="time">{t}</span>
            </div>
            <div class="ds-gcard-body">
                {hero.clone().map(|h| view! { <Portrait hero=h size=56 tone=if won { Tone::Pick } else { Tone::Loss } /> })}
                <div style="min-width: 0">
                    <strong>{m.map.clone().unwrap_or_else(|| "?".into())}</strong>
                    <em>{format!("{} · {}", hero.unwrap_or_else(|| "?".into()), fmt_dur(m.length))}</em>
                    <Kda p=me />
                </div>
                <span class="ds-res">{if won { "WIN" } else { "LOSS" }}</span>
            </div>
            {match aw {
                Some(a) => view! { <div class="ds-gcard-aw">{if a.mvp { "👑 MVP".to_string() } else { format!("◆ {}", a.label) }}</div> }.into_any(),
                None => view! { <div class="ds-gcard-aw none">"no award"</div> }.into_any(),
            }}
        </Link>
    }
}

/// Filter field with a text search: an input and a native `<datalist>` (typeahead); ✕ clears it.
#[component]
pub fn SearchSelect(
    options: Signal<Vec<String>>,
    value: Signal<String>,
    on_change: impl Fn(String) + Copy + Send + Sync + 'static,
    placeholder: &'static str,
    #[prop(into)] style: String,
    id: &'static str,
) -> impl IntoView {
    let pad = move || {
        if value.with(String::is_empty) {
            String::new()
        } else {
            "; padding-right: 20px".to_string()
        }
    };
    view! {
        <span style="position: relative; display: inline-flex; align-items: center">
            <input list=id class="ds-input" prop:value=move || value.get() placeholder=placeholder
                style=move || format!("{style}{}", pad())
                on:input=move |e| on_change(event_target_value(&e)) />
            <Show when=move || !value.with(String::is_empty)>
                <span on:click=move |_| on_change(String::new()) title="clear"
                    style="position: absolute; right: 6px; cursor: pointer; color: var(--text-2); font-size: 11px; line-height: 1">"✕"</span>
            </Show>
            <datalist id=id>
                {move || options.get().into_iter().map(|o| view! { <option value=o></option> }).collect_view()}
            </datalist>
        </span>
    }
}
