// Page d'un rapport de scouting, présentée en « dossier » — même langage visuel que l'export HTML
// (reportHtml.ts, inchangé), plus les parties interactives : édition, dépôt, côté manuel, roster,
// import d'analyse, export. Styles : design system src/ds.css (composants components/ds).
import { useMemo, useRef, useState } from "react";
import type { CSSProperties, ReactNode } from "react";
import { useNavigate, useParams } from "react-router-dom";
import { useQueryClient } from "@tanstack/react-query";
import { minimapImage, scoutingWrite, useDimHeroes, useScoutingReport } from "../api";
import {
  CallCard, Chips, Consider, EvLine, Lane, mapAnchor, Meter, PickChips, Pips, Portrait, SecHead, TextChips,
} from "../components/ds";
import type { Names } from "../components/ds";
import { exportReportHtml } from "../reportExport";
import { humanize as humanizeSafe } from "../reportHtml";
import { day, fmtRate, mapsWithPlans, pidNames, uploadLabel } from "../scouting";
import type { Evidence, Facts, GameRow, MapFacts, MapPlan, Rate, ScoutGame, ScoutingReport as Report } from "../scouting";

const fmtLen = (s: number | null | undefined) =>
  s == null ? "?" : `${Math.floor(s / 60)}:${String(Math.floor(s % 60)).padStart(2, "0")}`;
const pct = (r: Rate | null | undefined) => (r && r.n ? Math.round((r.k * 100) / r.n) : null);
/** `2/3` seul — sous une grande valeur en %, qui porte déjà le pourcentage. */
const kn = (r: Rate | null | undefined) => (r && r.n ? `${r.k}/${r.n}` : "—");
const imgVar = (url: string | null) => (url ? ({ "--img": `url('${url}')` } as CSSProperties) : undefined);

