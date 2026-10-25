// Aperçu de la sortie OBS : l'overlay réel (`/cast/overlay`, 1920×1080 logique) dans une iframe
// mise à l'échelle par transform à la largeur du volet.
import { useEffect, useRef, useState } from "react";

const W = 1920;
const H = 1080;

export function ScenePreview({ onAir }: { onAir: string }) {
  const box = useRef<HTMLDivElement>(null);
  const [scale, setScale] = useState(0.4);
  useEffect(() => {
    const el = box.current;
    if (!el) return;
    const fit = () => setScale(el.clientWidth / W || 0.4);
    fit();
    if (typeof ResizeObserver === "undefined") return;
    const ro = new ResizeObserver(fit);
    ro.observe(el);
    return () => ro.disconnect();
  }, []);
  return (
    <div className="cr-prev">
      <div className="cr-prev-k">Live preview · {onAir} on air</div>
      <div className="cr-prev-box" ref={box} style={{ height: H * scale }}>
        <iframe
          title="Cast overlay preview"
          src="/cast/overlay"
          width={W}
          height={H}
          style={{ transform: `scale(${scale})` }}
        />
      </div>
    </div>
  );
}
