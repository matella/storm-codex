// Pool de cartes de la série : ban / pick / decider / reset par carte, lancement d'une partie, vainqueur.
import { useState } from "react";
import { mapImage } from "../api";
import type { Side } from "../api";
import { castMarkMap, castRemoveGame, castStartGame, castWinner } from "../castApi";
import type { CastSeries, MapMark, PoolMap } from "../castApi";
import { canChangeWinner, canRemoveGame, canSetWinner, mapCardView, startableMaps } from "../castState";

type Run = (fn: () => Promise<unknown>) => Promise<void>;

const MARKS: { label: string; mark: MapMark; tone: string }[] = [
  { label: "Ban Blue", mark: { status: "banned", by: "blue" }, tone: "blue" },
  { label: "Ban Red", mark: { status: "banned", by: "red" }, tone: "red" },
  { label: "Pick Blue", mark: { status: "picked", by: "blue" }, tone: "blue" },
  { label: "Pick Red", mark: { status: "picked", by: "red" }, tone: "red" },
  { label: "Decider", mark: { status: "decider" }, tone: "gold" },
  { label: "Reset", mark: { status: "available" }, tone: "" },
];

const isActive = (m: PoolMap, mark: MapMark) =>
  m.status === mark.status && ("by" in m ? m.by : undefined) === ("by" in mark ? mark.by : undefined);

export function PoolPanel({ series, run }: { series: CastSeries; run: Run }) {
  const startable = startableMaps(series);
  const [pick, setPick] = useState("");
  const chosen = startable.includes(pick) ? pick : (startable[0] ?? "");
  const next = startable[0] ?? null;
  return (
    <section className="cr-box cr-pool">
      <div className="cr-k">Maps</div>
      {series.pool.length === 0 && <div className="cr-note">Empty pool — start a series with at least one map.</div>}
      <div className="cr-mapgrid">
        {series.pool.map((m) => {
          const v = mapCardView(m, series.games, next);
          const played = series.games.some((g) => g.map === m.name);
          const img = mapImage(m.name);
          return (
            <div key={m.name} className={`cr-map tone-${v.tone}${v.struck ? " struck" : ""}`}>
              <div className="cr-map-art" style={img ? { backgroundImage: `url(${img})` } : undefined} />
              <div className="cr-map-h"><b>{m.name}</b><em>{v.label}</em></div>
              <div className="cr-map-btns">
                {MARKS.map((x) => (
                  <button
                    key={x.label}
                    type="button"
                    className={`ds-btn small ${x.tone}${isActive(m, x.mark) ? " on" : ""}`}
                    aria-label={`${x.label} ${m.name}`}
                    aria-pressed={x.mark.status === "available" ? undefined : isActive(m, x.mark)}
                    disabled={played}
                    onClick={() => run(() => castMarkMap(m.name, x.mark))}
                  >{x.label}</button>
                ))}
              </div>
            </div>
          );
        })}
      </div>
      <div className="cr-start">
        <select className="ds-input" value={chosen} onChange={(e) => setPick(e.target.value)} disabled={startable.length === 0} aria-label="Map to start">
          {startable.length === 0 && <option value="">No picked / decider map left</option>}
          {startable.map((n) => <option key={n}>{n}</option>)}
        </select>
        <button type="button" className="ds-btn primary" disabled={!chosen} onClick={() => run(() => castStartGame(chosen))}>Start game</button>
      </div>
      {series.games.length > 0 && (
        <div className="cr-games">
          {series.games.map((g) => (
            <div key={g.number} className="cr-game">
              <span className="cr-gn">Game {g.number}</span>
              <span className="cr-gm">{g.map}</span>
              {g.winner
                ? <>
                    <span className={`cr-win tone-${g.winner}`}>{g.winner === "blue" ? "Blue" : "Red"} won</span>
                    {canChangeWinner(series, g.number) && (
                      <button
                        type="button"
                        className="ds-btn small"
                        aria-label={`Game ${g.number}: change winner to ${g.winner === "blue" ? "Red" : "Blue"}`}
                        title="Misclick? Give the win to the other side (not possible once a replay is loaded)"
                        onClick={() => run(() => castWinner(g.number, { side: g.winner === "blue" ? "red" : "blue" }))}
                      >Change winner</button>
                    )}
                  </>
                : (["blue", "red"] as Side[]).map((side) => (
                    <button
                      key={side}
                      type="button"
                      className={`ds-btn small ${side}`}
                      aria-label={`Game ${g.number}: ${side === "blue" ? "Blue" : "Red"} won`}
                      disabled={!canSetWinner(series, g.number)}
                      onClick={() => run(() => castWinner(g.number, { side }))}
                    >{side === "blue" ? "Blue" : "Red"} won</button>
                  ))}
              {canRemoveGame(series, g.number) && (
                <button
                  type="button"
                  className="ds-btn small"
                  aria-label={`Remove game ${g.number}`}
                  title="Started by mistake? Remove this game; its map becomes startable again"
                  onClick={() => {
                    if (window.confirm(`Remove game ${g.number} (${g.map})?`)) void run(() => castRemoveGame(g.number));
                  }}
                >Remove game</button>
              )}
            </div>
          ))}
        </div>
      )}
    </section>
  );
}
