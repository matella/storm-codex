//! Admin: identity, upload health, tokens, teams and collections. Port of `web/src/pages/Admin.tsx`.

use leptos::{prelude::*, task::spawn_local};
use serde_json::Value;
use storm_web_core::net;

use crate::ui::{fetch, use_operator};

const CSS: &str = r#"
.admin .mode.open{font:12px var(--mono);color:var(--muted-2)}
.admin .arow{display:flex;flex-wrap:wrap;gap:8px;align-items:center}
.admin .arow.pad{padding:14px 16px}
.admin .amsg{margin-top:10px;font:12px var(--mono)}
.admin .ahint{margin:12px 0 0;font:11px var(--mono);color:var(--kicker);line-height:1.6}
.admin .tiles{display:flex;flex-wrap:wrap;gap:10px}
.admin .tile{min-width:110px;padding:10px 14px;border-radius:12px;background:var(--panel-2);border:1px solid var(--line);box-shadow:inset 3px 0 0 var(--line-2)}
.admin .tile b{display:block;font:900 34px/1 var(--display)}
.admin .tile span{font:600 10px var(--mono);letter-spacing:.12em;text-transform:uppercase;color:var(--muted-2)}
.admin .tile.done,.admin .tile.parsed,.admin .tile.ok{box-shadow:inset 3px 0 0 var(--win)} .admin .tile.done b,.admin .tile.parsed b,.admin .tile.ok b{color:var(--win)}
.admin .tile.failed,.admin .tile.parse_failed{box-shadow:inset 3px 0 0 var(--loss)} .admin .tile.failed b,.admin .tile.parse_failed b{color:var(--loss-soft)}
.admin .tile.duplicate,.admin .tile.pending{box-shadow:inset 3px 0 0 var(--gold)}
.admin .newtok{margin-top:14px;padding:12px 14px;border-radius:12px;border:1px solid rgba(93,202,165,.35);background:rgba(93,202,165,.07);display:flex;flex-direction:column;gap:6px}
.admin .newtok code{font:600 13px var(--mono);color:var(--win);word-break:break-all;user-select:all}
.admin .acols{display:grid;grid-template-columns:1fr 1fr;gap:0 18px}
.admin .lrow{display:flex;align-items:center;gap:10px;padding:10px 16px;border-top:1px solid var(--line)}
.admin .lrow strong{font:800 17px var(--display);text-transform:uppercase;letter-spacing:.02em;min-width:0;overflow:hidden;text-overflow:ellipsis;white-space:nowrap}
.admin .lrow .ds-input.small{margin-left:auto;width:120px;padding:5px 9px;font-size:12px}
.admin .lrow .cnt{font:11px var(--mono);color:var(--muted-2);white-space:nowrap}
.admin .none{font:12px var(--mono);color:var(--kicker)} .admin .none.pad{padding:16px}
@media (max-width:900px){.admin .acols{grid-template-columns:1fr}}
"#;

