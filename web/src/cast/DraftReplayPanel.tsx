import { useEffect, useMemo, useRef, useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { Link } from "react-router-dom";
import {
  useDraft, sideOfStep, fetchDraftReplay,
  draftReplayLoad, draftReplayNext, draftReplayBack, draftReplayReset, draftReplaySelect,
  draftReplayFearless, draftReplayPlay, draftReplayMove, draftReplayRemove,
  type DraftState,
} from "../api";
import { currentPhase, loadErrorMessage, phaseLabel, phasesOf, secondsLeft } from "../draftReplay";

/** Télécommande du draft depuis un replay (corps extrait de `pages/DraftReplay.tsx`, monté dans la régie). Le public voit l'overlay OBS `/draft/overlay` ; cette page
 *  charge des replays Custom, prépare leur draft caché, puis le révèle phase par phase (manuel ou auto). */
export function DraftReplayPanel() {
  const { data: d } = useDraft();
  const qc = useQueryClient();
  const { data: rs } = useQuery({ queryKey: ["draft", "replay"], queryFn: fetchDraftReplay });
  const [err, setErr] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [secs, setSecs] = useState(30);
  const [now, setNow] = useState(() => Date.now());
  const fileRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    const id = setInterval(() => setNow(Date.now()), 500);
    return () => clearInterval(id);
  }, []);
  useEffect(() => { if (rs) setSecs(Math.round(rs.interval_ms / 1000)); }, [rs?.interval_ms]); // eslint-disable-line react-hooks/exhaustive-deps

  const run = async (f: () => Promise<unknown>) => {
    setErr(null);
    try { await f(); } catch (e) { setErr(e instanceof Error ? e.message : String(e)); }
    qc.invalidateQueries({ queryKey: ["draft"] });
  };

  const upload = async (files: FileList | File[]) => {
    setBusy(true);
    setErr(null);
    for (const f of Array.from(files)) {
      try { await draftReplayLoad(f); }
      catch (e) { setErr(`${f.name} — ${loadErrorMessage(e instanceof Error ? e.message : undefined)}`); }
    }
    setBusy(false);
    qc.invalidateQueries({ queryKey: ["draft"] });
  };

  const cur = rs?.current ?? null;
  const ready = d != null && cur != null;
  const complete = d != null && d.cursor >= d.steps.length;
  const playing = rs?.playing ?? false;

  // raccourcis : Espace = Suivant, Retour arrière = Retour (hors champs de saisie)
  const keys = useRef({ ready, complete });
  keys.current = { ready, complete };
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const t = e.target as HTMLElement | null;
      if (t && (t.tagName === "INPUT" || t.tagName === "SELECT" || t.tagName === "TEXTAREA")) return;
      if (!keys.current.ready) return;
      if (e.code === "Space") { e.preventDefault(); if (!keys.current.complete) run(draftReplayNext); }
      else if (e.code === "Backspace") { e.preventDefault(); run(draftReplayBack); }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }); // eslint-disable-line react-hooks/exhaustive-deps

  return (
    <div className="drp ds-page">
      <style>{CSS}</style>
      <header className="ds-cover">
        <div className="ds-kicker">Draft simulator · replay mode</div>
        <h1 className="ds-title" style={{ cursor: "default", fontSize: "clamp(34px, 5vw, 60px)" }}>Draft from a replay</h1>
        <div className="ds-subtitle" style={{ cursor: "default" }}>
          Load custom-game replays, then reveal the draft step by step on the OBS overlay{" "}
          <Link to="/draft/overlay" target="_blank" className="olink">/draft/overlay ↗</Link>
        </div>
      </header>

      {err && <div className="err">{err}</div>}

      <div className="grid2">
        <section className="card">
          <div className="lab">Replays · custom games only</div>
          <div
            className="drop"
            onDragOver={(e) => e.preventDefault()}
            onDrop={(e) => { e.preventDefault(); if (e.dataTransfer.files.length) upload(e.dataTransfer.files); }}
            onClick={() => fileRef.current?.click()}
          >
            <b>{busy ? "reading…" : "Drop one or more .StormReplay files"}</b>
            <small>added to the queue in order · nothing is saved to the match database</small>
            <input ref={fileRef} type="file" accept=".StormReplay" multiple hidden
              onChange={(e) => { if (e.target.files?.length) upload(e.target.files); e.target.value = ""; }} />
          </div>
        </section>

        <section className="card">
          <div className="lab">Queue · series ({rs?.queue.length ?? 0})</div>
          {rs?.queue.length === 0 && <div className="hint">no replay loaded</div>}
          {rs?.queue.map((g, i) => (
            <div key={i} className={`q ${cur === i ? "cur" : ""}`}>
              <button className="n" title="prepare this game" onClick={() => run(() => draftReplaySelect(i))}>{i + 1}</button>
              <div className="t"><b>{g.map || "—"}</b><span>{g.source}{cur === i ? ` · phase ${progress(d)}` : ""}</span></div>
              <button className="x" disabled={i === 0} title="move up" onClick={() => run(() => draftReplayMove(i, i - 1))}>↑</button>
              <button className="x" disabled={i === rs.queue.length - 1} title="move down" onClick={() => run(() => draftReplayMove(i, i + 1))}>↓</button>
              <button className="x" title="remove" onClick={() => run(() => draftReplayRemove(i))}>✕</button>
            </div>
          ))}
          <label className="tog">
            <input type="checkbox" checked={rs?.fearless ?? false} onChange={(e) => run(() => draftReplayFearless(e.target.checked))} />
            Fearless — picks of earlier games are unavailable
            {d && d.series_bans.length > 0 && <span className="hint"> ({d.series_bans.length} heroes)</span>}
          </label>
        </section>
      </div>

      {ready && d && <PhaseRail d={d} />}

      <section className="card ctl">
        <button className="ds-btn" disabled={!ready || d.cursor === 0} onClick={() => run(draftReplayBack)}>↶ Back <kbd>⌫</kbd></button>
        <button className="ds-btn primary big" disabled={!ready || complete} onClick={() => run(draftReplayNext)}>Next <kbd>Space</kbd></button>
        <button className="ds-btn" disabled={!ready} onClick={() => run(draftReplayReset)}>⟲ Reset</button>
        <span className="sep" />
        {playing
          ? <button className="ds-btn big" onClick={() => run(() => draftReplayPlay(false))}>❚❚ Pause</button>
          : <button className="ds-btn big go" disabled={!ready || complete} onClick={() => run(() => draftReplayPlay(true, Math.max(1, secs) * 1000))}>▶ Auto</button>}
        <span className="hint">every</span>
        <input className="dur" type="number" min={1} max={3600} value={secs} disabled={playing}
          onChange={(e) => setSecs(Number(e.target.value))} />
        <span className="hint">s per phase</span>
        {playing && <span className="cd">next phase in {secondsLeft(rs?.next_at_ms, now)}s</span>}
        {ready && complete && <span className="cd done">draft complete</span>}
      </section>
    </div>
  );
}

