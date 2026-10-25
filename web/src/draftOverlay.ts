import type { DraftState } from "./api";

/** Draft de replay préparé mais rien révélé : l'overlay affiche « Draft prêt — en attente ». */
export function isReplayReady(d: DraftState): boolean {
  return d.replay?.current != null && d.cursor === 0;
}

/** Fraction (0–1) de la barre de timer. Hors replay : décompte local de 45 s. En replay : pilotée par
 *  le prochain tick de lecture auto (`next_at_ms`), vide en lecture manuelle ou en pause. */
export function timerFraction(d: DraftState, now: number, localSecs: number, fallbackSecs: number): number {
  const r = d.replay;
  if (r?.current == null) return localSecs / fallbackSecs;
  if (!r.playing || r.next_at_ms == null || r.interval_ms <= 0) return 0;
  return Math.min(1, Math.max(0, (r.next_at_ms - now) / r.interval_ms));
}
