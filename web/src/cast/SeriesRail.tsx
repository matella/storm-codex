// Rail gauche de la régie : Série (Bo, format, pool, « New series »), Teams & lineups (nom, logo, bilan,
// 5 joueurs + capitaine). Chaque joueur : pseudo saisi + `PlayerPicker` (recherche → lien toon, sinon « non lié »).
import { useEffect, useRef, useState } from "react";
import type { DraftFormat, Side } from "../api";
import { castLogoUpload, castSeries, castTeams } from "../castApi";
import type { CastSeries, CastTeam } from "../castApi";
import { PlayerPicker } from "./PlayerPicker";
import { MAPS, reconcileTeam, seriesScore, togglePool } from "../castState";

type Run = (fn: () => Promise<unknown>) => Promise<void>;
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
  const dirty = useRef(false);
  const [logoErr, setLogoErr] = useState<Record<Side, boolean>>({ blue: false, red: false });

  // resynchronise depuis le serveur (autre onglet, WS) tant que l'opérateur n'a rien modifié
  useEffect(() => {
    if (dirty.current) return;
    setBlue(series.blue);
    setRed(series.red);
  }, [series.blue, series.red]);

  // un logo envoyé par fichier arrive du serveur : on l'adopte même si la saisie est modifiée
  const logoTouched = useRef<Record<Side, boolean>>({ blue: false, red: false });
  useEffect(() => { setBlue((t) => reconcileTeam(t, series.blue, logoTouched.current.blue)); }, [series.blue.logo]); // eslint-disable-line react-hooks/exhaustive-deps
  useEffect(() => { setRed((t) => reconcileTeam(t, series.red, logoTouched.current.red)); }, [series.red.logo]); // eslint-disable-line react-hooks/exhaustive-deps
  const current = () => ({
    blue: reconcileTeam(blue, series.blue, logoTouched.current.blue),
    red: reconcileTeam(red, series.red, logoTouched.current.red),
  });
  const clean = () => { dirty.current = false; logoTouched.current = { blue: false, red: false }; };

  const edit = (side: Side, t: CastTeam) => {
    dirty.current = true;
    (side === "blue" ? setBlue : setRed)(t);
  };
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
  const upload = (side: Side, file: File | undefined) => {
    if (!file) return;
    setLogoErr((e) => ({ ...e, [side]: false }));
    logoTouched.current[side] = false;
    void run(() => castLogoUpload(side, file));
  };
  const setPlayer = (side: Side, t: CastTeam, i: number, patch: Partial<CastTeam["players"][number]>) =>
    edit(side, { ...t, players: t.players.map((q, j) => (j === i ? { ...q, ...patch } : q)) });

  const [sb, sr] = seriesScore(series);
  const unsaved = !sameTeam(blue, series.blue) || !sameTeam(red, series.red);

  return (
    <aside className="cr-rail">
      <div className="cr-k">Series</div>
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

      <div className="cr-k">Teams &amp; lineups</div>
      {([["blue", blue], ["red", red]] as [Side, CastTeam][]).map(([side, t]) => (
        <fieldset key={side} className={`cr-team tone-${side}`}>
          <legend>{side === "blue" ? "Blue" : "Red"}</legend>
          <input className="ds-input" placeholder={side === "blue" ? "Team Blue" : "Team Red"} value={t.name}
            onChange={(e) => edit(side, { ...t, name: e.target.value })} aria-label={`${side} team name`} />
          <div className="cr-logo">
            {t.logo && !logoErr[side]
              ? <img src={t.logo} alt="" onError={() => setLogoErr((e) => ({ ...e, [side]: true }))} />
              : <span className="cr-nologo">no logo</span>}
            <input type="file" accept="image/png,image/jpeg,image/webp" aria-label={`${side} logo file`}
              onChange={(e) => { upload(side, e.target.files?.[0]); e.target.value = ""; }} />
          </div>
          <input className="ds-input" placeholder="logo URL (optional)" value={t.logo ?? ""} aria-label={`${side} logo URL`}
            onChange={(e) => { logoTouched.current[side] = true; edit(side, { ...t, logo: e.target.value.trim() === "" ? null : e.target.value }); }} />
          <input className="ds-input" placeholder="record (e.g. 4-1)" value={t.record} aria-label={`${side} record`}
            onChange={(e) => edit(side, { ...t, record: e.target.value })} />
          {t.players.map((p, i) => (
            <div key={i} className="cr-player">
              <PlayerPicker player={p} label={`${side} player ${i + 1}`} onChange={(patch) => setPlayer(side, t, i, patch)} />
              <div className="cr-pl2">
                <input className="ds-input" placeholder="tagline" value={p.tagline} aria-label={`${side} player ${i + 1} tagline`}
                  onChange={(e) => setPlayer(side, t, i, { tagline: e.target.value })} />
                <button type="button" className={`ds-btn small${t.captain === i ? " on" : ""}`} title="Captain" aria-pressed={t.captain === i}
                  onClick={() => edit(side, { ...t, captain: t.captain === i ? null : i })}>C</button>
              </div>
            </div>
          ))}
        </fieldset>
      ))}
      <button type="button" className="ds-btn cr-wide" disabled={!unsaved} onClick={saveTeams}>
        {unsaved ? "Save teams" : "Teams saved"}
      </button>
    </aside>
  );
}
