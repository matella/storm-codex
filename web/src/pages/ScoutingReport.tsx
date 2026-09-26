import { useMemo, useRef, useState } from "react";
import type { ReactNode } from "react";
import { useNavigate, useParams } from "react-router-dom";
import { useQueryClient } from "@tanstack/react-query";
import { scoutingWrite, useScoutingReport } from "../api";
import { Avatar } from "../components/Avatar";
import {
  claimState, day, fmtRate, fmtWilson, mapsWithPlans, pidNames, statusMeta, targetNames, uploadLabel,
} from "../scouting";
import type { Count, Evidence, Facts, HeroCall, Rate, ScoutGame, ScoutingReport as Report } from "../scouting";

type Tab = "overview" | "players" | "draft" | "maps" | "games" | "roster";
const TABS: [Tab, string][] = [
  ["overview", "Overview"], ["maps", "Maps"], ["draft", "Draft"],
  ["players", "Players"], ["games", "Games"], ["roster", "Roster"],
];
const inp = { background: "var(--surface-2)", border: "1px solid var(--hairline-strong)", color: "var(--text)", borderRadius: 6, padding: "5px 9px", fontSize: 12 } as const;

const fmtLen = (s: number | null | undefined) =>
  s == null ? "?" : `${Math.floor(s / 60)}:${String(Math.floor(s % 60)).padStart(2, "0")}`;

// ── preuves ─────────────────────────────────────────────────────────────────

function Ev({ e }: { e: Evidence }) {
  if (!e.known) return <span className="sc-ev bad" title={`Unknown fact id: ${e.id}`}>? {e.id}</span>;
  return (
    <span className={e.low ? "sc-ev low" : "sc-ev"} title={`${e.id}${e.low ? " — low sample: fewer than 3 games" : ""}`}>
      <span className="muted">{e.label}:</span> {e.text}
    </span>
  );
}

function Claim({ c, children }: { c: { evidence: Evidence[]; unsupported: boolean }; children: ReactNode }) {
  const st = claimState(c);
  return (
    <div className="sc-claim">
      {children}
      {st === "unsupported" && <span className="sc-flag dn">UNSUPPORTED</span>}
      {st === "partial" && <span className="sc-flag" style={{ color: "#fac775" }}>SOME IDS UNKNOWN</span>}
      <div>{c.evidence.map((e, i) => <Ev key={i} e={e} />)}</div>
    </div>
  );
}

function Kpi({ label, value, sub }: { label: string; value: ReactNode; sub?: ReactNode }) {
  return (
    <div className="sc-kpi">
      <div className="l">{label}</div>
      <div className="v mono">{value}</div>
      {sub && <div className="s">{sub}</div>}
    </div>
  );
}

function CountList({ title, items, empty = "—" }: { title: string; items: Count[]; empty?: string }) {
  return (
    <div className="card" style={{ margin: 0 }}>
      <div className="card-hd"><h2 style={{ fontSize: 12 }}>{title}</h2></div>
      {items.length === 0 && <div className="empty" style={{ padding: 14 }}>{empty}</div>}
      {items.map((c) => (
        <div key={c.id} className="row" title={c.id}>
          <Avatar hero={c.key} size={20} />
          <span style={{ fontSize: 12 }}>{c.key}</span>
          <span className="mono muted" style={{ marginLeft: "auto", fontSize: 11, opacity: c.count.n < 3 ? 0.6 : 1 }}>{fmtRate(c.count)}</span>
        </div>
      ))}
    </div>
  );
}

// ── page ────────────────────────────────────────────────────────────────────

