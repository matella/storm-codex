// Feed live d'une partie en cours (GET /api/live) — types + helpers purs (testés dans live.test.ts).
// Spec : docs/specs/2026-10-24-feed-live-en-partie-design.md.

export type LiveScope = "self" | "allies" | "all";
export type LiveStatus = "live" | "ended" | "unreadable";

export interface LiveAbility { link: number; cmd: number; count: number; recent: number[] }
export interface LivePlayer {
  player_id: number;
  team: 0 | 1;
  hero: string | null;
  hero_unit: string | null;
  level: number;
  talents: { tree_id: string; loop: number }[];
  deaths: number;
  takedowns: number;
  /** `null` = masquées par le niveau de visibilité ; `[]` = visibles, aucune pression. */
  abilities: LiveAbility[] | null;
}
export interface LiveFeedItem {
  loop: number;
  kind: "death" | "talent" | "level" | "camp" | "objective";
  player?: number;
  team?: 0 | 1;
  detail: Record<string, unknown>;
}
export interface LiveView {
  schema_version: number;
  session: string;
  status: LiveStatus;
  reason?: string;
  scope: LiveScope;
  me: number | null;
  me_source: "explicit" | "lobby" | "single_human" | null;
  map: string | null;
  gates_loop: number | null;
  tracker_loop: number;
  game_loop: number;
  started_at: string;
  last_chunk_at: string;
  protocol: { build: number | null; fallback: { requested: number; used: number } | null; assumed_latest: boolean };
  bytes: { tracker: number; game: number };
  team_levels: [number, number];
  players: LivePlayer[];
  feed: LiveFeedItem[];
}

export const LOOPS_PER_SEC = 16;
/** Au-delà, sans nouveau morceau, la page signale un flux interrompu (pause ou déconnexion). */
export const STALE_AFTER_SEC = 60;
/** Fenêtre de la timeline d'aptitudes (= fenêtre `recent` servie par le serveur). */
export const ABILITY_WINDOW_SEC = 180;

/** Horloge de jeu `m:ss` comptée depuis l'ouverture des portes ; avant, `-m:ss`. */
export function gameClock(loop: number, gatesLoop: number | null): string {
  const secs = Math.trunc((loop - (gatesLoop ?? 0)) / LOOPS_PER_SEC);
  const a = Math.abs(secs);
  return `${secs < 0 ? "-" : ""}${Math.floor(a / 60)}:${String(a % 60).padStart(2, "0")}`;
}

/** Retard des aptitudes sur le tracker, en secondes de jeu (≥ 0). */
export function abilityLagSec(v: Pick<LiveView, "tracker_loop" | "game_loop" | "bytes">): number | null {
  if (v.bytes.game === 0) return null; // aucune donnée d'aptitude encore
  return Math.max(0, Math.round((v.tracker_loop - v.game_loop) / LOOPS_PER_SEC));
}

/** Secondes écoulées depuis un horodatage ISO. */
export function secondsSince(iso: string, now: number): number {
  return Math.max(0, Math.round((now - Date.parse(iso)) / 1000));
}

/** Flux interrompu : partie encore « live » mais aucun morceau depuis `STALE_AFTER_SEC`. */
export function isStale(v: Pick<LiveView, "status" | "last_chunk_at">, now: number): boolean {
  return v.status === "live" && secondsSince(v.last_chunk_at, now) > STALE_AFTER_SEC;
}

/** Les deux équipes, joueurs triés par id. */
export function splitTeams(players: LivePlayer[]): [LivePlayer[], LivePlayer[]] {
  const sorted = [...players].sort((a, b) => a.player_id - b.player_id);
  return [sorted.filter((p) => p.team === 0), sorted.filter((p) => p.team === 1)];
}

/** Libellé d'une aptitude non nommée : `#491`, ou `#491·2` si l'index de commande n'est pas 0. */
export function abilityLabel(a: Pick<LiveAbility, "link" | "cmd">): string {
  return `#${a.link}${a.cmd ? `·${a.cmd}` : ""}`;
}

