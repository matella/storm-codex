import { useEffect, type ReactNode, type CSSProperties } from "react";

type Anchor = "top-left" | "top-right" | "bottom-left" | "bottom-right";

/**
 * Canevas OBS standard **1920×1080**, fond transparent : règle la browser source sur 1920×1080 et
 * tous les overlays partagent le même cadre — alignement et redimensionnement (scale uniforme)
 * propres et prévisibles. Le contenu est ancré dans un coin avec un retrait `pad` constant ; les
 * offsets explicites `top`/`right`/`bottom`/`left` (px) priment sur le coin choisi.
 */
export function OverlayFrame({
  anchor = "top-left",
  pad = 28,
  top,
  right,
  bottom,
  left,
  children,
}: {
  anchor?: Anchor;
  pad?: number;
  top?: number;
  right?: number;
  bottom?: number;
  left?: number;
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

  // position depuis le coin (pad = retrait), puis surcharges explicites
  const pos: CSSProperties = {};
  if (anchor.startsWith("top")) pos.top = pad;
  else pos.bottom = pad;
  if (anchor.endsWith("left")) pos.left = pad;
  else pos.right = pad;
  if (top != null) { pos.top = top; delete pos.bottom; }
  if (bottom != null) { pos.bottom = bottom; delete pos.top; }
  if (left != null) { pos.left = left; delete pos.right; }
  if (right != null) { pos.right = right; delete pos.left; }

  return (
    <div style={{ position: "fixed", top: 0, left: 0, width: 1920, height: 1080, overflow: "hidden" }}>
      <div style={{ position: "absolute", ...pos }}>{children}</div>
    </div>
  );
}