export function ScoutingReport() {
  const { id } = useParams();
  const { data: r, error, isLoading } = useScoutingReport(id);
  const [tab, setTab] = useState<Tab>("overview");
  const [msg, setMsg] = useState<string | null>(null);
  const [importOpen, setImportOpen] = useState(false);
  const qc = useQueryClient();
  const nav = useNavigate();
  const refresh = () => qc.invalidateQueries({ queryKey: ["scouting"] });
  const flash = (m: string) => { setMsg(m); setTimeout(() => setMsg(null), 5000); };

  if (isLoading) return <div className="empty">loading…</div>;
  if (error || !r) return <div className="empty">Report not found. <a href="/scouting">Back to reports</a></div>;

  const facts = r.snapshot?.facts ?? null;
  const st = statusMeta(r.status);

  const patch = async (body: object) => {
    const res = await scoutingWrite(`/api/scouting/${r.id}`, {
      method: "PATCH", headers: { "Content-Type": "application/json" }, body: JSON.stringify(body),
    });
    if (res.status === 401) flash("✗ unauthorized — set the admin token in Admin");
    else if (!res.ok) flash(`✗ HTTP ${res.status}`);
    refresh();
  };

  const copyPack = async () => {
    try {
      const text = await (await fetch(`/api/scouting/${r.id}/pack.md`)).text();
      await navigator.clipboard.writeText(text);
      flash(`✓ pack copied (${Math.round(text.length / 1000)} k characters) — paste it into ChatGPT, Claude or your local model`);
    } catch {
      flash("✗ could not copy — use Download .md instead");
    }
  };

  const remove = async () => {
    if (!confirm(`Delete the report “${r.title}” and its replays?`)) return;
    const res = await scoutingWrite(`/api/scouting/${r.id}`, { method: "DELETE" });
    if (res.ok) { refresh(); nav("/scouting"); } else flash(`✗ HTTP ${res.status}`);
  };

  return (
    <>
      <p className="kick" style={{ marginTop: 18 }}><a href="/scouting">Scouting</a> / report #{r.id}</p>
      <Header r={r} onPatch={patch} />
      <div style={{ display: "flex", flexWrap: "wrap", gap: 8, alignItems: "center", margin: "10px 0" }}>
        <span className={`bdg ${st.cls}`}>{st.label}</span>
        <span className="muted" style={{ fontSize: 11 }}>
          created {day(r.created_at)} · updated {day(r.updated_at)} · facts v{r.facts_version}
          {r.analysis_imported_at && ` · analysis imported ${day(r.analysis_imported_at)}${r.analysis_model ? ` (${r.analysis_model})` : ""}`}
        </span>
        <span style={{ marginLeft: "auto", display: "flex", gap: 6, flexWrap: "wrap" }}>
          <span className="pill" onClick={copyPack} title="Copy the LLM pack (Markdown) to the clipboard">Copy pack</span>
          <a className="pill" href={`/api/scouting/${r.id}/pack.md`} download>Download .md</a>
          <a className="pill" href={`/api/scouting/${r.id}/pack.xlsx`} download>Download .xlsx</a>
          <span className="pill on" onClick={() => setImportOpen(true)}>Import analysis</span>
          <span className="pill" onClick={remove} title="Delete this report">Delete</span>
        </span>
      </div>
      {msg && <div className="toast mono" style={{ borderRadius: 6 }}>{msg}</div>}
      <Banners r={r} facts={facts} go={setTab} />

      <div style={{ display: "flex", gap: 6, margin: "14px 0 4px" }}>
        {TABS.map(([t, label]) => (
          <span key={t} className={tab === t ? "pill on" : "pill"} onClick={() => setTab(t)}>{label}</span>
        ))}
      </div>

      {tab === "overview" && <Overview r={r} facts={facts} go={setTab} />}
      {tab === "players" && <Players facts={facts} />}
      {tab === "draft" && <Draft r={r} facts={facts} />}
      {tab === "maps" && <Maps r={r} facts={facts} />}
      {tab === "games" && <Games r={r} onChanged={refresh} flash={flash} />}
      {tab === "roster" && <Roster key={r.facts_version} r={r} onPatch={patch} />}
      {importOpen && <ImportModal r={r} onClose={() => setImportOpen(false)} onDone={refresh} />}
    </>
  );
}

function Header({ r, onPatch }: { r: Report; onPatch: (b: object) => Promise<void> }) {
  const [editing, setEditing] = useState(false);
  const [title, setTitle] = useState(r.title);
  const [target, setTarget] = useState(r.target_name ?? "");
  const save = async () => {
    setEditing(false);
    if (title.trim() && (title !== r.title || target !== (r.target_name ?? ""))) await onPatch({ title, target_name: target });
  };
  if (editing) {
    return (
      <div style={{ display: "flex", gap: 8, alignItems: "center", margin: "6px 0" }}>
        <input autoFocus style={{ ...inp, fontSize: 16, flex: 2 }} value={title} onChange={(e) => setTitle(e.target.value)}
          onKeyDown={(e) => { if (e.key === "Enter") save(); if (e.key === "Escape") setEditing(false); }} />
        <input style={{ ...inp, flex: 1 }} placeholder="team name" value={target} onChange={(e) => setTarget(e.target.value)}
          onKeyDown={(e) => { if (e.key === "Enter") save(); if (e.key === "Escape") setEditing(false); }} />
        <span className="pill on" onClick={save}>Save</span>
      </div>
    );
  }
  return (
    <h1 style={{ cursor: "text" }} title="Click to rename" onClick={() => { setTitle(r.title); setTarget(r.target_name ?? ""); setEditing(true); }}>
      {r.title} <span className="muted" style={{ fontSize: 13 }}>— {r.target_name ?? "unnamed team"} ✎</span>
    </h1>
  );
}