export function ScoutingReport() {
  const { id } = useParams();
  const { data: r, error, isLoading } = useScoutingReport(id);
  useDimHeroes(); // portraits et univers (anneaux)
  const [msg, setMsg] = useState<{ text: string; bad: boolean } | null>(null);
  const [importOpen, setImportOpen] = useState(false);
  const [exporting, setExporting] = useState(false);
  const qc = useQueryClient();
  const nav = useNavigate();
  const refresh = () => qc.invalidateQueries({ queryKey: ["scouting"] });
  const flash = (text: string) => { setMsg({ text, bad: text.startsWith("✗") }); setTimeout(() => setMsg(null), 6000); };

  if (isLoading) return <div className="ds-page"><div className="ds-empty" style={{ marginTop: 24 }}>loading…</div></div>;
  if (error || !r) return <div className="ds-page"><div className="ds-empty" style={{ marginTop: 24 }}>Report not found. <a href="/scouting">Back to reports</a></div></div>;

  const facts = r.snapshot?.facts ?? null;
  const names = pidNames(facts);
  const hasGames = !!facts && facts.overview.games > 0;
  const rows = mapsWithPlans(facts, r.analysis?.maps);
  const choice = r.analysis?.map_choice;
  const hasChoice = !!choice && choice.pick.length + choice.avoid.length > 0;
  const g = r.analysis?.general;
  const hasGeneral = !!g && g.bans.length + g.picks.length + g.considerations.length > 0;

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
  const exportHtml = async () => {
    if (exporting) return;
    setExporting(true);
    try {
      const res = await exportReportHtml(r);
      flash(`✓ ${res.fileName} saved (${Math.round(res.bytes / 1024)} KB, ${res.portraits}/${res.heroes} portraits, ${res.maps} maps) — send it to your team`);
    } catch {
      flash("✗ export failed");
    }
    setExporting(false);
  };
  const remove = async () => {
    if (!confirm(`Delete the report “${r.title}” and its replays?`)) return;
    const res = await scoutingWrite(`/api/scouting/${r.id}`, { method: "DELETE" });
    if (res.ok) { refresh(); nav("/scouting"); } else flash(`✗ HTTP ${res.status}`);
  };

  // numérotation des sections présentes, dans l'ordre de lecture
  let n = 0;
  const next = () => ++n;

  return (
    <div className="ds-page">
      <div className="ds-crumb"><a href="/scouting">Scouting</a> / report #{r.id}</div>
      <Cover r={r} facts={facts} onPatch={patch}>
        <div className="ds-cover-foot">
          <span className={`ds-status ${r.status}`}>{statusText(r.status)}</span>
          <span className="ds-meta">
            created {day(r.created_at)} · updated {day(r.updated_at)} · facts v{r.facts_version}
            {r.analysis_imported_at && ` · plan imported ${day(r.analysis_imported_at)}${r.analysis_model ? ` (${r.analysis_model})` : ""}`}
          </span>
          <span className="ds-actions">
            <span className="ds-btn" onClick={copyPack} title="Copy the LLM pack (Markdown) to the clipboard">Copy pack</span>
            <a className="ds-btn" href={`/api/scouting/${r.id}/pack.md`} download>.md</a>
            <a className="ds-btn" href={`/api/scouting/${r.id}/pack.xlsx`} download>.xlsx</a>
            <span className="ds-btn" onClick={() => setImportOpen(true)}>Import analysis</span>
            <span className="ds-btn primary" onClick={exportHtml} aria-disabled={exporting} title="Save a self-contained, shareable HTML report for your teammates">
              {exporting ? "Exporting…" : "Export HTML"}
            </span>
            <span className="ds-btn danger" onClick={remove} title="Delete this report">Delete</span>
          </span>
        </div>
      </Cover>

      <nav className="ds-nav">
        {hasGames && <a href="#identity">Identity</a>}
        {hasChoice && <a href="#map-choice">Map choice</a>}
        {rows.map((x) => <a key={x.map} className="map" href={`#${mapAnchor(x.map)}`}>{x.map}</a>)}
        {hasGeneral && <a href="#general">Any map</a>}
        {hasGames && <a href="#players">Players</a>}
        {hasGames && <a href="#faced">Faced</a>}
        {hasGames && <a href="#flow">Game flow</a>}
        <a href="#games">Replays</a>
        <a href="#roster">Roster</a>
      </nav>
      {msg && <div className={msg.bad ? "ds-toast bad" : "ds-toast"}>{msg.text}</div>}
      <Banners r={r} facts={facts} />

      {hasGames && facts && (
        <>
          <Identity r={r} facts={facts} names={names} num={next()} />
          {hasChoice && <MapChoice r={r} names={names} num={next()} />}
          <section className="ds-sec" id="maps">
            <SecHead num={next()} title="Map by map" sub="bans · picks · what they will play" />
            {rows.map((x, i) => (
              <MapBoard key={x.map} map={x.map} m={x.facts} plan={x.plan} facts={facts} names={names} i={i} hasAnalysis={!!r.analysis} />
            ))}
          </section>
          {hasGeneral && <General r={r} names={names} num={next()} />}
          <Players facts={facts} num={next()} />
          <Faced facts={facts} num={next()} />
          <Flow facts={facts} num={next()} />
        </>
      )}
      <Games r={r} facts={facts} names={names} num={next()} onChanged={refresh} flash={flash} />
      <Roster key={r.facts_version} r={r} num={next()} onPatch={patch} />
      {importOpen && <ImportModal r={r} onClose={() => setImportOpen(false)} onDone={refresh} />}
    </div>
  );
}

function statusText(s: Report["status"]): string {
  switch (s) {
    case "empty": return "no replays";
    case "ready": return "pack ready";
    case "analyzed": return "draft plan ready";
    case "stale": return "plan outdated";
  }
}

// ── couverture (titre éditable en place) ────────────────────────────────────