function progress(d: DraftState | undefined): string {
  if (!d) return "";
  const phases = phasesOf(d.steps);
  const c = currentPhase(phases, d.cursor);
  return c < 0 ? `${phases.length} / ${phases.length}` : `${c} / ${phases.length}`;
}

/** Rail des 12 phases : les héros ne s'affichent qu'une fois révélés (vue publique de l'état). */
function PhaseRail({ d }: { d: DraftState }) {
  const phases = useMemo(() => phasesOf(d.steps), [d.steps]);
  const cur = currentPhase(phases, d.cursor);
  return (
    <section className="card">
      <div className="lab">Draft — {phases.length} phases · unrevealed heroes stay hidden</div>
      <div className="rail" style={{ gridTemplateColumns: `repeat(${phases.length}, minmax(0, 1fr))` }}>
        {phases.map((p, i) => {
          const side = sideOfStep(d, d.steps[p.start]);
          const done = d.cursor >= p.start + p.len;
          const heroes = d.assignments.slice(p.start, p.start + p.len);
          return (
            <div key={i} className={`ph ${side} ${done ? "done" : ""} ${i === cur ? "cur" : ""}`}>
              <b>{i + 1} · {phaseLabel(p)}</b>
              {done
                ? heroes.map((h, k) => <span key={k} className="hh">{h ?? "no ban"}</span>)
                : <span className="hh hid">••••</span>}
            </div>
          );
        })}
      </div>
    </section>
  );
}

