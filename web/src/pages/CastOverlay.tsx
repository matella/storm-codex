import { useEffect } from "react";
import { useDimHeroes, useLiveUpdates } from "../api";
import { useCast } from "../castApi";
import type { CastSeries } from "../castApi";
import { overlayVisible } from "../castState";
import { DraftSceneLive, skinFromSearch } from "../cast/DraftScene";
import { InGameScene } from "../cast/InGameScene";
import { LineupsLive } from "../cast/LineupsScene";
import { SceneBackdrop } from "../cast/SceneBackdrop";
import { SeriesScene } from "../cast/SeriesScene";
import "../cast/cast-overlay.css";
import "../cast/postgame.css";

/** Point d'aiguillage des scènes (`series` / `lineups` : plein écran, sur `SceneBackdrop` opaque ; `series` alterne
 *  bandeau des cartes et post-game breakdown, cf. `SeriesScene`). `draft` : `DraftScene` (fond propre, skins `?skin=`). */
export function Scene({ s }: { s: CastSeries }) {
  switch (s.scene) {
    case "ingame":
      return <InGameScene s={s} />;
    case "series":
      return <><SceneBackdrop /><SeriesScene s={s} /></>;
    case "lineups":
      return <><SceneBackdrop /><LineupsLive s={s} /></>;
    case "draft":
      return <DraftSceneLive />; // peint son propre fond (`.stage`) : pas de SceneBackdrop
    case "hide":
    default:
      return null;
  }
}

/** Overlay OBS de la régie de cast. Reflète l'état serveur en direct (WS `cast.updated`). Skin via ?skin=. */
export function CastOverlay() {
  useLiveUpdates(); // hors Layout : l'overlay ouvre lui-même le WS (cast.updated invalide ["cast"])
  useDimHeroes(); // portraits (heroIcon) des scènes lineups et post-game
  const { data: s } = useCast({ refetchInterval: 10_000 }); // + WS : filet si une coupure passe inaperçue
  const skin = skinFromSearch(window.location.search);

  useEffect(() => {
    const prev = document.body.style.background;
    document.body.style.background = "transparent";
    return () => { document.body.style.background = prev; };
  }, []);

  // key = scène → remontage → fondu d'entrée ; `hide` (ou série inactive, sauf la scène Draft — R19) ne rend rien
  return (
    <div className="cast-overlay" data-skin={skin}>
      {s && overlayVisible(s) && <div className="scene" key={s.scene}><Scene s={s} /></div>}
    </div>
  );
}