function Cover({ r, facts, onPatch, children }: { r: Report; facts: Facts | null; onPatch: (b: object) => Promise<void>; children: ReactNode }) {
  const [editing, setEditing] = useState(false);
  const [title, setTitle] = useState(r.title);
  const [target, setTarget] = useState(r.target_name ?? "");
  const o = facts?.overview;
  const games = facts?.games ?? [];
  const roster = (facts?.players ?? []).filter((p) => p.core);
  const bg = facts?.maps[0] ? minimapImage(facts.maps[0].map) : null;
  const save = async () => {
    setEditing(false);
    if (title.trim() && (title !== r.title || target !== (r.target_name ?? ""))) await onPatch({ title, target_name: target });
  };
  const start = () => { setTitle(r.title); setTarget(r.target_name ?? ""); setEditing(true); };
  const keys = (e: React.KeyboardEvent) => { if (e.key === "Enter") save(); if (e.key === "Escape") setEditing(false); };

  return (
    <header className="ds-cover" style={imgVar(bg)}>
      <div className="ds-kicker">Scouting dossier · report #{r.id}</div>
      {editing ? (
        <div className="ds-edit">
          <input autoFocus className="ds-input big" placeholder="team name" value={target} onChange={(e) => setTarget(e.target.value)} onKeyDown={keys} />
          <input className="ds-input" style={{ flex: 2, minWidth: 220 }} placeholder="report title" value={title} onChange={(e) => setTitle(e.target.value)} onKeyDown={keys} />
          <span className="ds-btn primary" onClick={save}>Save</span>
          <span className="ds-btn" onClick={() => setEditing(false)}>Cancel</span>
        </div>
      ) : (
        <>
          <h1 className="ds-title" title="Click to rename" onClick={start}>{r.target_name ?? "Unnamed team"}</h1>
          <div className="ds-subtitle" title="Click to rename" onClick={start}>{r.title}<span className="edit">✎ RENAME</span></div>
        </>
      )}
      <div className="ds-statline">
        <div className="ds-stat big"><span className="ds-label">Record</span>
          <strong>{o ? <>{o.record.k}<span>–</span>{o.record.n - o.record.k}</> : "—"}</strong><Pips games={games} /></div>
        <div className="ds-stat"><span className="ds-label">Games</span><strong>{o?.games ?? 0}</strong>
          <small>{o && o.games > 0 ? `${day(o.first_date)} → ${day(o.last_date)}` : "add replays below"}</small></div>
        <div className="ds-stat"><span className="ds-label">Had first pick</span>
          <strong>{pct(facts?.draft.first_pick) ?? "—"}{pct(facts?.draft.first_pick) != null && <span>%</span>}</strong>
          <small>{kn(facts?.draft.first_pick)}</small></div>
        <div className="ds-stat"><span className="ds-label">Avg length</span><strong>{o && o.games ? fmtLen(o.avg_length_s) : "—"}</strong>
          <small>builds {o?.builds.join(", ") || "?"}</small></div>
        {o && o.games > 0 && o.games < 5 && (
          <div className="ds-sample">⚠ Small sample — {o.games} game{o.games === 1 ? "" : "s"}. Treat trends as hints, not certainties.</div>
        )}
      </div>
      {roster.length > 0 && (
        <div className="ds-roster">
          {roster.map((p) => (
            <div key={p.pid} className="ds-rm">
              {p.heroes[0] && <Portrait hero={p.heroes[0].hero} size={58} />}
              <div><strong>{p.name}</strong><span>{p.roles[0]?.key ?? ""}</span><em>{p.heroes.slice(0, 3).map((h) => h.hero).join(" · ")}</em></div>
            </div>
          ))}
        </div>
      )}
      {children}
    </header>
  );
}

function Banners({ r, facts }: { r: Report; facts: Facts | null }) {
  const s = r.snapshot;
  return (
    <>
      {s?.detection.ambiguous && s.roster_auto && r.anchors.length === 0 && r.games.length > 0 && (
        <div className="ds-banner">
          These replays don't tell which team to scout (the same players keep facing each other). Set an <a href="#roster">anchor player</a> from the team you're scouting.
        </div>
      )}
      {facts && facts.overview.excluded > 0 && (
        <div className="ds-banner">
          {facts.overview.excluded} replay{facts.overview.excluded > 1 ? "s" : ""} left out: the target team wasn't found (fewer than 3 roster players and no anchor).
          {" "}<a href="#games">Pick the side by hand</a> or adjust the roster.
        </div>
      )}
      {r.status === "stale" && (
        <div className="ds-banner">
          The draft plan was written on facts v{r.analysis_facts_version}; replays or roster changed since (now v{r.facts_version}). Copy the pack again and re-import to refresh it.
        </div>
      )}
      {!r.analysis && facts && facts.overview.games > 0 && (
        <div className="ds-banner" style={{ color: "var(--accent-2)", borderColor: "rgba(127,119,221,.35)", background: "rgba(127,119,221,.06)" }}>
          No draft plan yet — <b>Copy pack</b>, paste it into your LLM, then <b>Import analysis</b>. The numbers below come straight from the replays.
        </div>
      )}
    </>
  );
}