fn text(v: &Value, key: &str) -> String {
    v.get(key)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

fn entries(v: &Value, key: &str) -> Vec<(String, i64)> {
    v.get(key)
        .and_then(Value::as_object)
        .map(|m| {
            m.iter()
                .map(|(k, n)| (k.clone(), n.as_i64().unwrap_or(0)))
                .collect()
        })
        .unwrap_or_default()
}

fn null_if_empty(s: String) -> Value {
    if s.is_empty() {
        Value::Null
    } else {
        s.into()
    }
}

#[component]
fn Sec(
    num: Signal<u32>,
    title: &'static str,
    #[prop(optional, into)] note: Option<ViewFn>,
    children: Children,
) -> impl IntoView {
    view! {
        <section class="ds-sec" style="margin-top: 34px">
            <div class="ds-sec-hd">
                <span class="num">{move || format!("{:02}", num.get())}</span>
                <h2>{title}</h2>
                {note.map(|n| view! { <small>{n.run()}</small> })}
            </div>
            {children()}
        </section>
    }
}

/// Runs an authenticated write, then `then` with the body when the answer is a 2xx.
fn act(
    method: &'static str,
    url: String,
    body: Option<Value>,
    then: impl FnOnce(String) + 'static,
) {
    spawn_local(async move {
        if let Ok((s, t)) = net::write(method, &url, body.map(|b| b.to_string()), None).await {
            if (200..300).contains(&s) {
                then(t);
            }
        }
    });
}

#[component]
pub fn Admin() -> impl IntoView {
    let token = RwSignal::new(net::admin_token());
    let set_token = move |v: String| {
        net::storage_set("admin_token", &v);
        token.set(v);
    };
    let bump = RwSignal::new(0u32);
    let refresh = move |_: String| bump.update(|b| *b += 1);
    let settings = fetch::<Value>(|| "/api/settings".to_string());
    let admin_open = move || {
        settings
            .get()
            .and_then(Result::ok)
            .is_some_and(|s| s.get("admin_open").and_then(Value::as_bool) == Some(true))
    };
    let health = LocalResource::new(move || {
        let (t, open) = (token.get(), admin_open());
        async move {
            if t.is_empty() && !open {
                return None;
            }
            let (_, body) = net::write("GET", "/api/admin/uploads", None, Some(&t))
                .await
                .ok()?;
            serde_json::from_str::<Value>(&body).ok()
        }
    });
    let teams = fetch::<Vec<Value>>(move || {
        bump.track();
        "/api/teams".to_string()
    });
    let collections = fetch::<Vec<Value>>(move || {
        bump.track();
        "/api/collections".to_string()
    });
    let teams_l = move || teams.get().and_then(Result::ok).unwrap_or_default();
    let colls_l = move || collections.get().and_then(Result::ok).unwrap_or_default();

    let token_name = RwSignal::new(String::new());
    let new_token = RwSignal::new(None::<String>);
    let op_names = RwSignal::new(None::<String>);
    let op_msg = RwSignal::new(None::<String>);
    let operator = use_operator();
    // the typed value if touched, otherwise the loaded settings
    let op_value = move || {
        op_names.get().unwrap_or_else(|| {
            settings
                .get()
                .and_then(Result::ok)
                .and_then(|s| {
                    s.get("operator_names").and_then(Value::as_array).map(|a| {
                        a.iter()
                            .filter_map(Value::as_str)
                            .collect::<Vec<_>>()
                            .join(", ")
                    })
                })
                .unwrap_or_default()
        })
    };
    let save_operator = move || {
        if token.get_untracked().is_empty() && !admin_open() {
            op_msg.set(Some("⚠ enter the admin token above first".to_string()));
            return;
        }
        let names: Vec<String> = op_value()
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        let body = serde_json::json!({ "operator_names": names }).to_string();
        spawn_local(async move {
            let m = match net::write("PUT", "/api/admin/settings", Some(body), None).await {
                Ok((s, _)) if (200..300).contains(&s) => {
                    operator.set(names);
                    settings.refetch();
                    "✓ saved".to_string()
                }
                Ok((401 | 403, _)) => "✗ unauthorized — check the admin token".to_string(),
                Ok((s, _)) => format!("✗ save failed (HTTP {s})"),
                Err(_) => "✗ network error — server unreachable".to_string(),
            };
            op_msg.set(Some(m));
            net::after_ms(5000, move || op_msg.set(None));
        });
    };

    let team_name = RwSignal::new(String::new());
    let team_league = RwSignal::new(String::new());
    let coll_name = RwSignal::new(String::new());
    let create_token = move || {
        let body = serde_json::json!({ "name": token_name.get_untracked() });
        act("POST", "/api/admin/tokens".into(), Some(body), move |t| {
            let tok = serde_json::from_str::<Value>(&t)
                .ok()
                .and_then(|v| v.get("token").and_then(Value::as_str).map(str::to_string));
            if let Some(tok) = tok {
                new_token.set(Some(tok));
                token_name.set(String::new());
            }
        });
    };
    let create_team = move || {
        let body = serde_json::json!({
            "name": team_name.get_untracked(),
            "roster": [],
            "league": null_if_empty(team_league.get_untracked()),
        });
        act("POST", "/api/teams".into(), Some(body), move |_| {
            team_name.set(String::new());
            team_league.set(String::new());
            refresh(String::new());
        });
    };
    let set_league = move |id: i64, l: String| {
        let body = serde_json::json!({ "league": null_if_empty(l) });
        act("PUT", format!("/api/teams/{id}"), Some(body), refresh);
    };
    let create_coll = move || {
        let body = serde_json::json!({ "name": coll_name.get_untracked(), "match_ids": [] });
        act("POST", "/api/collections".into(), Some(body), move |_| {
            coll_name.set(String::new());
            refresh(String::new());
        });
    };
    let reprocess = move || act("POST", "/api/admin/reprocess".into(), None, |_| {});
    let del = move |url: String| act("DELETE", url, None, refresh);

    let has_health = move || health.get().flatten().is_some();
    let n_health = Signal::derive(|| 2);
    let n_tokens = Signal::derive(move || if has_health() { 3 } else { 2 });
    let n_teams = Signal::derive(move || if has_health() { 4 } else { 3 });
    let n_colls = Signal::derive(move || if has_health() { 5 } else { 4 });

    view! {
        <div class="ds-page admin">
            <style>{CSS}</style>
            <header class="ds-cover" style="margin-top: 18px">
                <div class="ds-kicker">"Operator · settings"</div>
                <h1 class="ds-title" style="cursor: default; font-size: clamp(44px, 7vw, 88px)">"Admin"</h1>
                <div class="ds-subtitle" style="cursor: default">"Identity, upload pipeline, tokens, teams and collections."</div>
                <div class="ds-cover-foot">
                    {move || if admin_open() {
                        view! { <span class="mode open">"🔓 Local mode — no admin token required (server has no ADMIN_TOKEN set). Set ADMIN_TOKEN in the server env to re-enable auth (recommended if exposed publicly)."</span> }.into_any()
                    } else {
                        view! {
                            <span class="ds-label" style="margin: 0">"Admin token"</span>
                            <input class="ds-input" style="flex: 1; min-width: 220px" type="password" placeholder="ADMIN_TOKEN"
                                prop:value=move || token.get() on:input=move |e| set_token(event_target_value(&e)) />
                            <span class=move || if token.get().is_empty() { "ds-tag loss" } else { "ds-tag win" }>
                                {move || if token.get().is_empty() { "required for actions" } else { "configured" }}
                            </span>
                        }.into_any()
                    }}
                </div>
            </header>

            <Sec num=Signal::derive(|| 1) title="My identity" note=ViewFn::from(|| "operator perspective")>
                <div class="ds-panel">
                    <div class="arow">
                        <input class="ds-input" style="flex: 1" placeholder="my in-game names, comma-separated (e.g. matella, MatellaSmurf)"
                            prop:value=op_value on:input=move |e| op_names.set(Some(event_target_value(&e))) />
                        <button class="ds-btn primary" on:click=move |_| save_operator()>"Save"</button>
                    </div>
                    {move || op_msg.get().map(|m| {
                        let color = if m.starts_with('✓') { "color: var(--win)" } else { "color: var(--loss-soft)" };
                        view! { <div class="amsg" style=color>{m}</div> }
                    })}
                    <p class="ahint">"Multiple accounts? Comma-separate them (e.g. matella, матella). Cyrillic / any UTF-8 is fine. Used everywhere (session, matches, widget) and by the Jarvis brief."</p>
                </div>
            </Sec>

            {move || health.get().flatten().map(|h| {
                let (by_status, by_class) = (entries(&h, "by_status"), entries(&h, "by_error_class"));
                let ver = match h.get("parser_version") {
                    Some(Value::String(s)) => s.clone(),
                    Some(Value::Number(n)) => n.to_string(),
                    _ => String::new(),
                };
                view! {
                    <Sec num=n_health title="Upload health" note=ViewFn::from(move || view! { "parser "<b>{ver.clone()}</b> })>
                        <div class="ds-panel">
                            <div class="ds-label">"By status"</div>
                            <div class="tiles">
                                {by_status.is_empty().then(|| view! { <span class="none">"no uploads yet"</span> })}
                                {by_status.into_iter().map(|(k, n)| { let class = format!("tile {k}"); view! { <div class=class><b>{n}</b><span>{k}</span></div> } }).collect_view()}
                            </div>
                            <div class="ds-label" style="margin-top: 16px">"Failures by class"</div>
                            <div class="tiles">
                                {by_class.is_empty().then(|| view! { <span class="none">"no failures"</span> })}
                                {by_class.into_iter().map(|(k, n)| view! { <div class="tile failed"><b>{n}</b><span>{k}</span></div> }).collect_view()}
                            </div>
                            <div class="arow" style="margin-top: 16px">
                                <button class="ds-btn" on:click=move |_| reprocess()>"Re-process stale parser_version ›"</button>
                            </div>
                        </div>
                    </Sec>
                }
            })}

            <Sec num=n_tokens title="Upload tokens" note=ViewFn::from(|| "one per uploader (client-rs)")>
                <div class="ds-panel">
                    <div class="arow">
                        <input class="ds-input" placeholder="name (e.g. matella)" prop:value=move || token_name.get() on:input=move |e| token_name.set(event_target_value(&e)) />
                        <button class="ds-btn primary" on:click=move |_| create_token()>"Create"</button>
                    </div>
                    {move || new_token.get().map(|t| view! {
                        <div class="newtok">
                            <span class="ds-label" style="margin: 0">"New token — copy it now, it won't be shown again"</span>
                            <code>{t}</code>
                        </div>
                    })}
                </div>
            </Sec>

            <div class="acols">
                <Sec num=n_teams title="Teams" note=ViewFn::from(move || teams_l().len().to_string())>
                    <div class="ds-panel flush">
                        <div class="arow pad">
                            <input class="ds-input" style="flex: 1" placeholder="team name" prop:value=move || team_name.get() on:input=move |e| team_name.set(event_target_value(&e)) />
                            <input class="ds-input" style="width: 140px" placeholder="league (optional)" prop:value=move || team_league.get() on:input=move |e| team_league.set(event_target_value(&e)) />
                            <button class="ds-btn primary" on:click=move |_| create_team()>"Add"</button>
                        </div>
                        <For each=teams_l key=|t| t.to_string() let:t>
                            {
                                let id = t.get("id").and_then(Value::as_i64).unwrap_or(0);
                                let members = t.get("roster").and_then(Value::as_array).map_or(0, Vec::len);
                                view! {
                                    <div class="lrow">
                                        <strong>{text(&t, "name")}</strong>
                                        <input class="ds-input small" placeholder="league" value=text(&t, "league") on:blur=move |e| set_league(id, event_target_value(&e)) />
                                        <span class="cnt">{format!("{members} members")}</span>
                                        <button class="ds-btn small danger" on:click=move |_| del(format!("/api/teams/{id}"))>"Delete"</button>
                                    </div>
                                }
                            }
                        </For>
                        {move || teams_l().is_empty().then(|| view! { <div class="none pad">"no teams"</div> })}
                    </div>
                </Sec>

                <Sec num=n_colls title="Collections" note=ViewFn::from(move || colls_l().len().to_string())>
                    <div class="ds-panel flush">
                        <div class="arow pad">
                            <input class="ds-input" style="flex: 1" placeholder="collection name" prop:value=move || coll_name.get() on:input=move |e| coll_name.set(event_target_value(&e)) />
                            <button class="ds-btn primary" on:click=move |_| create_coll()>"Add"</button>
                        </div>
                        <For each=colls_l key=|c| c.to_string() let:c>
                            {
                                let id = c.get("id").and_then(Value::as_i64).unwrap_or(0);
                                view! {
                                    <div class="lrow">
                                        <strong>{text(&c, "name")}</strong>
                                        <span class="cnt" style="margin-left: auto">{format!("{} matches", c.get("count").and_then(Value::as_i64).unwrap_or(0))}</span>
                                        <button class="ds-btn small danger" on:click=move |_| del(format!("/api/collections/{id}"))>"Delete"</button>
                                    </div>
                                }
                            }
                        </For>
                        {move || colls_l().is_empty().then(|| view! { <div class="none pad">"no collections"</div> })}
                    </div>
                </Sec>
            </div>
        </div>
    }
}