/** Aptitudes triées par nombre de pressions décroissant (puis par identifiant, stable). */
export function sortAbilities(list: LiveAbility[]): LiveAbility[] {
  return [...list].sort((a, b) => b.count - a.count || a.link - b.link || a.cmd - b.cmd);
}

/** Histogramme des pressions récentes : `bins` cases sur la fenêtre, la dernière = maintenant. */
export function recentBins(recent: number[], gameLoop: number, bins = 36, windowSec = ABILITY_WINDOW_SEC): number[] {
  const out = new Array<number>(bins).fill(0);
  const span = windowSec * LOOPS_PER_SEC;
  const start = gameLoop - span;
  for (const l of recent) {
    if (l < start || l > gameLoop) continue;
    const i = Math.min(bins - 1, Math.floor(((l - start) / span) * bins));
    out[i] += 1;
  }
  return out;
}

/** Talents actuels d'un joueur : le **dernier choix par palier**, triés par palier. Une
 *  réinitialisation de talents (possible en Sandbox) ré-émet tous les choix : le fil les garde,
 *  le tableau des scores n'affiche que l'état courant. `tierOf` = palier du référentiel
 *  (`dim_talents`) ; inconnu → le choix garde son rang d'arrivée. */
export function currentTalents<T extends { tree_id: string }>(
  talents: T[],
  tierOf: (treeId: string) => number | null,
): (T & { tier: number | null })[] {
  const byTier = new Map<string, T & { tier: number | null }>();
  talents.forEach((t, i) => {
    const tier = tierOf(t.tree_id);
    byTier.set(tier == null ? `#${i}` : String(tier), { ...t, tier });
  });
  return [...byTier.values()].sort((a, b) => (a.tier ?? Infinity) - (b.tier ?? Infinity));
}

const decamel = (s: string) => s.replace(/([a-z])([A-Z])/g, "$1 $2");

/** Phrase courte d'un élément du fil. `hero(p)` nomme un joueur, `talent(id)` un talent. */
export function feedText(
  item: LiveFeedItem,
  hero: (playerId: number) => string,
  talent: (treeId: string) => string,
): string {
  const d = item.detail;
  switch (item.kind) {
    case "death": {
      const killers = Array.isArray(d.killers) ? (d.killers as number[]) : [];
      const who = item.player != null ? hero(item.player) : "?";
      return killers.length ? `${who} killed by ${killers.map(hero).join(", ")}` : `${who} died`;
    }
    case "talent":
      return `${item.player != null ? hero(item.player) : "?"} took ${talent(String(d.tree_id ?? ""))}`;
    case "level":
      return `${teamName(item.team)} reached level ${String(d.level ?? "?")}`;
    case "camp":
      return `${teamName(item.team)} captured ${decamel(String(d.camp || "a camp"))}`;
    default:
      return decamel(String(d.name ?? "map event"));
  }
}

export function teamName(team: number | undefined): string {
  return team === 0 ? "Team 1" : team === 1 ? "Team 2" : "A team";
}

const SCOPE_KEY = "live.scope";
const ME_KEY = "live.me";

/** Niveau mémorisé par navigateur ; `self` par défaut (et si le stockage est indisponible). */
export function loadScope(): LiveScope {
  try {
    const v = localStorage.getItem(SCOPE_KEY);
    return v === "allies" || v === "all" ? v : "self";
  } catch {
    return "self";
  }
}
export function saveScope(s: LiveScope): void {
  try { localStorage.setItem(SCOPE_KEY, s); } catch { /* stockage indisponible */ }
}
/** « Je suis » choisi à la main (id joueur 1–10), mémorisé par session de jeu. */
export function loadMe(session: string): number | null {
  try {
    const raw = localStorage.getItem(ME_KEY);
    if (!raw) return null;
    const v = JSON.parse(raw) as { session?: string; me?: number };
    return v.session === session && typeof v.me === "number" ? v.me : null;
  } catch {
    return null;
  }
}
export function saveMe(session: string, me: number | null): void {
  try {
    if (me == null) localStorage.removeItem(ME_KEY);
    else localStorage.setItem(ME_KEY, JSON.stringify({ session, me }));
  } catch { /* stockage indisponible */ }
}
