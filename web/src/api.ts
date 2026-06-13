// Couche API typée vers storm-codex-server + hook WebSocket temps réel.
import { useEffect } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";

export interface MatchPlayer {
  toon: string;
  name: string | null;
  hero: string | null;
  team: number | null;
  win: boolean | null;
  kills: number | null;
  deaths: number | null;
  takedowns: number | null;
}
export interface MatchSummary {
  id: number;
  map: string | null;
  mode: number | null;
  played_at: string | null;
  length: number | null;
  winner: number | null;
  build: number | null;
  players: MatchPlayer[] | null;
}
export interface HeroStat {
  hero: string;
  games: number;
  wins: number;
}
export interface PlayerSummary {
  toon: string;
  name: string | null;
  names: string[];
  matches: number;
  wins: number;
  heroes: HeroStat[];
}

async function get<T>(path: string): Promise<T> {
  const r = await fetch(path);
  if (!r.ok) throw new Error(`${path} → ${r.status}`);
  return r.json() as Promise<T>;
}

export interface MatchListParams {
  map?: string;
  mode?: number;
  hero?: string;
  player?: string;
  limit?: number;
}
export function matchesUrl(p: MatchListParams): string {
  const q = new URLSearchParams();
  if (p.map) q.set("map", p.map);
  if (p.mode != null) q.set("mode", String(p.mode));
  if (p.hero) q.set("hero", p.hero);
  if (p.player) q.set("player", p.player);
  q.set("limit", String(p.limit ?? 50));
  return `/api/matches?${q.toString()}`;
}

export const fetchMatches = (p: MatchListParams) => get<MatchSummary[]>(matchesUrl(p));
export const fetchMatch = (id: number | string) =>
  get<{ id: number; match: any; players: Record<string, any> }>(`/api/matches/${id}`);
export const fetchPlayer = (toon: string) => get<PlayerSummary>(`/api/players/${encodeURIComponent(toon)}`);
export const fetchHeroes = () => get<HeroStat[]>("/api/heroes");

/** WS `/ws` : à chaque `match.parsed`, invalide les listes de matchs (temps réel). */
export function useLiveUpdates(onMatch?: (m: { match_id: number; map?: string }) => void) {
  const qc = useQueryClient();
  useEffect(() => {
    const proto = location.protocol === "https:" ? "wss" : "ws";
    let ws: WebSocket | null = null;
    let closed = false;
    const connect = () => {
      ws = new WebSocket(`${proto}://${location.host}/ws`);
      ws.onmessage = (e) => {
        try {
          const ev = JSON.parse(e.data);
          if (ev.type === "match.parsed") {
            qc.invalidateQueries({ queryKey: ["matches"] });
            qc.invalidateQueries({ queryKey: ["heroes"] });
            onMatch?.(ev);
          }
        } catch {
          /* ignore */
        }
      };
      ws.onclose = () => {
        if (!closed) setTimeout(connect, 2000); // reconnexion
      };
    };
    connect();
    return () => {
      closed = true;
      ws?.close();
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);
}

// ── helpers d'affichage ────────────────────────────────────────────────────
// Codes officiels (storm-stats constants.json → GameMode). Les anciennes valeurs étaient
// décalées (Storm League affichait "HL", QM "—"…).
const MODE_LABEL: Record<number, { short: string; cls: string }> = {
  50001: { short: "QM", cls: "b-qm" },
  50021: { short: "VS IA", cls: "b-qm" },
  50031: { short: "BRAWL", cls: "b-qm" },
  50041: { short: "ENTR.", cls: "b-qm" },
  50051: { short: "UD", cls: "b-sl" },
  50061: { short: "HL", cls: "b-sl" },
  50071: { short: "TL", cls: "b-sl" },
  50091: { short: "SL", cls: "b-sl" },
};
export function modeBadge(mode: number | null): { short: string; cls: string } {
  return (mode != null && MODE_LABEL[mode]) || { short: "—", cls: "b-qm" };
}

// ── référentiel héros (dim_heroes depuis HotsPatchNotes) → anneaux d'univers ──
export interface DimHero { universe: string | null; role: string | null; icon: string | null }
export type DimHeroes = Record<string, DimHero>;

const UNIVERSE_COLOR: Record<string, string> = {
  Warcraft: "var(--u-warcraft)",
  StarCraft: "var(--u-starcraft)",
  Diablo: "var(--u-diablo)",
  Overwatch: "var(--u-overwatch)",
  Nexus: "var(--u-nexus)",
};

/** Cache module-level peuplé par useDimHeroes — universeColor est synchrone (appelé par Avatar). */
let DIM: DimHeroes = {};
export function useDimHeroes() {
  const q = useQuery({ queryKey: ["dim-heroes"], queryFn: () => get<DimHeroes>("/api/dim/heroes"), staleTime: Infinity });
  if (q.data) DIM = q.data;
  return q.data;
}
export function universeColor(hero: string | null): string {
  const u = hero ? DIM[hero]?.universe : null;
  return (u && UNIVERSE_COLOR[u]) || "var(--u-nexus)";
}
export function initials(name: string | null): string {
  if (!name) return "··";
  return name.slice(0, 2).toUpperCase();
}
export function fmtTime(iso: string | null): string {
  if (!iso) return "—";
  const d = new Date(iso);
  return d.toLocaleString("fr-FR", { day: "2-digit", month: "2-digit", hour: "2-digit", minute: "2-digit" });
}
export function fmtDur(seconds: number | null): string {
  if (!seconds) return "—";
  const m = Math.floor(seconds / 60);
  const s = Math.floor(seconds % 60);
  return `${m}:${String(s).padStart(2, "0")}`;
}
