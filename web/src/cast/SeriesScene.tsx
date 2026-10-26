import { useEffect, useState } from "react";
import type { CSSProperties } from "react";
import { mapImage } from "../api";
import type { CastSeries } from "../castApi";
import { BREAKDOWN_TABS, badgeGameNumber, displayName, mapCardView, seriesScore, shownGame, slideCount, slideIndex, startableMaps } from "../castState";
import { PostGameSlide } from "./PostGameSlide";

/** Au-delà de 6 cartes le bandeau passe sur deux rangées de cinq (10 cartes max) pour tenir dans le cadre 1920×1080. */
const ONE_ROW_MAX = 6;

/** Slide 0 : en-tête (équipes + score de série) et bandeau des cartes du pool. */
function MapStrip({ s }: { s: CastSeries }) {
  const [sb, sr] = seriesScore(s);
  const names = { blue: displayName(s.blue, "blue"), red: displayName(s.red, "red") };
  const next = startableMaps(s)[0] ?? null;
  const two = s.pool.length > ONE_ROW_MAX;
  const stripStyle = { "--n": Math.max(1, s.pool.length) } as CSSProperties;
  return (
    <>
      <div className="sr-head">
        <div className="sr-ttl">BEST OF {s.bo} · GAME {badgeGameNumber(s.games)}</div>
        <div className="sr-teams">
          <span className="b">{names.blue}</span>
          <span className="s">{sb} – {sr}</span>
          <span className="r">{names.red}</span>
        </div>
      </div>
      <div className={`sr-strip${two ? " two-rows" : ""}`} style={stripStyle}>
        {s.pool.map((m) => {
          const v = mapCardView(m, s.games, next, names);
          const img = mapImage(m.name);
          return (
            <article key={m.name} className={`mc tone-${v.tone}${v.struck ? " struck" : ""}`}>
              <div className="art" style={img ? { backgroundImage: `url(${img})` } : undefined} />
              <div className="tx">
                <b>{m.name}</b>
                {v.label !== "" && <i className="lb">{v.label}</i>}
              </div>
            </article>
          );
        })}
      </div>
    </>
  );
}

/** Minuterie de l'overlay (mode auto) : avance d'un cran toutes les `interval_ms`, repart de 0 à
 *  chaque changement des réglages du diaporama ou du nombre de slides. */
function useTick(s: CastSeries, n: number): number {
  const [tick, setTick] = useState(0);
  const { auto, interval_ms, index, game } = s.slide;
  useEffect(() => {
    setTick(0);
    if (!auto || n <= 1) return;
    const id = window.setInterval(() => setTick((t) => t + 1), Math.max(500, interval_ms));
    return () => window.clearInterval(id);
  }, [auto, interval_ms, index, game, n]);
  return tick;
}

/** Scène « Series » plein cadre (R15) : slide 0 = bandeau des cartes ; slides 1..4 = post-game
 *  breakdown (onglets dégâts / soins / siège / takedowns) de la partie montrée, s'il y a un résumé. */
export function SeriesScene({ s }: { s: CastSeries }) {
  const n = slideCount(s);
  const idx = slideIndex(s, useTick(s, n));
  const g = shownGame(s);
  const body = idx > 0 && g?.summary
    ? <PostGameSlide s={s} game={g} summary={g.summary} tab={BREAKDOWN_TABS[idx - 1]} />
    : <MapStrip s={s} />;
  return <div className="sr-frame" key={idx}>{body}</div>;
}
