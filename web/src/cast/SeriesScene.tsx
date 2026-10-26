import type { CSSProperties } from "react";
import { mapImage } from "../api";
import type { CastSeries } from "../castApi";
import { badgeGameNumber, displayName, mapCardView, seriesScore, startableMaps } from "../castState";

/** Au-delà de 6 cartes le bandeau passe sur deux rangées de cinq (10 cartes max) pour tenir dans le cadre 1920×1080. */
const ONE_ROW_MAX = 6;

/** Point d'aiguillage des slides de la scène Series (contenu : tâche 9). Vide pour l'instant. */
export function SlideZone(_props: { game: number | null; index: number }): null {
  return null;
}

/** Scène « Series » : en-tête (équipes + score de série), bandeau des cartes du pool, zone de slides. */
export function SeriesScene({ s }: { s: CastSeries }) {
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
      <div className="sr-slide"><SlideZone game={s.slide.game} index={s.slide.index} /></div>
    </>
  );
}