// ── sections du dossier ─────────────────────────────────────────────────────

function Identity({ r, facts, names, num }: { r: Report; facts: Facts; names: Names; num: number }) {
  const d = facts.draft;
  const t = r.analysis_tally;
  return (
    <section className="ds-sec" id="identity">
      <SecHead num={num} title="Draft identity" />
      <div className="ds-identity">
        <div>
          {r.analysis?.summary
            ? <blockquote className="ds-quote">{humanizeSafe(r.analysis.summary, names)}</blockquote>
            : <blockquote className="ds-quote muted">No draft plan imported yet — the numbers come straight from the replays.</blockquote>}
          {t && <div className="ds-tally">{t.claims} recommendations · {t.unsupported} unsupported · {t.unknown_ids} unknown fact ids</div>}
        </div>
        <div className="ds-tells">
          <div className="ds-tell"><span className="ds-label">They ban first</span><div className="ds-chips"><Chips list={d.bans_first} tone="ban" /></div></div>
          <div className="ds-tell"><span className="ds-label">They ban mid-draft</span><div className="ds-chips"><Chips list={d.bans_mid} tone="ban" /></div></div>
          <div className="ds-tell"><span className="ds-label">Their first pick</span><div className="ds-chips"><Chips list={d.openers} tone="them" /></div></div>
          <div className="ds-tell"><span className="ds-label">Their last pick</span><div className="ds-chips"><Chips list={d.last_picks} tone="them" /></div></div>
          <div className="ds-tell"><span className="ds-label">Banned against them</span><div className="ds-chips"><Chips list={d.bans_against} /></div></div>
          <div className="ds-tell"><span className="ds-label">Role of their first pick · first pick {fmtRate(d.first_pick)}</span>
            <div className="ds-chips"><TextChips list={d.opener_roles} /></div></div>
        </div>
      </div>
    </section>
  );
}

function MapChoice({ r, names, num }: { r: Report; names: Names; num: number }) {
  const c = r.analysis?.map_choice;
  if (!c) return null;
  const tile = (m: { map: string; why: string; evidence: Evidence[]; unsupported: boolean }, kind: "go" | "no", i: number) => (
    <a key={`${kind}${i}`} className={`ds-mtile ${kind}`} href={`#${mapAnchor(m.map)}`} style={imgVar(minimapImage(m.map))}>
      <span className="badge">{kind === "go" ? "PICK" : "AVOID"}</span>
      <strong>{m.map}</strong>
      <p>{humanizeSafe(m.why, names)}</p>
      <EvLine ev={m.evidence} unsupported={m.unsupported} names={names} map={m.map} />
    </a>
  );
  return (
    <section className="ds-sec" id="map-choice">
      <SecHead num={num} title="Map choice" sub="when we get to pick the map" />
      <div className="ds-mtiles">{c.pick.map((m, i) => tile(m, "go", i))}{c.avoid.map((m, i) => tile(m, "no", i))}</div>
    </section>
  );
}