function Banners({ r, facts, go }: { r: Report; facts: Facts | null; go: (t: Tab) => void }) {
  const out: ReactNode[] = [];
  const s = r.snapshot;
  if (s?.detection.ambiguous && s.roster_auto && r.anchors.length === 0 && r.games.length > 0) {
    out.push(
      <div key="amb" className="sc-banner">
        These replays don't tell which team to scout (the same players keep facing each other). Set an{" "}
        <b style={{ cursor: "pointer", textDecoration: "underline" }} onClick={() => go("roster")}>anchor player</b> from the team you're scouting.
      </div>,
    );
  }
  if (facts && facts.overview.excluded > 0) {
    out.push(
      <div key="exc" className="sc-banner">
        {facts.overview.excluded} replay{facts.overview.excluded > 1 ? "s" : ""} left out: the target team wasn't found (fewer than 3 roster players and no anchor).{" "}
        <b style={{ cursor: "pointer", textDecoration: "underline" }} onClick={() => go("games")}>Pick the side by hand</b> or adjust the roster.
      </div>,
    );
  }
  if (r.status === "stale") {
    out.push(
      <div key="stale" className="sc-banner">
        The analysis was written on facts v{r.analysis_facts_version}; replays or roster changed since (now v{r.facts_version}). Copy the pack again and re-import to refresh it.
      </div>,
    );
  }
  return <>{out}</>;
}

// ── onglets ─────────────────────────────────────────────────────────────────

function NoFacts({ r, go }: { r: Report; go: (t: Tab) => void }) {
  return (
    <div className="card"><div className="empty">
      {r.games.length === 0 ? "No replays yet. " : "No game with the target team identified yet. "}
      <b style={{ cursor: "pointer", textDecoration: "underline" }} onClick={() => go(r.games.length === 0 ? "games" : "roster")}>
        {r.games.length === 0 ? "Add replays" : "Check the roster"}
      </b>
    </div></div>
  );
}

function rateKpi(label: string, r: Rate, rec?: Rate, recLabel = "win rate then") {
  return <Kpi label={label} value={fmtRate(r)} sub={rec && rec.n > 0 ? `${recLabel}: ${fmtRate(rec)}` : undefined} />;
}

function Confidence({ c }: { c: string | null }) {
  if (!c) return null;
  return <span className={`bdg ${c === "high" ? "b-win" : c === "low" ? "b-qm" : "b-live"}`}>confidence: {c}</span>;
}

/** Un héros de l'analyse : ban (avec phase), pick, ou pick adverse attendu (avec joueur). */
function HeroCallRow({ h, kind, names }: { h: HeroCall; kind: "ban" | "pick" | "theirs"; names: Record<string, string> }) {
  const badge = kind === "ban"
    ? <span className="bdg b-loss">ban{h.phase ? ` · ${h.phase === "mid" ? "mid" : "1st phase"}` : ""}</span>
    : kind === "pick" ? <span className="bdg b-win">pick</span> : <span className="bdg b-live">they pick</span>;
  return (
    <Claim c={h}>
      <span style={{ display: "inline-flex", alignItems: "center", gap: 8 }}>
        {badge}<Avatar hero={h.hero} size={20} /><b>{h.hero}</b>
        {kind === "theirs" && h.player && <span className="muted">({names[h.player] ?? h.player})</span>}
      </span>
      <span> — {h.why}</span>
    </Claim>
  );
}

function PlanSection({ title, children, empty }: { title: string; children: ReactNode[]; empty?: boolean }) {
  if (empty) return null;
  return (
    <>
      <div className="kick" style={{ padding: "10px 18px 0", margin: 0 }}>{title}</div>
      {children}
    </>
  );
}