const CSS = `
.drp .err{background:rgba(224,65,75,.12);border:1px solid rgba(224,65,75,.5);color:var(--loss-soft,#ff9aa4);border-radius:9px;padding:9px 12px;margin:12px 0;font-size:13px}
.drp .grid2{display:grid;grid-template-columns:repeat(auto-fit,minmax(340px,1fr));gap:14px;margin:14px 0}
.drp .card{background:var(--panel,#12151d);border:1px solid var(--line,#222838);border-radius:12px;padding:12px 14px;margin-bottom:14px}
.drp .grid2 .card{margin-bottom:0}
.drp .lab{font:600 10.5px var(--mono);letter-spacing:.12em;text-transform:uppercase;color:var(--text-3,#7c8398);margin-bottom:8px}
.drp .hint{color:var(--text-3,#7c8398);font-size:12px}
.drp .drop{border:2px dashed var(--line-2,#34406a);border-radius:12px;padding:22px;text-align:center;cursor:pointer}
.drp .drop b{display:block;color:var(--text,#cdd4e6);margin-bottom:3px}
.drp .drop small{color:var(--text-3,#6f7794)}
.drp .q{display:flex;align-items:center;gap:8px;padding:7px 9px;border-radius:9px;background:var(--panel-2,#0f121a);border:1px solid var(--line,#1e2433);margin-bottom:6px}
.drp .q.cur{border-color:var(--accent,#7f77dd);box-shadow:0 0 0 1px var(--accent,#7f77dd)}
.drp .q .n{width:26px;height:26px;border-radius:50%;border:none;background:var(--line,#1b2030);color:var(--text,#fff);font-weight:800;cursor:pointer}
.drp .q .t{flex:1;min-width:0}
.drp .q .t b{display:block;font-size:13px}
.drp .q .t span{font-size:11.5px;color:var(--text-3,#8b94ad);word-break:break-all}
.drp .q .x{background:none;border:none;color:var(--text-3,#7c8398);cursor:pointer;font-size:15px}
.drp .q .x:disabled{opacity:.3;cursor:default}
.drp .tog{display:flex;align-items:center;gap:8px;font-size:13px;margin-top:10px}
.drp .rail{display:grid;gap:6px}
.drp .ph{border-radius:9px;padding:7px 4px;text-align:center;background:var(--panel-2,#0f121a);border:1px solid var(--line,#1e2433);font-size:11px;color:var(--text-3,#8b94ad);min-width:0}
.drp .ph b{display:block;font-size:11.5px;color:var(--text-2,#cdd4e6);margin-bottom:2px}
.drp .ph.blue{border-top:3px solid #2f6df6}
.drp .ph.red{border-top:3px solid #e0414b}
.drp .ph.done{background:rgba(127,119,221,.1)}
.drp .ph.cur{border-color:#e8c66a;box-shadow:0 0 0 1px #e8c66a}
.drp .ph .hh{display:block;margin-top:3px;font-size:10.5px;color:var(--text,#e7ebf5);white-space:nowrap;overflow:hidden;text-overflow:ellipsis}
.drp .ph .hid{color:var(--text-3,#4b5470);letter-spacing:.2em}
.drp .ctl{display:flex;align-items:center;gap:10px;flex-wrap:wrap}
.drp .ds-btn.big{padding:11px 22px;font-size:13px}
.drp .ds-btn.go{background:#1f9d62;border-color:#1f9d62;color:#fff}
.drp .ds-btn:disabled{opacity:.45;cursor:default}
.drp kbd{background:transparent;border:1px solid currentColor;opacity:.6;border-radius:5px;padding:1px 6px;font-size:10px;margin-left:6px}
.drp .sep{width:1px;height:26px;background:var(--line,#2a3142);margin:0 6px}
.drp .dur{width:60px;background:var(--panel-2,#10131c);border:1px solid var(--line-2,#2a3142);border-radius:8px;padding:8px;color:var(--text,#e7ebf5);text-align:center;font:inherit}
.drp .cd{margin-left:auto;color:#e8c66a;font-weight:800;font-variant-numeric:tabular-nums}
.drp .cd.done{color:var(--text-3,#9aa3bd)}
`;
