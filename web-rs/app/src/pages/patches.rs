//! Patch Notes: the dated list of the official HotS patch notes (HotsPatchNotes proxy), filtered by type on the
//! client. Port of `web/src/pages/Patches.tsx` and `DateBlock` of `components/ds/patch.tsx`.

use leptos::prelude::*;
use leptos_router::hooks::use_navigate;
use storm_web_core::{
    aggregates::{patch_counts, patch_types, PatchList},
    net,
    route::uri_component,
};

use crate::{
    layout::LiveBus,
    ui::{fetch, Link},
};

/// Date block `20 / Apr 2026` of a patch row.
#[component]
pub fn DateBlock(iso: Option<String>) -> impl IntoView {
    match iso.filter(|i| !i.is_empty()) {
        None => view! { <span class="ds-pdate"><b>"?"</b></span> }.into_any(),
        Some(i) => {
            let (day, month_year) = net::date_block(&i);
            view! { <span class="ds-pdate"><b>{day}</b><span>{month_year}</span></span> }.into_any()
        }
    }
}

fn pill(on: bool) -> &'static str {
    if on {
        "ds-pill on"
    } else {
        "ds-pill"
    }
}

#[component]
pub fn Patches() -> impl IntoView {
    let nav = use_navigate();
    let kind = RwSignal::new(String::new());
    let bus = expect_context::<LiveBus>();
    let data = fetch::<PatchList>(move || {
        bus.patches.track(); // a new patch re-reads the list
        "/api/patches".to_string()
    });
    let items = move || match data.get() {
        Some(Ok(l)) => l.items,
        _ => Vec::new(),
    };
    view! {
        <div class="ds-page">
            <header class="ds-cover" style="margin-top: 18px">
                <div class="ds-kicker">"Patch notes"</div>
                <h1 class="ds-title" style="cursor: default">"Patch notes"</h1>
                <div class="ds-subtitle" style="cursor: default">
                    {move || format!("Official HotS patch notes — {} patches. Open one for details, or browse the changes hero by hero.", items().len())}
                </div>
                <div class="ds-filterbar">
                    <span class=move || pill(kind.with(String::is_empty)) on:click=move |_| kind.set(String::new())>"All"</span>
                    {move || patch_types(&items()).into_iter().map(|t| {
                        let (t2, t3) = (t.clone(), t.clone());
                        view! { <span class=move || pill(kind.with(|k| *k == t2)) on:click=move |_| kind.set(t3.clone())>{t}</span> }
                    }).collect_view()}
                    <Link to="/hero-changes" class="ds-btn primary" style="margin-left: auto">"Changes by hero ›"</Link>
                </div>
            </header>

            <section class="ds-sec" style="margin-top: 26px">
                {move || {
                    let loading = data.get().is_none();
                    let k = kind.get();
                    let rows: Vec<_> = items().into_iter().filter(|p| k.is_empty() || p.patch_type == k).collect();
                    let nav = nav.clone();
                    view! {
                        {loading.then(|| view! { <div class="ds-empty">"loading…"</div> })}
                        {(!loading && rows.is_empty()).then(|| view! { <div class="ds-empty">"No patch notes (referential unavailable?)."</div> })}
                        <div class="ds-plist">
                            {rows.into_iter().map(|p| {
                                let go = {
                                    let nav = nav.clone();
                                    let id = p.internal_id.clone();
                                    move || nav(&format!("/patch/{}", uri_component(&id)), Default::default())
                                };
                                let go2 = go.clone();
                                view! {
                                    <div class="ds-prow" role="link" tabindex="0" on:click=move |_| go() on:keydown=move |e| if e.key() == "Enter" { go2() }>
                                        <DateBlock iso=p.live_date.clone() />
                                        <span class="t">{p.patch_name.clone()}</span>
                                        <span class="ds-tag award">{p.patch_type.clone()}</span>
                                        <span class="c">{patch_counts(&p)}</span>
                                    </div>
                                }
                            }).collect_view()}
                        </div>
                    }
                }}
            </section>
        </div>
    }
}
