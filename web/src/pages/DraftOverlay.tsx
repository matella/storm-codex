import { useEffect } from "react";
import { useDraft, useDimHeroes } from "../api";
import { DraftScene, skinFromSearch } from "../cast/DraftScene";

/** Overlay OBS du draft (vue Timeline). Reflète l'état serveur en direct (WS). Skin via ?skin=. */
export function DraftOverlay() {
  const { data: d } = useDraft();
  useDimHeroes(); // peuple le cache portraits/univers (l'overlay est hors du Layout qui le fait normalement)
  const skin = skinFromSearch(window.location.search);

  useEffect(() => {
    const prev = document.body.style.background;
    document.body.style.background = "transparent";
    return () => { document.body.style.background = prev; };
  }, []);

  return <DraftScene d={d} skin={skin} />;
}
