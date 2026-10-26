import { useEffect } from "react";
import { useLiveUpdates } from "../api";
import { useCast } from "../castApi";
import type { CastSeries } from "../castApi";
import { InGameScene } from "../cast/InGameScene";
import { SeriesScene } from "../cast/SeriesScene";
import "../cast/cast-overlay.css";

const SKINS = ["nexus", "glass", "tactical", "mono"];

/** Point d'aiguillage des scènes. `draft` / `lineups` : vides jusqu'aux tâches 8 et 10. */
function Scene({ s }: { s: CastSeries }) {
  switch (s.scene) {
    case "ingame":
      return <InGameScene s={s} />;
    case "series":
      return <SeriesScene s={s} />;
    case "draft":
    case "lineups":
    case "hide":
    default:
      return null;
  }
}

/** Overlay OBS de la régie de cast. Reflète l'état serveur en direct (WS `cast.updated`). Skin via ?skin=. */
export function CastOverlay() {
  useLiveUpdates(); // hors Layout : l'overlay ouvre lui-même le WS (cast.updated invalide ["cast"])
  const { data: s } = useCast();
  const skinParam = new URLSearchParams(window.location.search).get("skin") ?? "nexus";
  const skin = SKINS.includes(skinParam) ? skinParam : "nexus";

  useEffect(() => {
    const prev = document.body.style.background;
    document.body.style.background = "transparent";
    return () => { document.body.style.background = prev; };
  }, []);

  // key = scène → remontage → fondu d'entrée ; `hide` (ou série inactive) ne rend rien
  return (
    <div className="cast-overlay" data-skin={skin}>
      {s && s.active && <div className="scene" key={s.scene}><Scene s={s} /></div>}
    </div>
  );
}
