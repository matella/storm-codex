// Régie de cast (/cast) — disposition B : rail série à gauche, barre de scènes, aperçu OBS, espace de travail.
// Spec : docs/specs/2026-10-25-cast-control-room-design.md. Les erreurs serveur (409 : NotInPool,
// SeriesInactive, MapAlreadyPlayed…) sont affichées telles quelles à l'opérateur.
import { useRef, useState } from "react";
import { useLocation } from "react-router-dom";
import { useQueryClient } from "@tanstack/react-query";
import { castScene, castScore, useCast } from "../castApi";
import type { CastScene, CastSeries } from "../castApi";
import { badgeGameNumber, friendlyError, sceneFromSearch, seriesScore, tabKeyTarget } from "../castState";
import { ObsSource } from "../cast/ObsSource";
import { DraftWorkspace } from "../cast/DraftWorkspace";
import { LineupsWorkspace } from "../cast/LineupsWorkspace";
import { PoolPanel } from "../cast/PoolPanel";
import { ScenePreview } from "../cast/ScenePreview";
import { SeriesRail } from "../cast/SeriesRail";
import { SeriesWorkspace } from "../cast/SeriesWorkspace";
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

/** `scene` = onglet de travail LOCAL (≠ scène à l'antenne `series.scene`). */
function Workspace({ series, scene: sc, run }: { series: CastSeries; scene: CastScene; run: Run }) {
  if (sc === "hide") return null;
  if (sc === "series") return <SeriesWorkspace series={series} run={run} />;
  if (sc === "lineups") return <LineupsWorkspace series={series} run={run} />;
  if (sc === "draft") return <DraftWorkspace series={series} />;
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
        On-screen badge: BO {series.bo} · GAME {badgeGameNumber(series)} · {series.format.toUpperCase()}
        {series.score_override ? " · score set by you" : " · score from game winners"}
      </div>
    </section>
  );
}

/** Barre de scènes (motif onglets, activation manuelle) : `aria-selected` = onglet de travail local ;
 *  la scène à l'antenne est dite en texte (« ON AIR »). Clic / Entrée / Espace = sélectionne l'onglet de travail
 *  seulement ; la diffusion passe par le bouton « Put … on air » (R26). Les flèches, Début et Fin ne font que déplacer le focus. */
export function SceneTabs({ onAir, local, onPick }: { onAir: CastScene; local: CastScene; onPick: (s: CastScene) => void }) {
  const refs = useRef<(HTMLButtonElement | null)[]>([]);
  return (
    <div className="cr-scenes" role="tablist" aria-label="Scene (selects the working tab; the button puts it on air)">
      {SCENES.map((s, i) => (
        <button key={s.id} type="button" role="tab" id={`cr-tab-${s.id}`} aria-controls="cr-workspace"
          aria-selected={local === s.id} tabIndex={local === s.id ? 0 : -1}
          ref={(el) => { refs.current[i] = el; }}
          className={`cr-scene${onAir === s.id ? " on" : ""}${local === s.id && local !== onAir ? " ed" : ""}`}
          onKeyDown={(e) => {
            const t = tabKeyTarget(i, e.key, SCENES.length);
            if (t === null) return;
            e.preventDefault();
            refs.current[t]?.focus();
          }}
          onClick={() => onPick(s.id)}>
          {s.label}{onAir === s.id && <i className="cr-air">ON AIR</i>}
        </button>
      ))}
    </div>
  );
}

export function Cast() {
  const { data: series, isLoading, error: loadError } = useCast();
  const qc = useQueryClient();
  const [err, setErr] = useState<string | null>(null);
  // onglet de travail local : `?scene=` à l'arrivée (redirection de /draft), sans toucher à la scène à l'antenne ;
  // null = suit la scène à l'antenne. Choisir un onglet ne diffuse rien ; le bouton « Put … on air » diffuse.
  const loc = useLocation();
  const [local, setLocal] = useState<CastScene | null>(() => sceneFromSearch(loc.search));

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
        <SceneTabs onAir={series.scene} local={local ?? series.scene}
          onPick={(sc) => setLocal(sc === series.scene ? null : sc)} />
        {local && local !== series.scene && (
          <button type="button" className="ds-btn primary small" onClick={() => { const next = local; void run(async () => { const r = await castScene(next); setLocal(null); return r; }); }}>
            Put {sceneLabel(local)} on air
          </button>
        )}
        {!series.active && <span className="cr-inactive">no active series</span>}
      </div>
      <ObsSource />
      {err && (
        <div className="cr-err" role="alert">
          {friendlyError(err)}{friendlyError(err) !== err && <span className="cr-note"> ({err})</span>}
          <button type="button" className="ds-btn small" onClick={() => setErr(null)}>dismiss</button>
        </div>
      )}
      <div className="cr-body">
        <SeriesRail series={series} run={run} />
        <div className="cr-main">
          <ScenePreview onAir={sceneLabel(series.scene)} />
          <div role="tabpanel" id="cr-workspace" aria-labelledby={`cr-tab-${local ?? series.scene}`}>
            <Workspace series={series} scene={local ?? series.scene} run={run} />
          </div>
          <PoolPanel series={series} run={run} />
        </div>
      </div>
    </div>
  );
}