function Overview({ r, facts, go }: { r: Report; facts: Facts | null; go: (t: Tab) => void }) {
  if (!facts || facts.overview.games === 0) return <NoFacts r={r} go={go} />;
  const o = facts.overview, fl = facts.flow, d = facts.draft;
  const a = r.analysis;
  const choice = a?.map_choice ?? { pick: [], avoid: [] };
  return (
    <>
      {a ? (
        <div className="card">
          <div className="card-hd"><h2>Draft plan</h2>
            {r.analysis_tally && (
              <span className="muted mono" style={{ marginLeft: "auto", fontSize: 11 }}>
                {r.analysis_tally.claims} items · {r.analysis_tally.unsupported} unsupported · {r.analysis_tally.unknown_ids} unknown ids
              </span>
            )}
          </div>
          <div className="sc-claim" style={{ fontSize: 13, color: "var(--text)", whiteSpace: "pre-wrap" }}>{a.summary || <span className="muted">no summary</span>}</div>
          <div className="sc-claim muted" style={{ fontSize: 11 }}>
            Bans and picks map by map: <b style={{ cursor: "pointer", textDecoration: "underline" }} onClick={() => go("maps")}>Maps</b> ·
            on any map: <b style={{ cursor: "pointer", textDecoration: "underline" }} onClick={() => go("draft")}>Draft</b>
          </div>
        </div>
      ) : (
        <div className="card"><div className="empty">No draft plan yet — <b>Copy pack</b>, paste it into your LLM, then <b>Import analysis</b>.</div></div>
      )}
      {(choice.pick.length > 0 || choice.avoid.length > 0) && (
        <div className="card">
          <div className="card-hd"><h2>Map choice</h2><span className="muted" style={{ fontSize: 11 }}>when we get to pick the map</span></div>
          {choice.pick.map((m, i) => <Claim key={`p${i}`} c={m}><span className="bdg b-win" style={{ marginRight: 8 }}>pick</span><b>{m.map}</b> — {m.why}</Claim>)}
          {choice.avoid.map((m, i) => <Claim key={`a${i}`} c={m}><span className="bdg b-loss" style={{ marginRight: 8 }}>avoid</span><b>{m.map}</b> — {m.why}</Claim>)}
        </div>
      )}
      <p className="cap">Key facts — seen from {r.target_name ?? "the target team"}</p>
      <div className="card"><div className="sc-grid">
        <Kpi label="Record" value={fmtRate(o.record)} sub={`${day(o.first_date)} → ${day(o.last_date)}`} />
        <Kpi label="Avg length" value={fmtLen(o.avg_length_s)} sub={`builds ${o.builds.join(", ") || "?"}`} />
        {rateKpi("Had first pick", d.first_pick, d.first_pick_record, "win rate with FP")}
        {rateKpi("First to level 10", fl.first_to_10, fl.first_to_10_record)}
        {rateKpi("First fort", fl.first_fort, fl.first_fort_record)}
        {rateKpi("First objective", fl.first_objective, fl.first_objective_record)}
        <Kpi label="Comebacks" value={fmtRate(fl.comebacks)} sub="wins after trailing by 2+ levels" />
        <Kpi label="Throws" value={fmtRate(fl.throws)} sub="losses after leading by 2+ levels" />
        {fl.level_diff.map((l) => (
          <Kpi key={l.id} label={`Level lead at ${l.minute} min`} value={l.n ? `${l.avg >= 0 ? "+" : ""}${l.avg.toFixed(1)}` : "—"} sub={`${l.n} game${l.n === 1 ? "" : "s"}`} />
        ))}
        {fl.length.map((b) => <Kpi key={b.id} label={`Games ${b.label}`} value={fmtRate(b.record)} />)}
      </div></div>
    </>
  );
}

function Players({ facts }: { facts: Facts | null }) {
  if (!facts || facts.players.length === 0) return <div className="card"><div className="empty">No player data yet.</div></div>;
  return (
    <>
      {facts.players.map((p) => {
        const s = p.stats;
        return (
          <div key={p.pid} className="card">
            <div className="card-hd">
              <Avatar hero={p.heroes[0]?.hero ?? null} size={28} />
              <h2>{p.name}</h2>
              <span className="mono muted" style={{ fontSize: 11 }}>{p.pid}</span>
              {!p.core && <span className="bdg b-qm">substitute</span>}
              <span className="mono" style={{ marginLeft: "auto", fontSize: 11 }}>{fmtRate(p.record)}</span>
            </div>
            <div className="row" style={{ flexWrap: "wrap", gap: 12 }}>
              {p.heroes.map((h) => (
                <span key={h.id} title={h.id} style={{ display: "inline-flex", alignItems: "center", gap: 6, opacity: h.picks.k < 3 ? 0.75 : 1 }}>
                  <Avatar hero={h.hero} size={22} />
                  <span style={{ fontSize: 12 }}>{h.hero}</span>
                  <span className="mono muted" style={{ fontSize: 10 }}>{h.picks.k}g · {fmtRate(h.record)}</span>
                </span>
              ))}
            </div>
            <div className="row mono muted" style={{ fontSize: 10, flexWrap: "wrap" }}>
              {p.roles.map((x) => `${x.key} ×${x.count.k}`).join(" · ")}
              <span style={{ marginLeft: "auto" }}>
                K/D/A {s.kills.toFixed(1)}/{s.deaths.toFixed(1)}/{s.assists.toFixed(1)} · KP {s.kill_participation_pct.toFixed(0)}% ·
                dmg {s.hero_damage_pm.toFixed(0)}/min · siege {s.siege_damage_pm.toFixed(0)}/min · heal {s.healing_pm.toFixed(0)}/min ·
                dead {s.time_dead_pct.toFixed(1)}%
              </span>
            </div>
          </div>
        );
      })}
    </>
  );
}

