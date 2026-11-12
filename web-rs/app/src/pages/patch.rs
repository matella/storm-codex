//! Patch: cover, sticky table of contents (from `tableOfContents`, anchors scroll, links to hero pages), and the
//! content converted by `patch_headings` then sanitised by `SafeHtml`. Port of `web/src/pages/Patch.tsx`.

use leptos::prelude::*;
use leptos_router::hooks::use_params_map;
use storm_web_core::{
    net,
    patchnotes::{hero_classes, patch_headings, ptag_class, toc, PatchDetail},
    route::uri_component,
    sanitize::is_web_link,
};

use crate::{
    safehtml::SafeHtml,
    ui::{fetch, Link},
};

#[component]
pub fn Patch() -> impl IntoView {
    let params = use_params_map();
    let data = fetch::<Option<PatchDetail>>(move || {
        format!(
            "/api/patches/{}",
            uri_component(&params.read().get("id").unwrap_or_default())
        )
    });
    move || {
        match data.get() {
        None => view! { <div class="ds-page"><div class="ds-empty" style="margin-top: 24px">"loading…"</div></div> }.into_any(),
        Some(Ok(Some(d))) => {
            let classes = hero_classes(&d);
            let entries = toc(&d);
            let has_toc = !entries.is_empty();
            let nav = has_toc.then(|| view! {
                <nav class="ds-toc" aria-label="On this page">
                    <div class="ds-label">"On this page"</div>
                    {entries.iter().map(|t| {
                        let is_hero = t.section_type == "Hero";
                        let class = classes.get(t.anchor.as_str()).copied();
                        view! {
                            <div class=if is_hero { "it" } else { "it sec" } style=format!("padding-left: {}px", (t.heading_level - 2).max(0) * 12)>
                                <a href=format!("#{}", t.anchor)>{t.title.clone()}</a>
                                {class.map(|c| view! { <span class=format!("ds-ptag {}", ptag_class(Some(c))) style="padding: 1px 5px">{c.to_string()}</span> })}
                                {is_hero.then(|| view! {
                                    <Link to=format!("/hero/{}", uri_component(&t.title)) class="muted" style="margin-left: auto">"↗"</Link>
                                })}
                            </div>
                        }
                    }).collect_view()}
                </nav>
            });
            let content = match d.content.as_deref().filter(|c| !c.is_empty()) {
                Some(c) => view! { <SafeHtml class="ds-prose patch-content" html=patch_headings(c) keep_ids=true /> }.into_any(),
                None => view! { <div class="ds-empty">"No content for this patch."</div> }.into_any(),
            };
            view! {
                <div class="ds-page">
                    <header class="ds-cover" style="margin-top: 18px">
                        <div class="ds-kicker">{format!("{} · {}", d.patch_type, net::fmt_time(d.live_date.as_deref()))}</div>
                        <h1 class="ds-title" style="cursor: default; font-size: clamp(40px, 6vw, 80px)">{d.patch_name.clone()}</h1>
                        <div class="ds-cover-foot">
                            <Link to="/patches" class="ds-btn">"‹ All patches"</Link>
                            <Link to="/hero-changes" class="ds-btn">"Changes by hero"</Link>
                            {d.official_link.clone().filter(|l| is_web_link(l)).map(|l| view! {
                                <a href=l target="_blank" rel="noreferrer" class="ds-btn primary">"Official notes ↗"</a>
                            })}
                        </div>
                    </header>
                    <div class=if has_toc { "ds-patchgrid" } else { "" } style=if has_toc { "" } else { "margin-top: 26px" }>
                        {nav}
                        {content}
                    </div>
                </div>
            }.into_any()
        }
        Some(_) => view! {
            <div class="ds-page"><div class="ds-empty" style="margin-top: 24px">"Patch not found. "<Link to="/patches">"All patches"</Link></div></div>
        }.into_any(),
    }
    }
}
