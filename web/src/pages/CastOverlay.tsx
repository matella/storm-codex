import { useEffect, useState } from "react";
import { useDimHeroes, useLiveUpdates } from "../api";
import { useCast } from "../castApi";
import type { CastSeries } from "../castApi";
import { sceneTransition, shownScene } from "../castState";
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

/** Durée du fondu de sortie (ms), égale au fondu d'entrée de `cast-overlay.css`. */
const FADE_MS = 300;

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

  // couche courante (key = id de couche → montage neuf à chaque changement de scène → fondu d'entrée ; la couche qui part
  // garde son id, donc la MÊME instance, seule la classe `out` change) + couche sortante gardée le temps du fondu
  // de sortie, rendue avec l'instantané qui l'affichait ; `hide` (ou série inactive, sauf Draft — R19) = rien
  const [st, setSt] = useState(() => ({ cur: shownScene(s), leaving: null as ReturnType<typeof shownScene> }));
  const next = sceneTransition(st, s);
  if (next !== st) setSt(next); // mise à jour pendant le rendu : pas d'image intermédiaire sans l'ancienne scène
  const { cur, leaving } = next;
  useEffect(() => {
    if (!leaving) return;
    const id = window.setTimeout(() => setSt((x) => (x.leaving === leaving ? { ...x, leaving: null } : x)), FADE_MS);
    return () => window.clearTimeout(id);
  }, [leaving]);
  return (
    <div className="cast-overlay" data-skin={skin}>
      {leaving && <div className="scene out" key={leaving.id} aria-hidden="true"><Scene s={leaving.s} /></div>}
      {cur && <div className="scene" key={cur.id}><Scene s={cur.s} /></div>}
    </div>
  );
}
