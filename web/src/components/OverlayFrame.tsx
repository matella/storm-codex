import { useEffect, type ReactNode, type CSSProperties } from "react";

type Anchor = "top-left" | "top-right" | "bottom-left" | "bottom-right";
const ANCHORS: Record<Anchor, CSSProperties> = {
  "top-left": { top: 0, left: 0 },
  "top-right": { top: 0, right: 0 },
  "bottom-left": { bottom: 0, left: 0 },
  "bottom-right": { bottom: 0, right: 0 },
};

/**
 * Canevas OBS standard **1920×1080**, fond transparent : règle la browser source sur 1920×1080 et
 * tous les overlays partagent le même cadre — alignement et redimensionnement (scale uniforme)
 * propres et prévisibles. Le contenu est ancré dans un coin avec un padding constant.
 */
export function OverlayFrame({
  anchor = "top-left",
  pad = 28,
  children,
}: {
  anchor?: Anchor;
  pad?: number;
  children: ReactNode;
}) {
  useEffect(() => {
    document.body.style.background = "transparent";
    document.body.style.margin = "0";
    return () => {
      document.body.style.background = "";
      document.body.style.margin = "";
    };
  }, []);
  return (
    <div style={{ position: "fixed", top: 0, left: 0, width: 1920, height: 1080, overflow: "hidden" }}>
      <div style={{ position: "absolute", padding: pad, ...ANCHORS[anchor] }}>{children}</div>
    </div>
  );
}
