// Rail gauche de la régie : Série (Bo, format, pool, « New series »), Teams & lineups (`TeamEditor` par côté :
// nom, logo, bilan, 5 joueurs + capitaine ; chaque joueur : pseudo saisi + `PlayerPicker`).
import { useEffect, useRef, useState } from "react";
import type { DraftFormat, Side } from "../api";
import { castLogoUpload, castSeries, castTeams } from "../castApi";
import type { CastSeries, CastTeam } from "../castApi";
import { MAPS, nextHint, reconcileTeam, resyncTeam, seriesScore, setupSteps, togglePool } from "../castState";
import { ReplayImport } from "./ReplayImport";
import { TeamEditor } from "./TeamEditor";

type Run = (fn: () => Promise<unknown>) => Promise<void>;
type ReplayMode = "manual" | "batch";
const BOS = [1, 3, 5, 7] as const;
type Bo = (typeof BOS)[number];
const FORMATS: DraftFormat[] = ["fearless", "standard", "normal"];

const sameTeam = (a: CastTeam, b: CastTeam) => JSON.stringify(a) === JSON.stringify(b);

export function SeriesRail({ series, run }: { series: CastSeries; run: Run }) {
  const [blue, setBlue] = useState<CastTeam>(series.blue);
  const [red, setRed] = useState<CastTeam>(series.red);
  const [bo, setBo] = useState<Bo>(BOS.find((n) => n === series.bo) ?? 5);
  const [format, setFormat] = useState<DraftFormat>(series.format);
  const [pool, setPool] = useState<string[]>(series.pool.map((m) => m.name));
  const [replayMode, setReplayMode] = useState<ReplayMode>("manual");

  // dernière version serveur vue : une saisie égale à `base` (rien modifié, ou modification annulée)
  // adopte la nouvelle version du serveur (autre onglet, WS) ; une saisie qui a divergé est conservée
  const base = useRef({ blue: series.blue, red: series.red });
  // un logo envoyé par fichier arrive du serveur : on l'adopte même si la saisie est modifiée
  const logoTouched = useRef<Record<Side, boolean>>({ blue: false, red: false });
  useEffect(() => {
    const old = base.current;
    base.current = { blue: series.blue, red: series.red };
    setBlue((t) => reconcileTeam(resyncTeam(t, old.blue, series.blue), series.blue, logoTouched.current.blue));
    setRed((t) => reconcileTeam(resyncTeam(t, old.red, series.red), series.red, logoTouched.current.red));
  }, [series.blue, series.red]);

  const current = () => ({
    blue: reconcileTeam(blue, series.blue, logoTouched.current.blue),
    red: reconcileTeam(red, series.red, logoTouched.current.red),
  });
  const clean = () => { logoTouched.current = { blue: false, red: false }; };

  const edit = (side: Side, t: CastTeam) => (side === "blue" ? setBlue : setRed)(t);
  const saveTeams = () =>
    run(async () => {
      const c = current();
      const s = await castTeams(c.blue, c.red);
      clean();
      return s;
    });
  const newSeries = () =>
    run(async () => {
      if (series.active && !window.confirm("Replace the running series? Games and score are reset.")) return null;
      const c = current();
      // un seul appel : équipes complètes + série (pas d'état intermédiaire à l'antenne)
      const s = await castSeries({ blue: c.blue, red: c.red, bo, format, pool });
      clean();
      return s;
    });
  const upload = (side: Side, file: File) => {
    logoTouched.current[side] = false;
    void run(() => castLogoUpload(side, file));
  };

  const [sb, sr] = seriesScore(series);
  const unsaved = !sameTeam(blue, series.blue) || !sameTeam(red, series.red);

  return (
    <aside className="cr-rail">
      <div className="cr-k">Series</div>
      <ol className="cr-steps" aria-label="Setup steps">
        {setupSteps(series).map((st, i) => (
          <li key={st.id} className={st.done ? "done" : ""}>{i + 1}. {st.label}{st.done ? " ✓" : ""}</li>
        ))}
      </ol>
      {nextHint(series) && <div className="cr-note cr-hint">{nextHint(series)}</div>}
      <div className="cr-score">
        <span className="tone-blue">{blue.name.trim() || "Blue"}</span><b>{sb}</b>
        <b>{sr}</b><span className="tone-red">{red.name.trim() || "Red"}</span>
      </div>
      <div className="cr-row">
        <label>Best of
          <select className="ds-input" value={bo} onChange={(e) => setBo(Number(e.target.value) as Bo)}>
            {BOS.map((n) => <option key={n} value={n}>Bo{n}</option>)}
          </select>
        </label>
        <label>Format
          <select className="ds-input" value={format} onChange={(e) => setFormat(e.target.value as DraftFormat)}>
            {FORMATS.map((f) => <option key={f}>{f}</option>)}
          </select>
        </label>
      </div>
      <div className="cr-poolpick" role="group" aria-label="Map pool">
        {MAPS.map((m) => (
          <label key={m} className={pool.includes(m) ? "on" : ""}>
            <input type="checkbox" checked={pool.includes(m)} onChange={() => setPool((p) => togglePool(p, m))} />{m}
          </label>
        ))}
      </div>
      <button type="button" className="ds-btn primary cr-wide" disabled={pool.length === 0} onClick={newSeries}>
        New series ({pool.length} maps)
      </button>
      {pool.length === 0 && <div className="cr-note">Tick at least one map to create a series.</div>}
      {unsaved && <div className="cr-note">Unsaved team changes will be sent with the new series.</div>}
      <label className="cr-note">Replays{" "}
        <select className="ds-input" aria-label="Replay loading" value={replayMode} onChange={(e) => setReplayMode(e.target.value as ReplayMode)}>
          <option value="manual">Per game (one file each)</option>
          <option value="batch">Batch import (all the series' files)</option>
        </select>
      </label>
      {replayMode === "batch" && (series.active
        ? <ReplayImport series={series} run={run} />
        : <div className="cr-note">Create the series first, then add its replays here.</div>)}

      <div className="cr-k">Teams &amp; lineups</div>
      {([["blue", blue], ["red", red]] as [Side, CastTeam][]).map(([side, t]) => (
        <TeamEditor key={side} side={side} team={t} onEdit={(nt) => edit(side, nt)} onUpload={(f) => upload(side, f)}
          onLogoTyped={() => { logoTouched.current[side] = true; }} />
      ))}
      <button type="button" className="ds-btn cr-wide" disabled={!unsaved} onClick={saveTeams}>
        {unsaved ? "Save teams" : "Teams saved"}
      </button>
    </aside>
  );
}
