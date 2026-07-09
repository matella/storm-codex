// Types + logique de seek pure (aucune dépendance React) pour la visionneuse 2D de replay.
// Le modèle vient de GET /api/matches/{id}/replay2d (crate storm-replay-viewer).

export interface Sample { t: number; x: number; y: number; exact: boolean }
export interface Interval { from: number; to: number }
export interface HeroTrack { playerId: number; samples: Sample[]; life: Interval[] }
export interface PlayerMeta { playerId: number; name: string | null; hero: string | null; team: number | null; win: boolean | null }
export interface Death { t: number; x: number; y: number; victimPlayerId: number; killerPlayerId: number | null }
export interface Replay2D {
  meta: { mapName: string; mapSize: [number, number]; durationSec: number; loopOffset: number; viewerVersion: number };
  players: PlayerMeta[]; heroes: HeroTrack[]; deaths: Death[]; warnings: string[];
}

const aliveAt = (life: Interval[], t: number) => life.some((iv) => t >= iv.from && t <= iv.to);

/** Fin de vie la plus récente avant/à t (= instant de la mort). null si aucune (avant spawn). */
function lastAliveEnd(life: Interval[], t: number): number | null {
  let best: number | null = null;
  for (const iv of life) if (iv.to <= t && (best === null || iv.to > best)) best = iv.to;
  return best;
}

/** Interpolation linéaire de la position à l'instant `t` (bornée au 1er/dernier sample). */
function interp(s: Sample[], t: number): { x: number; y: number } {
  const hi0 = s.length - 1;
  if (t <= s[0].t) return { x: s[0].x, y: s[0].y };
  if (t >= s[hi0].t) return { x: s[hi0].x, y: s[hi0].y };
  let lo = 0, hi = hi0, i = 0;
  while (lo <= hi) { const mid = (lo + hi) >> 1; if (s[mid].t <= t) { i = mid; lo = mid + 1; } else hi = mid - 1; }
  const a = s[i], b = s[i + 1] ?? a;
  const f = b.t === a.t ? 0 : (t - a.t) / (b.t - a.t);
  return { x: a.x + (b.x - a.x) * f, y: a.y + (b.y - a.y) * f };
}

/** Position + état vivant/mort d'un héros à l'instant t (pure). null si pas de samples.
 *  Mort : on FIGE la position à l'instant de la mort (fin du dernier intervalle vivant) — pas de
 *  lerp à travers le trou mort→respawn (le respawn est à la base, loin du lieu de mort). */
export function sampleAt(h: HeroTrack, t: number): { x: number; y: number; alive: boolean } | null {
  const s = h.samples;
  if (!s.length) return null;
  const alive = aliveAt(h.life, t);
  const et = alive ? t : (lastAliveEnd(h.life, t) ?? t); // temps effectif : mort → instant du décès
  const p = interp(s, et);
  return { x: p.x, y: p.y, alive };
}

/** Morts « récentes » à t (marqueur qui persiste ~4 s de scrub). */
export function deathsNear(deaths: Death[], t: number, window = 4): Death[] {
  return deaths.filter((d) => t >= d.t && t <= d.t + window);
}