function MapBoard({ map, m, plan, facts, names, i, hasAnalysis }: {
  map: string; m: MapFacts | null; plan: MapPlan | null; facts: Facts; names: Names; i: number; hasAnalysis: boolean;
}) {
  const games = facts.games.filter((x) => x.map === map);
  return (
    <article className="ds-mboard" id={mapAnchor(map)}>
      <div className="ds-mboard-hd" style={imgVar(minimapImage(map))}>
        <div>
          <span className="ds-idx">MAP {String(i + 1).padStart(2, "0")}</span>
          <h3>{map}</h3>
          <div className="ds-mboard-rec">
            {m ? <><span className="rec">{m.record.k}–{m.record.n - m.record.k}</span><Pips games={games} /></> : <span className="rec none">not in these replays</span>}
            {m?.wilson && <span className="ds-ci">95% CI {Math.round(m.wilson[0])}–{Math.round(m.wilson[1])}%</span>}
          </div>
        </div>
        {plan && <Meter conf={plan.confidence} />}
      </div>
      {plan?.overview && (
        <>
          <p className="ds-overview">{humanizeSafe(plan.overview, names)}</p>
          <EvLine ev={plan.evidence} unsupported={plan.unsupported} names={names} map={map} />
        </>
      )}
      {plan ? (
        <>
          <div className="ds-board">
            <Lane title="We ban" kind="ban" empty="no ban suggested">{plan.bans.map((h, k) => <CallCard key={k} h={h} tone="ban" names={names} map={map} />)}</Lane>
            <Lane title="We pick" kind="pick" empty="no pick suggested">{plan.picks.map((h, k) => <CallCard key={k} h={h} tone="pick" names={names} map={map} />)}</Lane>
            <Lane title="They will likely pick" kind="them" empty="—">{plan.their_picks.map((h, k) => <CallCard key={k} h={h} tone="them" names={names} map={map} />)}</Lane>
          </div>
          <Consider points={plan.considerations} names={names} map={map} />
        </>
      ) : (
        <div className="ds-noplan">{hasAnalysis ? <>No plan for this map — see <a href="#general">Any map</a>.</> : "No draft plan imported yet."}</div>
      )}
      {m && (
        <div className="ds-lasttime">
          <div><span className="ds-label">They played</span><div className="ds-chips"><PickChips list={m.picks} names={names} /></div></div>
          <div><span className="ds-label">They banned</span><div className="ds-chips"><Chips list={m.bans} tone="ban" /></div></div>
          <div><span className="ds-label">Banned against them</span><div className="ds-chips"><Chips list={m.bans_against} /></div></div>
        </div>
      )}
    </article>
  );
}

function General({ r, names, num }: { r: Report; names: Names; num: number }) {
  const g = r.analysis?.general;
  if (!g) return null;
  return (
    <section className="ds-sec" id="general">
      <SecHead num={num} title="Any map" sub="applies everywhere, including maps missing from the replays" />
      <div className="ds-board two">
        <Lane title="We ban" kind="ban" empty="—">{g.bans.map((h, k) => <CallCard key={k} h={h} tone="ban" names={names} />)}</Lane>
        <Lane title="We pick" kind="pick" empty="—">{g.picks.map((h, k) => <CallCard key={k} h={h} tone="pick" names={names} />)}</Lane>
      </div>
      <Consider points={g.considerations} names={names} />
    </section>
  );
}

function Players({ facts, num }: { facts: Facts; num: number }) {
  if (!facts.players.length) return null;
  return (
    <section className="ds-sec" id="players">
      <SecHead num={num} title="Their players" />
      <div className="ds-pcards">
        {facts.players.map((p) => {
          const max = Math.max(1, ...p.heroes.map((h) => h.picks.k));
          return (
            <div key={p.pid} className="ds-pcard">
              <div className="ds-pcard-hd">
                {p.heroes[0] && <Portrait hero={p.heroes[0].hero} size={72} />}
                <div><strong>{p.name}</strong><span>{p.roles.map((x) => x.key).join(" / ")}{p.core ? "" : " · substitute"}</span><em>{fmtRate(p.record)} wins</em></div>
              </div>
              {p.heroes.map((h) => (
                <div key={h.id} className="ds-pool-row" title={h.id}>
                  <Portrait hero={h.hero} size={24} /><span>{h.hero}</span>
                  <i className="ds-bar"><b style={{ width: `${Math.round((h.picks.k * 100) / max)}%` }} /></i>
                  <em>{h.picks.k}g · {pct(h.record)}%</em>
                </div>
              ))}
              <div className="ds-pstats">
                K/D/A <b>{p.stats.kills.toFixed(1)}/{p.stats.deaths.toFixed(1)}/{p.stats.assists.toFixed(1)}</b> · KP <b>{p.stats.kill_participation_pct.toFixed(0)}%</b> ·
                dmg <b>{p.stats.hero_damage_pm.toFixed(0)}</b>/min · heal <b>{p.stats.healing_pm.toFixed(0)}</b>/min · dead <b>{p.stats.time_dead_pct.toFixed(1)}%</b>
              </div>
            </div>
          );
        })}
      </div>
    </section>
  );
}

