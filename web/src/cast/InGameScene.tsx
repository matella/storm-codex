import type { CSSProperties } from "react";
import type { CastSeries } from "../castApi";
import { badgeGameNumber, displayName, seriesScore } from "../castState";
import { INGAME_ANCHORS, tabBox, type Anchors } from "./anchors";

/** Scène « In-game » : deux onglets rectangulaires ancrés sur le HUD du jeu + pastille de série en haut à droite. */
export function InGameScene({ s, anchors = INGAME_ANCHORS }: { s: CastSeries; anchors?: Anchors }) {
  const [sb, sr] = seriesScore(s);
  const bStyle: CSSProperties = tabBox("blue", anchors);
  const rStyle: CSSProperties = tabBox("red", anchors);
  return (
    <>
      <div className="tab b" style={bStyle}>
        <span className="n">{displayName(s.blue, "blue")}</span>
        <span className="s">{sb}</span>
      </div>
      <div className="tab r" style={rStyle}>
        <span className="s">{sr}</span>
        <span className="n">{displayName(s.red, "red")}</span>
      </div>
      <div className="badge">
        <div className="bo">BO{s.bo}<small>GAME {badgeGameNumber(s)}</small></div>
        <div className="fl">{s.format.toUpperCase()}</div>
      </div>
    </>
  );
}