function Draft({ r, facts }: { r: Report; facts: Facts | null }) {
  if (!facts || facts.overview.games === 0) return <div className="card"><div className="empty">No draft data in these replays.</div></div>;
  const d = facts.draft;
  const g = r.analysis?.general;
  const names = pidNames(facts);
  const grid = { display: "grid", gridTemplateColumns: "repeat(auto-fill, minmax(250px, 1fr))", gap: 12, margin: "12px 0" } as const;
  const faced = d.faced ?? [];
  return (
    <>
      {g && (g.bans.length + g.picks.length + g.considerations.length > 0) && (
        <div className="card">
          <div className="card-hd"><h2>Draft plan — any map</h2></div>
          {g.bans.map((h, i) => <HeroCallRow key={`b${i}`} h={h} kind="ban" names={names} />)}
          {g.picks.map((h, i) => <HeroCallRow key={`p${i}`} h={h} kind="pick" names={names} />)}
          {g.considerations.map((c, i) => <Claim key={`c${i}`} c={c}>{c.point}</Claim>)}
        </div>
      )}
      <div className="card"><div className="sc-grid">
        {rateKpi("Had first pick", d.first_pick)}
        <Kpi label="Win rate with first pick" value={fmtRate(d.first_pick_record)} />
        <Kpi label="Win rate with second pick" value={fmtRate(d.second_pick_record)} />
        <Kpi label="Drafts analysed" value={d.games} />
      </div></div>
      <div style={grid}>
        <CountList title="Their first-phase bans" items={d.bans_first} />
        <CountList title="Their mid-draft bans" items={d.bans_mid} />
        <CountList title="Banned against them" items={d.bans_against} />
        <CountList title="Their first pick" items={d.openers} />
        <CountList title="Their last pick" items={d.last_picks} />
        <div className="card" style={{ margin: 0 }}>
          <div className="card-hd"><h2 style={{ fontSize: 12 }}>Role of their first pick</h2></div>
          {d.opener_roles.map((c) => (
            <div key={c.id} className="row"><span style={{ fontSize: 12 }}>{c.key}</span>
              <span className="mono muted" style={{ marginLeft: "auto", fontSize: 11 }}>{fmtRate(c.count)}</span></div>
          ))}
        </div>
      </div>
      {faced.length > 0 && (
        <div className="card">
          <div className="card-hd"><h2>Heroes picked against them</h2><span className="muted" style={{ fontSize: 11 }}>games faced · their record in those games</span></div>
          <div className="row" style={{ flexWrap: "wrap", gap: 14 }}>
            {faced.map((h) => (
              <span key={h.id} title={h.id} style={{ display: "inline-flex", alignItems: "center", gap: 6, opacity: h.picks.k < 3 ? 0.75 : 1 }}>
                <Avatar hero={h.hero} size={22} />
                <span style={{ fontSize: 12 }}>{h.hero}</span>
                <span className="mono muted" style={{ fontSize: 10 }}>{h.picks.k}g · {fmtRate(h.record)}</span>
              </span>
            ))}
          </div>
        </div>
      )}
    </>
  );
}