function Faced({ facts, num }: { facts: Facts; num: number }) {
  const list = [...(facts.draft.faced ?? [])].sort((x, y) => (pct(x.record) ?? 0) - (pct(y.record) ?? 0) || y.picks.k - x.picks.k);
  if (!list.length) return null;
  return (
    <section className="ds-sec" id="faced">
      <SecHead num={num} title="What they faced" sub={<>heroes picked against them · their record in those games · <b className="lg-hot">green</b> = they never beat it</>} />
      <div className="ds-faced">
        {list.map((h) => {
          const p = pct(h.record) ?? 0;
          return (
            <div key={h.id} className={`ds-fc ${p === 0 ? "hot" : p === 100 ? "cold" : ""}`} title={h.id}>
              <Portrait hero={h.hero} size={44} /><span>{h.hero}</span><em>{h.record.k}/{h.record.n}</em>
            </div>
          );
        })}
      </div>
    </section>
  );
}

function Flow({ facts, num }: { facts: Facts; num: number }) {
  const fl = facts.flow;
  const tile = (label: string, r: Rate, sub?: string) => (
    <div className="ds-stat"><span className="ds-label">{label}</span><strong>{pct(r) ?? "—"}{pct(r) != null && <span>%</span>}</strong><small>{kn(r)}{sub ? ` · ${sub}` : ""}</small></div>
  );
  return (
    <section className="ds-sec" id="flow">
      <SecHead num={num} title="Game flow" sub="how their games tend to go" />
      <div className="ds-flow">
        {tile("First to level 10", fl.first_to_10, fl.first_to_10_record.n ? `won ${kn(fl.first_to_10_record)} then` : undefined)}
        {tile("First fort", fl.first_fort, fl.first_fort_record.n ? `won ${kn(fl.first_fort_record)} then` : undefined)}
        {tile("First objective", fl.first_objective, fl.first_objective_record.n ? `won ${kn(fl.first_objective_record)} then` : undefined)}
        {tile("Comebacks", fl.comebacks, "won after trailing by 2+ levels")}
        {tile("Throws", fl.throws, "lost after leading by 2+ levels")}
        {fl.level_diff.map((l) => (
          <div key={l.id} className="ds-stat"><span className="ds-label">Level lead at {l.minute} min</span>
            <strong>{l.n ? `${l.avg >= 0 ? "+" : ""}${l.avg.toFixed(1)}` : "—"}</strong><small>{l.n} game{l.n === 1 ? "" : "s"}</small></div>
        ))}
        {fl.length.filter((b) => b.record.n > 0).map((b) => tile(`Games ${b.label}`, b.record))}
      </div>
    </section>
  );
}

// ── replays (journal + gestion) ─────────────────────────────────────────────

function Games({ r, facts, names, num, onChanged, flash }: {
  r: Report; facts: Facts | null; names: Names; num: number; onChanged: () => void; flash: (m: string) => void;
}) {
  const [queue, setQueue] = useState<{ name: string; state: string }[]>([]);
  const [over, setOver] = useState(false);
  const input = useRef<HTMLInputElement>(null);
  const rowOf = useMemo(() => new Map((facts?.games ?? []).map((x) => [x.gid, x])), [facts]);

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
    if (team !== null && g.target_team === team && g.target_source === "manual") return;
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
  const cls = (s: string) => (s === "waiting" || s.endsWith("…") ? "wait" : s.startsWith("added") && !s.includes("not found") ? "ok" : "ko");

  return (
    <section className="ds-sec" id="games">
      <SecHead num={num} title="Replays" sub="click a team to set it as the scouted side by hand" />
      <div
        className={over ? "ds-drop over" : "ds-drop"}
        onClick={() => input.current?.click()}
        onDragOver={(e) => { e.preventDefault(); setOver(true); }}
        onDragLeave={() => setOver(false)}
        onDrop={(e) => { e.preventDefault(); setOver(false); upload([...e.dataTransfer.files]); }}
      >
        <b>Drop replays here</b>
        .StormReplay files of their games — or click to choose
        <input ref={input} type="file" multiple accept=".StormReplay,.stormreplay" style={{ display: "none" }}
          onChange={(e) => { upload([...(e.target.files ?? [])]); e.target.value = ""; }} />
      </div>
      {queue.length > 0 && (
        <div className="ds-queue">{queue.map((q, i) => <div key={i}><span>{q.name}</span><span className={cls(q.state)}>{q.state}</span></div>)}</div>
      )}
      {r.games.length === 0 ? null : (
        <div className="ds-glog">
          {r.games.map((g) => <GameLine key={g.gid} g={g} row={rowOf.get(g.gid) ?? null} names={names} onSide={setSide} onDelete={del} />)}
        </div>
      )}
    </section>
  );
}

