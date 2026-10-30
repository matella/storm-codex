// Aperçu de la sortie OBS : l'overlay réel (`/cast/overlay`, 1920×1080 logique) dans une iframe
// mise à l'échelle par transform à la largeur du volet. Tant que la largeur n'est pas mesurée, le
// cadre garde le ratio 16:9 et l'iframe reste masquée (pas d'image à une échelle arbitraire).
import { useEffect, useLayoutEffect, useRef, useState } from "react";

const W = 1920;
const H = 1080;

// useLayoutEffect côté navigateur (mesure avant la première peinture) ; useEffect en rendu serveur/test.
const useIsoLayoutEffect = typeof window === "undefined" ? useEffect : useLayoutEffect;

/** Styles du cadre et de l'iframe pour une échelle mesurée (`null` = pas encore mesurée). */
export function previewStyles(scale: number | null) {
  return {
    box: scale === null ? { aspectRatio: `${W} / ${H}` } : { height: H * scale },
    frame: scale === null ? { visibility: "hidden" as const } : { transform: `scale(${scale})` },
  };
}

export function ScenePreview({ onAir }: { onAir: string }) {
  const box = useRef<HTMLDivElement>(null);
  const [scale, setScale] = useState<number | null>(null);
  useIsoLayoutEffect(() => {
    const el = box.current;
    if (!el) return;
    const fit = () => { if (el.clientWidth > 0) setScale(el.clientWidth / W); };
    fit();
    if (typeof ResizeObserver === "undefined") return;
    const ro = new ResizeObserver(fit);
    ro.observe(el);
    return () => ro.disconnect();
  }, []);
  const st = previewStyles(scale);
  return (
    <div className="cr-prev">
      <div className="cr-prev-k">Live preview · {onAir} on air</div>
      <div className="cr-prev-box" ref={box} style={st.box}>
        <iframe title="Cast overlay preview" src="/cast/overlay" width={W} height={H} style={st.frame} />
      </div>
    </div>
  );
}
