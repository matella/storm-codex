//! `ds` pieces for patch notes: the expandable patch entry (hero to patch). Port of `PatchEntry` of
//! `components/ds/patch.tsx` (`DateBlock` is in `pages/patches.rs`).

use leptos::prelude::*;
use storm_web_core::{
    net,
    patchnotes::{ptag_class, HeroPatchSection},
    route::uri_component,
};

use crate::{
    safehtml::SafeHtml,
    ui::{Link, Portrait},
};

/// A section of a patch about a hero: date, classification, patch and summary; content unfolds, a link opens the
/// whole patch. `hero` puts the hero first (the chronological view).
#[component]
pub fn PatchEntry(
    p: HeroPatchSection,
    #[prop(optional, into)] hero: Option<String>,
) -> impl IntoView {
    let class = p.classification.clone().unwrap_or_default().to_uppercase();
    let date = match p.live_date.as_deref().filter(|d| !d.is_empty()) {
        Some(d) => format!(
            "{}/{}",
            net::fmt_time(Some(d)).chars().take(5).collect::<String>(),
            d.chars().take(4).collect::<String>()
        ),
        None => "?".to_string(),
    };
    let summary_style = hero
        .is_some()
        .then_some("grid-template-columns: 34px 150px 110px 84px 1fr auto");
    let name = format!(
        "{}{}",
        p.patch_name.clone().unwrap_or_default(),
        p.short_summary
            .as_deref()
            .filter(|s| !s.is_empty())
            .map(|s| format!(" — {s}"))
            .unwrap_or_default()
    );
    let open = format!(
        "/patch/{}#{}",
        uri_component(&p.patch_internal_id),
        p.anchor
    );
    let label = if class.is_empty() {
        "UPDATE".to_string()
    } else {
        class.clone()
    };
    view! {
        <details class="ds-patch">
            <summary style=summary_style>
                {hero.clone().map(|h| view! { <Portrait hero=h size=30 /> })}
                {hero.clone().map(|h| view! {
                    <Link to=format!("/hero/{}", uri_component(&h)) style="font: 800 17px var(--display); text-transform: uppercase; text-decoration: none; color: var(--text)">{h}</Link>
                })}
                <span class="pdate">{date}</span>
                <span class=format!("ds-ptag {}", ptag_class(Some(&class)))>{label}</span>
                <span class="pname">{name}</span>
                <Link class="popen" to=open>"open ›"</Link>
            </summary>
            {p.content.clone().filter(|c| !c.is_empty()).map(|c| view! { <SafeHtml class="pbody" html=c /> })}
        </details>
    }
}

/// The list of patch entries. Keyed like the React one (`patchInternalId + anchor + index`), so an unfolded entry
/// stays with its own patch when the list changes. `with_hero` puts the hero first in each entry.
#[component]
pub fn PatchEntries(
    list: Vec<HeroPatchSection>,
    #[prop(optional)] with_hero: bool,
) -> impl IntoView {
    let indexed: Vec<(usize, HeroPatchSection)> = list.into_iter().enumerate().collect();
    view! {
        <div class="ds-patches">
            <For
                each=move || indexed.clone()
                key=|(i, p)| format!("{}{}{i}", p.patch_internal_id, p.anchor)
                children=move |(_, p)| {
                    if with_hero {
                        let hero = p.hero_name.clone();
                        view! { <PatchEntry p=p hero=hero /> }.into_any()
                    } else {
                        view! { <PatchEntry p=p /> }.into_any()
                    }
                }
            />
        </div>
    }
}
