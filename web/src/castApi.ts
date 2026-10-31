// Couche API de la régie de cast : types miroir de `Series` (crates/storm-codex-server/src/cast/mod.rs)
// + un fetcher par route de cast/api.rs + `useCast()` (rafraîchi par le WS `cast.updated`, cf. api.ts).
import { useQuery } from "@tanstack/react-query";
import { httpError } from "./api";
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

/** Ligne joueur du résumé (miroir de `PlayerSummary`, cast/mod.rs ; valeurs `gameStats` storm-stats). */
export interface PlayerSummary {
  side: Side;
  /** ToonHandle storm-stats (rattachement R14). */
  toon: string;
  name: string;
  hero: string;
  /** `SoloKill` (colonne K de la fiche de match). */
  kills: number;
  takedowns: number;
  deaths: number;
  assists: number;
  hero_damage: number;
  siege_damage: number;
  healing: number;
}

/** Résumé d'une partie extrait de son replay Custom (`POST /api/cast/games/{n}/replay`).
 *  Paires `[blue, red]` ; `null` / vide = valeur absente de storm-stats → ligne retirée de l'écran. */
export interface GameSummary {
  duration_secs: number;
  winner: Side;
  takedowns: [number, number];
  team_level: [number, number] | null;
  /** Forts + keeps détruits par l'équipe. */
  structures: [number, number] | null;
  merc_camps: [number, number] | null;
  objectives: [number, number] | null;
  bans: [string[], string[]];
  /** Blue puis Red, ordre de pick. */
  players: PlayerSummary[];
  /** [secondes, avance d'XP de Blue] (négatif = Red devant). */
  xp_lead: [number, number][];
}

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
  /** Ancre de la rotation auto (ms, horloge serveur) : posée à chaque modification du diaporama. */
  since_ms: number;
}

export interface CastSeries {
  schema_version: number;
  /** Identité de la série (uuid posé par « New series » ; "" avant la première). */
  series_id: string;
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
  if (!r.ok) throw await httpError(r);
  return r.json() as Promise<T>;
}

export const fetchCast = () => send<CastSeries>("GET", "/api/cast");
export const castSeries = (b: CastSeriesBody) => send<CastSeries>("POST", "/api/cast/series", b);
export const castTeams = (blue: CastTeam, red: CastTeam) => send<CastSeries>("PUT", "/api/cast/teams", { blue, red });
export const castMarkMap = (name: string, mark: MapMark) =>
  send<CastSeries>("POST", "/api/cast/pool/map", { name, ...mark });
export const castStartGame = (map: string) => send<{ number: number }>("POST", "/api/cast/games/start", { map });
export const castWinner = (n: number, b: WinnerBody) => send<CastSeries>("POST", `/api/cast/games/${n}/winner`, b);
/** Retire la dernière partie lancée par erreur (R21 ; 409 `NotLastGame` / `GameHasResult`). */
export const castRemoveGame = (n: number) => send<CastSeries>("DELETE", `/api/cast/games/${n}`);
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
  if (!r.ok) throw await httpError(r);
  return r.json() as Promise<CastSeries>;
}

/** Replay Custom de la partie `n` (octets bruts) → résumé ; rien n'est écrit en base. Erreurs :
 *  `empty_body`, `SeriesInactive`, `UnknownGame`, `side_mismatch`, `series_changed`, `not_custom`, `parse_failed`, `incomplete`.
 *  `swap` : l'opérateur force l'échange des côtés (équipe 1 du replay = Blue de la série). */
/** Réponse du dépôt : résumé orienté + `orientation` (`evidence` | `assumed` | `forced`) et
 *  `forced_against_evidence: true` quand l'échange forcé contredit les joueurs liés. */
export type GameReplayResult = GameSummary & {
  orientation?: "evidence" | "assumed" | "forced";
  forced_against_evidence?: boolean;
};
export async function castGameReplay(n: number, file: File, swap = false): Promise<GameReplayResult> {
  const r = await fetch(`/api/cast/games/${n}/replay${swap ? "?swap=1" : ""}`, {
    method: "POST",
    headers: { "Content-Type": "application/octet-stream" },
    body: file,
  });
  if (!r.ok) throw await httpError(r);
  return r.json() as Promise<GameReplayResult>;
}

/** État de la série ; invalidé en direct par `useLiveUpdates` (`cast.updated`). `refetchInterval` :
 *  filet de sécurité de l'overlay OBS (WS coupé sans reconnexion détectée). */
export function useCast(opts: { refetchInterval?: number } = {}) {
  return useQuery({ queryKey: ["cast"], queryFn: fetchCast, refetchInterval: opts.refetchInterval });
}

/** Héros récent d'un joueur (GET /api/cast/lineup) : ≤ 3 par joueur, liste vide = masqué. */
export interface HeroCount {
  hero: string;
  games: number;
}
/** Une liste par joueur, dans l'ordre des toons envoyés. */
export interface CastLineup {
  blue: HeroCount[][];
  red: HeroCount[][];
}

/** Toon vide = joueur non lié (liste vide renvoyée à sa position). */
export async function fetchCastLineup(blue: string[], red: string[]): Promise<CastLineup> {
  const q = `blue=${encodeURIComponent(blue.join(","))}&red=${encodeURIComponent(red.join(","))}`;
  return send<CastLineup>("GET", `/api/cast/lineup?${q}`);
}

/** Héros récents des joueurs de la série (rien à chercher si aucun toon n'est lié). */
export function useCastLineup(s: CastSeries) {
  const blue = s.blue.players.map((p) => p.toon ?? "");
  const red = s.red.players.map((p) => p.toon ?? "");
  const linked = [...blue, ...red].some((t) => t !== "");
  return useQuery({
    queryKey: ["cast-lineup", blue, red],
    queryFn: () => fetchCastLineup(blue, red),
    enabled: linked,
    staleTime: 60_000,
  });
}
