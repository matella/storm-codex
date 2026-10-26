// Couche API de la régie de cast : types miroir de `Series` (crates/storm-codex-server/src/cast/mod.rs)
// + un fetcher par route de cast/api.rs + `useCast()` (rafraîchi par le WS `cast.updated`, cf. api.ts).
import { useQuery } from "@tanstack/react-query";
import type { DraftFormat, Side } from "./api";

export type CastScene = "lineups" | "series" | "draft" | "ingame" | "hide";
export type LineupMode = "auto" | "a" | "b";

/** `MapStatus` serde (`tag = "status"`, aplati dans `PoolMap`) : `by` n'existe que pour banned/picked. */
export type PoolMap = { name: string } & (
  | { status: "available" }
  | { status: "banned"; by: Side }
  | { status: "picked"; by: Side }
  | { status: "decider" }
);

export interface CastPlayer {
  name: string;
  tagline: string;
  toon: string | null;
}

export interface CastTeam {
  name: string;
  /** URL `/api/cast/logo/{side}?v=…` ou null. */
  logo: string | null;
  record: string;
  /** Toujours 5 entrées côté serveur. */
  players: CastPlayer[];
  captain: number | null;
}

/** Résumé de partie : objet vide pour l'instant, champs ajoutés à la tâche 9. */
export type GameSummary = Record<string, never>;

export interface GameRecord {
  number: number;
  map: string;
  winner: Side | null;
  /** [blue, red]. */
  takedowns: [number, number];
  duration_secs: number | null;
  summary: GameSummary | null;
}

export interface SlideState {
  game: number | null;
  index: number;
  auto: boolean;
  interval_ms: number;
}

export interface CastSeries {
  schema_version: number;
  active: boolean;
  blue: CastTeam;
  red: CastTeam;
  bo: number;
  format: DraftFormat;
  /** [blue, red] — remplace le score calculé quand présent. */
  score_override: [number, number] | null;
  pool: PoolMap[];
  games: GameRecord[];
  scene: CastScene;
  lineup_mode: LineupMode;
  slide: SlideState;
}

export interface CastSeriesBody {
  /** Équipe complète (nom, bilan, joueurs, capitaine, logo) ; `{name}` seul reste accepté. Logo null = on garde celui en place. */
  blue: Partial<CastTeam> & { name: string };
  red: Partial<CastTeam> & { name: string };
  bo: 1 | 3 | 5 | 7;
  format: DraftFormat;
  pool: string[];
}

export type MapMark =
  | { status: "available" }
  | { status: "banned"; by: Side }
  | { status: "picked"; by: Side }
  | { status: "decider" };

export interface WinnerBody {
  side: Side;
  takedowns?: [number, number];
  duration_secs?: number;
}

/** `game: 0` = désépingler (retour à « dernière partie », `slide.game` = null). */
export type SlidePatch = Partial<Pick<SlideState, "game" | "index" | "auto" | "interval_ms">>;

/** Les erreurs serveur sont `{"error":"<code>"}` (409 : variante `CastError`, ex. `NotInPool`) :
 *  on rejette avec `Error(code)`, comme `draftReplayLoad`. */
async function send<T>(method: string, path: string, body?: unknown): Promise<T> {
  const r = await fetch(path, {
    method,
    headers: body !== undefined ? { "Content-Type": "application/json" } : undefined,
    body: body !== undefined ? JSON.stringify(body) : undefined,
  });
  if (!r.ok) {
    const b = await r.json().catch(() => ({}));
    throw new Error((b as { error?: string }).error ?? `http_${r.status}`);
  }
  return r.json() as Promise<T>;
}

export const fetchCast = () => send<CastSeries>("GET", "/api/cast");
export const castSeries = (b: CastSeriesBody) => send<CastSeries>("POST", "/api/cast/series", b);
export const castTeams = (blue: CastTeam, red: CastTeam) => send<CastSeries>("PUT", "/api/cast/teams", { blue, red });
export const castMarkMap = (name: string, mark: MapMark) =>
  send<CastSeries>("POST", "/api/cast/pool/map", { name, ...mark });
export const castStartGame = (map: string) => send<{ number: number }>("POST", "/api/cast/games/start", { map });
export const castWinner = (n: number, b: WinnerBody) => send<CastSeries>("POST", `/api/cast/games/${n}/winner`, b);
/** `null` = retour au score calculé. */
export const castScore = (score: { blue: number; red: number } | null) =>
  send<CastSeries>("POST", "/api/cast/score", score);
export const castScene = (scene: CastScene) => send<CastSeries>("POST", "/api/cast/scene", { scene });
export const castLineupMode = (mode: LineupMode) => send<CastSeries>("POST", "/api/cast/lineup-mode", { mode });
export const castSlide = (patch: SlidePatch) => send<CastSeries>("POST", "/api/cast/slide", patch);
/** Corps = octets de l'image (png/jpeg/webp, ≤ 512 Ko) ; répond la série avec `Team.logo` mis à jour. */
export async function castLogoUpload(side: Side, file: File): Promise<CastSeries> {
  const r = await fetch(`/api/cast/logo/${side}`, {
    method: "POST",
    headers: { "Content-Type": file.type },
    body: file,
  });
  if (!r.ok) {
    const b = await r.json().catch(() => ({}));
    throw new Error((b as { error?: string }).error ?? `http_${r.status}`);
  }
  return r.json() as Promise<CastSeries>;
}

/** État de la série ; invalidé en direct par `useLiveUpdates` (`cast.updated`). */
export function useCast() {
  return useQuery({ queryKey: ["cast"], queryFn: fetchCast });
}
