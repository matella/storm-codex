// Bloc « OBS source » de la régie : l'URL de l'overlay de cast, toujours visible dans l'en-tête de /cast
// (quel que soit l'onglet de scène), avec copie et choix du skin. La préférence de skin reste locale au
// navigateur (affichage seulement, aucun état partagé).
import { useState } from "react";

export const OBS_SKINS = ["nexus", "glass", "tactical", "mono"] as const;
export type ObsSkin = (typeof OBS_SKINS)[number];
const SKIN_KEY = "cast.obsSkin";

/** URL de la source navigateur OBS pour une origine et un skin donnés. */
export function obsOverlayUrl(origin: string, skin: ObsSkin): string {
  return `${origin}/cast/overlay?skin=${skin}`;
}

function readSkin(): ObsSkin {
  try {
    const v = window.localStorage.getItem(SKIN_KEY);
    return OBS_SKINS.find((s) => s === v) ?? "nexus";
  } catch {
    return "nexus"; // navigation privée ou stockage bloqué : repli sans erreur
  }
}

export function ObsSource() {
  const [skin, setSkin] = useState<ObsSkin>(readSkin);
  const [copy, setCopy] = useState<"idle" | "copied" | "failed">("idle");
  const origin = typeof window === "undefined" ? "" : window.location.origin;
  const url = obsOverlayUrl(origin, skin);

  const pick = (s: ObsSkin) => {
    setSkin(s);
    try {
      window.localStorage.setItem(SKIN_KEY, s);
    } catch {
      // préférence non mémorisée : le choix vaut pour cette session
    }
  };
  const doCopy = async () => {
    try {
      await navigator.clipboard.writeText(url);
      setCopy("copied");
    } catch {
      setCopy("failed");
    }
    window.setTimeout(() => setCopy("idle"), 1800);
  };

  return (
    <section className="cr-obs" aria-label="OBS source">
      <div className="cr-obs-row">
        <span className="cr-k">OBS source</span>
        <code className="cr-obs-url" title={url}>{url}</code>
        <button type="button" className="ds-btn small" onClick={() => void doCopy()}>
          {copy === "copied" ? "Copied" : copy === "failed" ? "Copy failed — select the URL" : "Copy URL"}
        </button>
        <label className="cr-note">Skin{" "}
          <select className="ds-input" value={skin} aria-label="Overlay skin" onChange={(e) => pick(e.target.value as ObsSkin)}>
            {OBS_SKINS.map((s) => <option key={s} value={s}>{s}</option>)}
          </select>
        </label>
      </div>
      <div className="cr-note">Add it as a Browser source · 1920×1080 · transparent background. The scene shown is chosen in this console.</div>
    </section>
  );
}
