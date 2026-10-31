import type { CSSProperties } from "react";
import { mapImage } from "../api";
import type { CastSeries } from "../castApi";
import { useSlideTick } from "./useSlideTick";
import { BREAKDOWN_TABS, badgeGameNumber, displayName, mapCardView, seriesScore, shownGame, slideIndex, startableMaps, stripLayout } from "../castState";
import { PostGameSlide } from "./PostGameSlide";

/** Slide 0 : en-tête (équipes + score de série) et bandeau des cartes du pool. */
function MapStrip({ s }: { s: CastSeries }) {
  const [sb, sr] = seriesScore(s);
  const names = { blue: displayName(s.blue, "blue"), red: displayName(s.red, "red") };
  const next = startableMaps(s)[0] ?? null;
  // ≤ 6 cartes : une rangée ; 7–10 : deux rangées de cinq ; 11+ : trois rangées (cartes plus basses)
  const { cols, rows } = stripLayout(s.pool.length);
  const rowsClass = rows === 2 ? " two-rows" : rows >= 3 ? " three-rows" : "";
  const stripStyle = { "--n": cols } as CSSProperties;
  return (
    <>
      <div className="sr-head">
        <div className="sr-ttl">BEST OF {s.bo} · GAME {badgeGameNumber(s)}</div>
        <div className="sr-teams">
          <span className="b">{names.blue}</span>
          <span className="s">{sb} – {sr}</span>
          <span className="r">{names.red}</span>
        </div>
      </div>
      <div className={`sr-strip${rowsClass}`} style={stripStyle} data-rows={rows}>
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

/** Scène « Series » plein cadre (R15) : slide 0 = bandeau des cartes ; slides 1..4 = post-game
 *  breakdown (onglets dégâts / soins / siège / takedowns) de la partie montrée, s'il y a un résumé. */
export function SeriesScene({ s }: { s: CastSeries }) {
  const idx = slideIndex(s, useSlideTick(s));
  const g = shownGame(s);
  const body = idx > 0 && g?.summary
    ? <PostGameSlide s={s} game={g} summary={g.summary} tab={BREAKDOWN_TABS[idx - 1]} />
    : <MapStrip s={s} />;
  return <div className="sr-frame" key={idx}>{body}</div>;
}
