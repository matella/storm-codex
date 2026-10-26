// Régie de cast (/cast) — disposition B : rail série à gauche, barre de scènes, aperçu OBS, espace de travail.
// Spec : docs/specs/2026-10-25-cast-control-room-design.md. Les erreurs serveur (409 : NotInPool,
// SeriesInactive, MapAlreadyPlayed…) sont affichées telles quelles à l'opérateur.
import { useState } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { castScene, castScore, castSlide, useCast } from "../castApi";
import type { CastScene, CastSeries } from "../castApi";
import { badgeGameNumber, seriesScore } from "../castState";
import { PoolPanel } from "../cast/PoolPanel";
import { ScenePreview } from "../cast/ScenePreview";
import { SeriesRail } from "../cast/SeriesRail";
import "../cast/cast.css";

const SCENES: { id: CastScene; label: string }[] = [
  { id: "lineups", label: "Lineups" },
  { id: "series", label: "Series" },
  { id: "draft", label: "Draft" },
  { id: "ingame", label: "In-game" },
  { id: "hide", label: "Hide" },
];
const sceneLabel = (s: CastScene) => SCENES.find((x) => x.id === s)?.label ?? s;

type Run = (fn: () => Promise<unknown>) => Promise<void>;

/** Scène Series : jeu dont les données sont montrées, slide courante, défilement auto/manuel (contenu des slides : tâche 9). */
function SeriesWorkspace({ series, run }: { series: CastSeries; run: Run }) {
  const sl = series.slide;
  const shown = sl.game ?? series.games[series.games.length - 1]?.number ?? null;
  return (
    <section className="cr-box">
      <div className="cr-k">Series · slides</div>
      <div className="cr-steppers">
        <label className="cr-note">Game shown{" "}
          <select className="ds-input" aria-label="Game shown" value={shown ?? ""} disabled={series.games.length === 0}
            onChange={(e) => run(() => castSlide({ game: Number(e.target.value) }))}>
            {series.games.length === 0 && <option value="">No game yet</option>}
            {series.games.map((g) => <option key={g.number} value={g.number}>Game {g.number} · {g.map}</option>)}
          </select>
        </label>
        <div className="cr-stepper">
          <button type="button" className="ds-btn small" aria-label="Previous slide" disabled={sl.index <= 0}
            onClick={() => run(() => castSlide({ index: Math.max(0, sl.index - 1) }))}>‹</button>
          <b>{sl.index + 1}</b>
          <button type="button" className="ds-btn small" aria-label="Next slide"
            onClick={() => run(() => castSlide({ index: sl.index + 1 }))}>›</button>
        </div>
        <button type="button" className={`ds-btn small${sl.auto ? " on" : ""}`} aria-pressed={sl.auto}
          onClick={() => run(() => castSlide({ auto: !sl.auto }))}>{sl.auto ? "Auto" : "Manual"}</button>
        <label className="cr-note">Every{" "}
          <input className="ds-input" type="number" min={2} max={120} step={1} aria-label="Slide interval (seconds)"
            style={{ width: 70 }} defaultValue={Math.round(sl.interval_ms / 1000)} key={sl.interval_ms}
            onBlur={(e) => {
              const n = Math.round(Number(e.target.value));
              if (Number.isFinite(n) && n >= 1 && n * 1000 !== sl.interval_ms) run(() => castSlide({ interval_ms: n * 1000 }));
            }} /> s
        </label>
      </div>
      <div className="cr-note">The strip shows every pool map; slide content arrives with the next task.</div>
    </section>
  );
}

function Workspace({ series, run }: { series: CastSeries; run: Run }) {
  const sc = series.scene;
  if (sc === "hide") return null;
  if (sc === "series") return <SeriesWorkspace series={series} run={run} />;
  if (sc !== "ingame") {
    return (
      <section className="cr-box">
        <div className="cr-k">{sceneLabel(sc)}</div>
        <div className="cr-note">Controls for this scene — coming in a later task.</div>
      </section>
    );
  }
  const [b, r] = seriesScore(series);
  const set = (nb: number, nr: number) => run(() => castScore({ blue: Math.max(0, nb), red: Math.max(0, nr) }));
  return (
    <section className="cr-box">
      <div className="cr-k">In-game · score</div>
      <div className="cr-steppers">
        {([["blue", b], ["red", r]] as const).map(([side, v]) => (
          <div key={side} className={`cr-stepper tone-${side}`}>
            <button type="button" className="ds-btn small" aria-label={`${side} score minus`}
              onClick={() => set(side === "blue" ? b - 1 : b, side === "red" ? r - 1 : r)}>−</button>
            <b>{v}</b>
            <button type="button" className="ds-btn small" aria-label={`${side} score plus`}
              onClick={() => set(side === "blue" ? b + 1 : b, side === "red" ? r + 1 : r)}>+</button>
          </div>
        ))}
        {series.score_override && (
          <button type="button" className="ds-btn small" onClick={() => run(() => castScore(null))}>Back to computed score</button>
        )}
      </div>
      <div className="cr-note">
        Badge shows: BO {series.bo} · GAME {badgeGameNumber(series.games)} · {series.format.toUpperCase()}
        {series.score_override ? " · score forced by operator" : " · score from game winners"}
      </div>
    </section>
  );
}

export function Cast() {
  const { data: series, isLoading, error: loadError } = useCast();
  const qc = useQueryClient();
  const [err, setErr] = useState<string | null>(null);

  /** Exécute une action serveur ; une réponse `CastSeries` alimente le cache, toute erreur est affichée. */
  const run: Run = async (fn) => {
    setErr(null);
    try {
      const res = await fn();
      if (res && typeof res === "object" && "pool" in res) qc.setQueryData(["cast"], res);
      else await qc.invalidateQueries({ queryKey: ["cast"] });
    } catch (e) {
      setErr(e instanceof Error ? e.message : String(e));
      await qc.invalidateQueries({ queryKey: ["cast"] });
    }
  };

  if (isLoading) return <div className="empty">loading…</div>;
  if (!series) return <div className="empty">cast state unavailable{loadError ? `: ${(loadError as Error).message}` : ""}</div>;

  return (
    <div className="cr">
      <div className="cr-top">
        <span className="cr-br">CAST <b>CONTROL ROOM</b></span>
        <div className="cr-scenes" role="tablist" aria-label="Scene">
          {SCENES.map((s) => (
            <button key={s.id} type="button" role="tab" aria-selected={series.scene === s.id}
              className={`cr-scene${series.scene === s.id ? " on" : ""}`}
              onClick={() => run(() => castScene(s.id))}>
              {s.label}{series.scene === s.id && <i className="cr-air">ON AIR</i>}
            </button>
          ))}
        </div>
        {!series.active && <span className="cr-inactive">no active series</span>}
      </div>
      {err && (
        <div className="cr-err" role="alert">
          Server refused: <b>{err}</b>
          <button type="button" className="ds-btn small" onClick={() => setErr(null)}>dismiss</button>
        </div>
      )}
      <div className="cr-body">
        <SeriesRail series={series} run={run} />
        <div className="cr-main">
          <ScenePreview onAir={sceneLabel(series.scene)} />
          <Workspace series={series} run={run} />
          <PoolPanel series={series} run={run} />
        </div>
      </div>
    </div>
  );
}