function GameLine({ g, row, names, onSide, onDelete }: {
  g: ScoutGame; row: GameRow | null; names: Names;
  onSide: (g: ScoutGame, team: number | null) => void; onDelete: (g: ScoutGame) => void;
}) {
  const t = g.target_team;
  const won = t != null && g.winner != null ? g.winner === t : null;
  const side = (s: 0 | 1) => {
    const isTarget = t === s;
    const team = g.teams[s];
    // ordre de pick lu dans le replay quand la partie est analysée, sinon ordre du lobby
    const order = row ? (isTarget ? row.picks : t != null ? row.opp_picks : null) : null;
    const heroes = order ? order.map((p) => ({ hero: p.hero, who: isTarget ? names[p.player] ?? p.player : p.player })) : team.map((p) => ({ hero: p.hero, who: p.name }));
    const bans = row && t != null ? (isTarget ? row.bans : row.bans_against) : [];
    return (
      <div className={isTarget ? "ds-gl-side target" : "ds-gl-side"} onClick={() => onSide(g, s)} title="Set as the scouted team">
        <span className="ds-label"><span>{isTarget ? "Scouted team" : t == null ? `Team ${s === 0 ? "A" : "B"}` : "Opponent"}</span></span>
        <div className="ds-gl-picks">
          {heroes.map((h, i) => <span key={i}><Portrait hero={h.hero} size={30} /><em>{h.who}</em></span>)}
        </div>
        {bans.length > 0 && <div className="ds-gl-bans"><span className="ds-label">bans</span>{bans.map((b) => <Portrait key={b} hero={b} size={22} tone="ban" />)}</div>}
      </div>
    );
  };
  return (
    <div className={`ds-gl ${won === true ? "w" : won === false ? "l" : ""}`}>
      <div className="ds-gl-meta">
        <strong>{won === null ? "?" : won ? "WIN" : "LOSS"}</strong>
        <span>{g.map ?? "?"}</span>
        <em>{day(g.played_at)} · {fmtLen(g.length_s)}{row?.first_pick === true ? " · they had first pick" : row?.first_pick === false ? " · opponent had first pick" : ""}</em>
      </div>
      {side(0)}
      {side(1)}
      <div className="ds-gl-manage">
        {t != null ? <span className="ds-src">by {g.target_source}</span> : <span className="ds-src missing">team not found</span>}
        <span style={{ display: "flex", gap: 6 }}>
          {g.target_source === "manual" && <span className="ds-btn small" onClick={() => onSide(g, null)} title="Back to automatic detection">auto</span>}
          <span className="ds-btn small danger" onClick={() => onDelete(g)} title="Remove from the report">remove</span>
        </span>
      </div>
    </div>
  );
}

