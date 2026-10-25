// Ancres de l'overlay In-game, en fractions de l'écran 16:9 (repères relevés sur la capture réelle
// docs/specs/2026-10-25-cast-mockups/ingame.webp). Surchargeables pour un réglage fin.
export const INGAME_ANCHORS = { blueInnerX: 0.392, redInnerX: 0.5965, topY: 0.099, tabH: 0.046, maxW: 0.26 };
export type Anchors = typeof INGAME_ANCHORS;

export interface TabBox {
  top: string;
  height: string;
  maxWidth: string;
  left?: string;
  right?: string;
}

const pct = (f: number) => `${(f * 100).toFixed(2)}%`;

/** Boîte d'un onglet : le coin intérieur haut est posé sur l'ancre, l'onglet pousse vers l'extérieur. */
export function tabBox(side: "blue" | "red", a: Anchors = INGAME_ANCHORS): TabBox {
  const base = { top: pct(a.topY), height: pct(a.tabH), maxWidth: pct(a.maxW) };
  return side === "blue" ? { ...base, right: pct(1 - a.blueInnerX) } : { ...base, left: pct(a.redInnerX) };
}
