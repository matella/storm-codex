// Logique pure de la page Session (tests : session.test.ts) : dernière soirée de l'opérateur,
// forme récente, bilans par mode et par héros. `me` = joueur opérateur d'une partie (ou undefined).

import type { MatchPlayer, MatchSummary } from "./api";

export interface MyGame { m: MatchSummary; me: MatchPlayer; won: boolean }

/** Parties de l'opérateur (du plus récent au plus ancien, ordre de l'API conservé). */
export function myGames(matches: MatchSummary[], pick: (ps: MatchPlayer[]) => MatchPlayer | undefined): MyGame[] {
  const out: MyGame[] = [];
  for (const m of matches) {
    const me = pick(m.players ?? []);
    if (!me) continue;
    out.push({ m, me, won: me.team != null && m.winner === me.team });
  }
  return out;
}

/** Jour local (AAAA-MM-JJ) d'une date ISO. */
export function localDay(iso: string | null): string {
  if (!iso) return "";
  const d = new Date(iso);
  return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}-${String(d.getDate()).padStart(2, "0")}`;
}

/** Dernière session : les parties du jour local de la partie la plus récente, dans l'ordre
 *  chronologique. Une soirée qui passe minuit reste une session tant que l'écart entre deux
 *  parties consécutives est < 3 h. */
export function lastSession(games: MyGame[]): MyGame[] {
  if (!games.length) return [];
  const sorted = [...games].sort((a, b) => (b.m.played_at ?? "").localeCompare(a.m.played_at ?? ""));
  const out = [sorted[0]];
  for (let i = 1; i < sorted.length; i++) {
    const prev = new Date(out[out.length - 1].m.played_at ?? 0).getTime();
    const cur = new Date(sorted[i].m.played_at ?? 0).getTime();
    if (prev - cur > 3 * 3600 * 1000) break;
    out.push(sorted[i]);
  }
  return out.reverse();
}

/** K / D / A d'une partie : l'API donne kills, deaths et takedowns (A = takedowns − kills). */
export function kda(p: MatchPlayer): { k: number; d: number; a: number } {
  const k = p.kills ?? 0;
  return { k, d: p.deaths ?? 0, a: Math.max(0, (p.takedowns ?? 0) - k) };
}

export interface Tally { key: string; games: number; wins: number }

function tally(games: MyGame[], key: (g: MyGame) => string | null): Tally[] {
  const m = new Map<string, Tally>();
  for (const g of games) {
    const k = key(g);
    if (!k) continue;
    const t = m.get(k) ?? { key: k, games: 0, wins: 0 };
    t.games += 1;
    if (g.won) t.wins += 1;
    m.set(k, t);
  }
  return [...m.values()].sort((a, b) => b.games - a.games || a.key.localeCompare(b.key));
}

export const byHero = (games: MyGame[]) => tally(games, (g) => g.me.hero);
export const byMode = (games: MyGame[], label: (mode: number | null) => string) => tally(games, (g) => label(g.m.mode));

/** Pourcentage arrondi (0 si aucun effectif). */
export const pctOf = (wins: number, games: number) => (games ? Math.round((wins * 100) / games) : 0);