function Maps({ r, facts }: { r: Report; facts: Facts | null }) {
  const rows = mapsWithPlans(facts, r.analysis?.maps);
  if (rows.length === 0) return <div className="card"><div className="empty">No map data yet.</div></div>;
  const names = pidNames(facts);
  const list = (cs: Count[]) => cs.map((c) => `${c.key} ×${c.count.k}`).join(", ") || "—";
  return (
    <>
      {rows.map(({ map, facts: m, plan }) => (
        <div key={map} className="card">
          <div className="card-hd">
            <h2>{map}</h2>
            {m && <span className="mono" style={{ fontSize: 11 }}>{fmtRate(m.record)}</span>}
            {m && <span className="mono muted" style={{ fontSize: 10 }}>{fmtWilson(m.wilson)}</span>}
            {!m && <span className="bdg b-qm">not in these replays</span>}
            <span style={{ marginLeft: "auto" }}>{plan && <Confidence c={plan.confidence} />}</span>
          </div>
          {m && (
            <div className="row" style={{ flexDirection: "column", alignItems: "stretch", gap: 4, fontSize: 11 }}>
              <div><span className="muted">They picked: </span>
                {m.picks.map((h) => `${h.hero}${h.by?.length ? ` (${h.by.map((p) => names[p] ?? p).join(", ")})` : ""}`).join(" · ") || "—"}</div>
              <div><span className="muted">They banned: </span>{list(m.bans)}</div>
              <div><span className="muted">Banned against them: </span>{list(m.bans_against)}</div>
            </div>
          )}
          {plan ? (
            <>
              {plan.overview && <Claim c={plan}><span style={{ color: "var(--text)" }}>{plan.overview}</span></Claim>}
              <PlanSection title="We ban" empty={plan.bans.length === 0}>{plan.bans.map((h, i) => <HeroCallRow key={i} h={h} kind="ban" names={names} />)}</PlanSection>
              <PlanSection title="We pick" empty={plan.picks.length === 0}>{plan.picks.map((h, i) => <HeroCallRow key={i} h={h} kind="pick" names={names} />)}</PlanSection>
              <PlanSection title="They will likely pick" empty={plan.their_picks.length === 0}>{plan.their_picks.map((h, i) => <HeroCallRow key={i} h={h} kind="theirs" names={names} />)}</PlanSection>
              <PlanSection title="Consider" empty={plan.considerations.length === 0}>{plan.considerations.map((c, i) => <Claim key={i} c={c}>{c.point}</Claim>)}</PlanSection>
            </>
          ) : (
            r.analysis && <div className="sc-claim muted">No plan for this map in the analysis — see the Draft tab for advice on any map.</div>
          )}
        </div>
      ))}
    </>
  );
}