function Roster({ r, num, onPatch }: { r: Report; num: number; onPatch: (b: object) => Promise<void> }) {
  const snap = r.snapshot;
  const candidates = new Set(snap?.detection.candidates.map((c) => c.toon) ?? []);
  const seen = useMemo(() => {
    const m = new Map<string, { name: string; games: number; hero: string }>();
    for (const g of r.games) for (const t of g.teams) for (const p of t) {
      const e = m.get(p.toon) ?? { name: p.name, games: 0, hero: p.hero };
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
    <section className="ds-sec" id="roster">
      <SecHead num={num} title="Roster" sub={snap?.roster_auto ? "auto-detected" : "set by hand"} />
      <div className="ds-roster-hd">
        <span>A game counts when 3+ roster players are on one side, otherwise the anchor's side (⚓).</span>
        <span className="ds-actions">
          {!snap?.roster_auto && <span className="ds-btn" onClick={() => onPatch({ roster: [] })}>Back to auto</span>}
          <span className={dirty ? "ds-btn primary" : "ds-btn"} aria-disabled={!dirty} onClick={() => dirty && onPatch({ roster, anchors })}>Save roster</span>
        </span>
      </div>
      {seen.length === 0 && <div className="ds-empty">Add replays first.</div>}
      <div className="ds-roster-list">
        {seen.map(([toon, p]) => (
          <label key={toon} className={roster.includes(toon) ? "ds-roster-row in" : "ds-roster-row"}>
            <input type="checkbox" checked={roster.includes(toon)} onChange={() => toggle(roster, setRoster, toon)} title="In the main roster" />
            <Portrait hero={p.hero} size={30} />
            <strong>{p.name}</strong>
            {candidates.has(toon) && <span className="ds-cand">candidate</span>}
            <span className="toon">{toon}</span>
            <span className="games">{p.games} game{p.games === 1 ? "" : "s"}</span>
            <span className={anchors.includes(toon) ? "ds-btn small on" : "ds-btn small"}
              onClick={(e) => { e.preventDefault(); toggle(anchors, setAnchors, toon); }} title="Anchor: this player's side is the scouted team">⚓ anchor</span>
          </label>
        ))}
      </div>
    </section>
  );
}

function ImportModal({ r, onClose, onDone }: { r: Report; onClose: () => void; onDone: () => void }) {
  const [text, setText] = useState("");
  const [result, setResult] = useState<{ ok: boolean; lines: string[] } | null>(null);
  const [busy, setBusy] = useState(false);
  const submit = async () => {
    if (!text.trim()) return;
    if (r.analysis && !confirm(`Replace the draft plan imported on ${day(r.analysis_imported_at)}?`)) return;
    setBusy(true);
    try {
      const res = await scoutingWrite(`/api/scouting/${r.id}/analysis`, { method: "PUT", headers: { "Content-Type": "text/plain" }, body: text });
      const body = await res.json().catch(() => ({}));
      if (res.ok) {
        const t = body.tally ?? {};
        setResult({ ok: true, lines: [`✓ imported — ${t.claims ?? 0} recommendations, ${t.unsupported ?? 0} unsupported, ${t.unknown_ids ?? 0} unknown fact ids`, ...(body.warnings ?? []).map((w: string) => `⚠ ${w}`)] });
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
    <div className="ds-modal-bg" onClick={onClose}>
      <div className="ds-modal" onClick={(e) => e.stopPropagation()}>
        <div style={{ display: "flex", alignItems: "center", marginBottom: 12 }}>
          <h2>Import analysis</h2>
          <span className="ds-btn" style={{ marginLeft: "auto" }} onClick={onClose}>Close</span>
        </div>
        <div style={{ display: "flex", flexDirection: "column", gap: 10 }}>
          <span className="muted" style={{ fontSize: 13 }}>Paste the LLM's whole reply (the JSON block can be surrounded by text), or load it from a file.
            {r.analysis && " The current draft plan will be replaced."}</span>
          <textarea className="ds-input" value={text} onChange={(e) => setText(e.target.value)} rows={14}
            placeholder={'```json\n{ "format_version": 2, "report_id": ' + r.id + ', ... }\n```'} />
          <div style={{ display: "flex", gap: 8, alignItems: "center" }}>
            <input type="file" accept=".json,.txt,.md" onChange={async (e) => { const f = e.target.files?.[0]; if (f) setText(await f.text()); }} style={{ fontSize: 11, color: "var(--muted-2)" }} />
            <span className="ds-btn primary" style={{ marginLeft: "auto" }} aria-disabled={busy} onClick={() => !busy && submit()}>{busy ? "Importing…" : "Import"}</span>
          </div>
          {result && result.lines.map((l, i) => (
            <div key={i} className="res" style={{ color: result.ok && i === 0 ? "var(--win)" : result.ok ? "var(--muted-2)" : "var(--loss-soft)" }}>{l}</div>
          ))}
        </div>
      </div>
    </div>
  );
}