function Games({ r, onChanged, flash }: { r: Report; onChanged: () => void; flash: (m: string) => void }) {
  const [queue, setQueue] = useState<{ name: string; state: string }[]>([]);
  const [over, setOver] = useState(false);
  const input = useRef<HTMLInputElement>(null);

  const upload = async (files: File[]) => {
    const list = files.filter((f) => f.name.toLowerCase().endsWith(".stormreplay"));
    if (list.length === 0) { flash("✗ drop .StormReplay files"); return; }
    setQueue(list.map((f) => ({ name: f.name, state: "waiting" })));
    for (let i = 0; i < list.length; i++) {
      const f = list[i];
      setQueue((q) => q.map((x, j) => (j === i ? { ...x, state: "uploading…" } : x)));
      let state: string;
      try {
        const res = await scoutingWrite(`/api/scouting/${r.id}/replays`, {
          method: "POST",
          headers: { "X-Filename": encodeURIComponent(f.name) },
          body: await f.arrayBuffer(),
        });
        state = res.status === 401 ? "unauthorized — set the admin token in Admin" : res.ok ? uploadLabel(await res.json()) : `HTTP ${res.status}`;
      } catch {
        state = "network error";
      }
      setQueue((q) => q.map((x, j) => (j === i ? { ...x, state } : x)));
      onChanged();
    }
  };

  const setSide = async (g: ScoutGame, team: number | null) => {
    const res = await scoutingWrite(`/api/scouting/${r.id}/replays/${g.gid}`, {
      method: "PATCH", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ target_team: team }),
    });
    if (!res.ok) flash(`✗ HTTP ${res.status}`);
    onChanged();
  };
  const del = async (g: ScoutGame) => {
    if (!confirm(`Remove ${g.map ?? "this game"} (${day(g.played_at)}) from the report?`)) return;
    const res = await scoutingWrite(`/api/scouting/${r.id}/replays/${g.gid}`, { method: "DELETE" });
    if (!res.ok) flash(`✗ HTTP ${res.status}`);
    onChanged();
  };

  return (
    <>
      <div className="card">
        <div
          className={over ? "sc-drop over" : "sc-drop"}
          onClick={() => input.current?.click()}
          onDragOver={(e) => { e.preventDefault(); setOver(true); }}
          onDragLeave={() => setOver(false)}
          onDrop={(e) => { e.preventDefault(); setOver(false); upload([...e.dataTransfer.files]); }}
        >
          Drop .StormReplay files here, or click to choose
          <input ref={input} type="file" multiple accept=".StormReplay,.stormreplay" style={{ display: "none" }}
            onChange={(e) => { upload([...(e.target.files ?? [])]); e.target.value = ""; }} />
        </div>
        {queue.map((q, i) => (
          <div key={i} className="row mono" style={{ fontSize: 11 }}>
            <span className="muted" style={{ overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{q.name}</span>
            <span style={{ marginLeft: "auto" }} className={q.state.startsWith("added") && !q.state.includes("not found") ? "up" : q.state === "waiting" || q.state.endsWith("…") ? "muted" : "dn"}>{q.state}</span>
          </div>
        ))}
      </div>

      <div className="card">
        <div className="card-hd"><h2>Replays</h2><span className="muted" style={{ fontSize: 11 }}>click a team to set it as the scouted side by hand</span></div>
        {r.games.length === 0 && <div className="empty">No replays yet.</div>}
        {r.games.map((g) => {
          const won = g.target_team != null && g.winner != null ? g.winner === g.target_team : null;
          return (
            <div key={g.gid} className="row" style={{ alignItems: "flex-start", flexWrap: "wrap" }}>
              <div style={{ width: 150 }}>
                <div style={{ fontSize: 12 }}>{g.map ?? "?"}</div>
                <div className="muted mono" style={{ fontSize: 10 }}>{day(g.played_at)} · {fmtLen(g.length_s)}</div>
              </div>
              <div style={{ width: 70 }}>
                {won === null ? <span className="bdg b-qm">?</span> : <span className={`bdg ${won ? "b-win" : "b-loss"}`}>{won ? "WIN" : "LOSS"}</span>}
              </div>
              <div style={{ flex: 1, display: "flex", flexDirection: "column", gap: 2, minWidth: 260 }}>
                {g.teams.map((t, side) => (
                  <span key={side} className={g.target_team === side ? "sc-team on" : "sc-team"} onClick={() => setSide(g, side)} title="Set as the scouted team">
                    {t.map((p) => p.name).join(" · ")}
                  </span>
                ))}
              </div>
              <div style={{ width: 150, textAlign: "right", display: "flex", flexDirection: "column", gap: 4, alignItems: "flex-end" }}>
                {targetNames(g) ? <span className="bdg b-live">by {g.target_source}</span> : <span className="bdg b-mvp">team not found</span>}
                <span style={{ display: "flex", gap: 6 }}>
                  {g.target_source === "manual" && <span className="pill" onClick={() => setSide(g, null)} title="Back to automatic detection">auto</span>}
                  <span className="pill" onClick={() => del(g)} title="Remove from the report">remove</span>
                </span>
              </div>
            </div>
          );
        })}
      </div>
    </>
  );
}

function Roster({ r, onPatch }: { r: Report; onPatch: (b: object) => Promise<void> }) {
  const snap = r.snapshot;
  const candidates = new Set(snap?.detection.candidates.map((c) => c.toon) ?? []);
  // tous les joueurs vus, avec leur nombre de parties
  const seen = useMemo(() => {
    const m = new Map<string, { name: string; games: number }>();
    for (const g of r.games) for (const t of g.teams) for (const p of t) {
      const e = m.get(p.toon) ?? { name: p.name, games: 0 };
      e.games += 1; e.name = p.name;
      m.set(p.toon, e);
    }
    return [...m.entries()].sort((a, b) =>
      Number(candidates.has(b[0])) - Number(candidates.has(a[0])) || b[1].games - a[1].games || a[1].name.localeCompare(b[1].name));
  }, [r.games, snap]);
  const [roster, setRoster] = useState<string[]>(snap?.roster ?? []);
  const [anchors, setAnchors] = useState<string[]>(r.anchors);
  const toggle = (list: string[], set: (v: string[]) => void, t: string) =>
    set(list.includes(t) ? list.filter((x) => x !== t) : [...list, t]);
  const dirty = JSON.stringify([...roster].sort()) !== JSON.stringify([...(snap?.roster ?? [])].sort())
    || JSON.stringify([...anchors].sort()) !== JSON.stringify([...r.anchors].sort());

  return (
    <div className="card">
      <div className="card-hd">
        <h2>Roster</h2>
        <span className="muted" style={{ fontSize: 11 }}>
          {snap?.roster_auto ? "auto-detected" : "set by hand"} · a game counts when 3+ roster players are on one side, otherwise the anchor's side
        </span>
        <span style={{ marginLeft: "auto", display: "flex", gap: 6 }}>
          {!snap?.roster_auto && <span className="pill" onClick={() => onPatch({ roster: [] })}>Back to auto</span>}
          <span className={dirty ? "pill on" : "pill"} onClick={() => dirty && onPatch({ roster, anchors })}>Save</span>
        </span>
      </div>
      {seen.length === 0 && <div className="empty">Add replays first.</div>}
      {seen.map(([toon, p]) => (
        <div key={toon} className="row">
          <input type="checkbox" checked={roster.includes(toon)} onChange={() => toggle(roster, setRoster, toon)} title="In the main roster" />
          <span style={{ fontSize: 12 }}>{p.name}</span>
          {candidates.has(toon) && <span className="bdg b-live">candidate</span>}
          <span className="mono muted" style={{ fontSize: 10 }}>{toon}</span>
          <span className="mono muted" style={{ marginLeft: "auto", fontSize: 11 }}>{p.games} game{p.games === 1 ? "" : "s"}</span>
          <span className={anchors.includes(toon) ? "pill on" : "pill"} onClick={() => toggle(anchors, setAnchors, toon)} title="Anchor: this player's side is the scouted team">⚓ anchor</span>
        </div>
      ))}
    </div>
  );
}

function ImportModal({ r, onClose, onDone }: { r: Report; onClose: () => void; onDone: () => void }) {
  const [text, setText] = useState("");
  const [result, setResult] = useState<{ ok: boolean; lines: string[] } | null>(null);
  const [busy, setBusy] = useState(false);
  const submit = async () => {
    if (!text.trim()) return;
    if (r.analysis && !confirm(`Replace the analysis imported on ${day(r.analysis_imported_at)}?`)) return;
    setBusy(true);
    try {
      const res = await scoutingWrite(`/api/scouting/${r.id}/analysis`, { method: "PUT", headers: { "Content-Type": "text/plain" }, body: text });
      const body = await res.json().catch(() => ({}));
      if (res.ok) {
        const t = body.tally ?? {};
        setResult({ ok: true, lines: [`✓ imported — ${t.claims ?? 0} claims, ${t.unsupported ?? 0} unsupported, ${t.unknown_ids ?? 0} unknown fact ids`, ...(body.warnings ?? []).map((w: string) => `⚠ ${w}`)] });
        onDone();
      } else {
        setResult({ ok: false, lines: [`✗ ${res.status === 401 ? "unauthorized — set the admin token in Admin" : body.error ?? `HTTP ${res.status}`}`, "Nothing was changed."] });
      }
    } catch {
      setResult({ ok: false, lines: ["✗ network error"] });
    }
    setBusy(false);
  };
  return (
    <div className="sc-modal-bg" onClick={onClose}>
      <div className="sc-modal" onClick={(e) => e.stopPropagation()}>
        <div className="card-hd"><h2>Import analysis</h2><span className="pill" style={{ marginLeft: "auto" }} onClick={onClose}>close</span></div>
        <div style={{ padding: "12px 18px", display: "flex", flexDirection: "column", gap: 10 }}>
          <span className="note">Paste the LLM's whole reply (the JSON block can be surrounded by text), or load it from a file.
            {r.analysis && " The current analysis will be replaced."}</span>
          <textarea value={text} onChange={(e) => setText(e.target.value)} rows={14}
            style={{ ...inp, fontFamily: "JetBrains Mono, Consolas, monospace", fontSize: 11, resize: "vertical" }}
            placeholder={'```json\n{ "format_version": 1, "report_id": ' + r.id + ', ... }\n```'} />
          <div style={{ display: "flex", gap: 8, alignItems: "center" }}>
            <input type="file" accept=".json,.txt,.md" onChange={async (e) => { const f = e.target.files?.[0]; if (f) setText(await f.text()); }} style={{ fontSize: 11, color: "var(--muted-2)" }} />
            <span className={busy ? "pill" : "pill on"} style={{ marginLeft: "auto" }} onClick={() => !busy && submit()}>{busy ? "importing…" : "Import"}</span>
          </div>
          {result && result.lines.map((l, i) => <div key={i} className={`mono ${result.ok && i === 0 ? "up" : result.ok ? "muted" : "dn"}`} style={{ fontSize: 11 }}>{l}</div>)}
        </div>
      </div>
    </div>
  );
}
