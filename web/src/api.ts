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
  award?: string | null; // ex. "EndOfMatchAwardMVPBoolean"
}

/** Emoji par type d'award HotS (clé = nom « core », sans EndOfMatchAward…Boolean). Couvre les
 *  types réellement présents dans la base ; fallback 🏅 pour un type inconnu. */
const AWARD_ICON: Record<string, string> = {
  MVP: "👑",
  MostHeroDamageDone: "🗡️",
  MostTeamfightHeroDamageDone: "⚔️",
  MostSiegeDamageDone: "🏰",
  MostHealing: "💚",
  MostTeamfightHealingDone: "💗",
  ClutchHealer: "🚑",
  MostDamageTaken: "🛡️",
  MostTeamfightDamageTaken: "🧱",
  MostMercCampsCaptured: "🏕️",
  HighestKillStreak: "🔥",
  HatTrick: "🎩",
  MostKills: "💀",
  MostStuns: "💫",
  MostSilences: "🤐",
  MostRoots: "🌿",
  MostXPContribution: "📈",
  MostDamageDoneToZerg: "🐛",
  MostAltarDamageDone: "⛩️",
  MostImmortalDamage: "👹",
  MostCurseDamageDone: "☠️",
  MostGemsTurnedIn: "💎",
  MostTimeOnPoint: "📍",
  MostDamageToMinions: "🧟",
  MostEscapes: "🏃",
  MostDaredevilEscapes: "🪂",
  MostDragonShrinesCaptured: "🐉",
  MostTimeInTemple: "🛕",
  MostVengeancesPerformed: "😤",
  MostProtection: "🤝",
  "0OutnumberedDeaths": "🧊",
  "0Deaths": "😇",
  MostInterruptedCageUnlocks: "🔓",
  MostCoinsPaid: "🪙",
  MostNukeDamageDone: "☢️",
  MostSeedsCollected: "🌱",
};

/** Award de fin de partie d'un joueur → libellé court + emoji par type + drapeau MVP. `null` si
 *  aucun award. HotS attribue UN award par joueur : MVP au meilleur, sinon une catégorie. */
export function awardLabel(raw: string | null | undefined): { label: string; icon: string; mvp: boolean } | null {
  if (!raw) return null;
  const core = raw.replace(/^EndOfMatchAward/, "").replace(/Boolean$/, "");
  const icon = AWARD_ICON[core] ?? "🏅";
  if (core === "MVP") return { label: "MVP", icon, mvp: true };
  // décamelise + raccourcit les libellés verbeux pour le tooltip
  const label = core
    .replace(/([a-z])([A-Z])/g, "$1 $2")
    .replace(/^Most /, "")
    .replace(/ Done$/, "");
  return { label, icon, mvp: false };
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
export interface PlayerHero { hero: string; games: number; wins: number; avg_kills: number | null; avg_deaths: number | null; avg_takedowns: number | null }
export interface PlayerGame { match_id: number; hero: string | null; map: string | null; mode: number | null; win: boolean | null; kills: number | null; deaths: number | null; takedowns: number | null; played_at: string | null; award: string | null }
export interface PlayerSummary {
  toon: string;
  name: string | null;
  names: string[];
  matches: number;
  wins: number;
  avg_kills: number | null;
  avg_deaths: number | null;
  avg_takedowns: number | null;
  heroes: PlayerHero[];
  recent: PlayerGame[];
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
  account?: string;            // compte opérateur précis
  result?: "win" | "loss";     // perspective opérateur
  mvp?: boolean;               // opérateur MVP uniquement
  from?: string;               // date ISO (inclus)
  to?: string;                 // date ISO (exclu)
  limit?: number;
}
export function matchesParams(p: MatchListParams): URLSearchParams {
  const q = new URLSearchParams();
  if (p.map) q.set("map", p.map);
  if (p.mode != null) q.set("mode", String(p.mode));
  if (p.hero) q.set("hero", p.hero);
  if (p.player) q.set("player", p.player);
  if (p.account) q.set("account", p.account);
  if (p.result) q.set("result", p.result);
  if (p.mvp) q.set("mvp", "true");
  if (p.from) q.set("from", p.from);
  if (p.to) q.set("to", p.to);
  return q;
}
export function matchesUrl(p: MatchListParams): string {
  const q = matchesParams(p);
  q.set("limit", String(p.limit ?? 50));
  return `/api/matches?${q.toString()}`;
}

export const fetchMatches = (p: MatchListParams) => get<MatchSummary[]>(matchesUrl(p));

// ── filtres d'agrégats (Heroes/Maps) ─────────────────────────────────────────
export interface AggFilter { mode?: number; mine?: boolean; account?: string; from?: string; to?: string }
/** Construit la query string d'un agrégat filtré. `from`/`to` sont des dates locales YYYY-MM-DD ;
 *  `to` est rendu inclusif (borne serveur exclusive → +1 jour). */
export function aggParams(f: AggFilter): string {
  const q = new URLSearchParams();
  if (f.mode != null) q.set("mode", String(f.mode));
  if (f.mine) q.set("mine", "true");
  if (f.account) q.set("account", f.account);
  if (f.from) q.set("from", new Date(f.from + "T00:00:00").toISOString());
  if (f.to) { const d = new Date(f.to + "T00:00:00"); d.setDate(d.getDate() + 1); q.set("to", d.toISOString()); }
  return q.toString();
}
export const fetchMatch = (id: number | string) =>
  get<{ id: number; match: any; players: Record<string, any> }>(`/api/matches/${id}`);
export const fetchPlayer = (toon: string) => get<PlayerSummary>(`/api/players/${encodeURIComponent(toon)}`);
export const fetchHeroes = (f: AggFilter = {}) => get<HeroStat[]>(`/api/heroes?${aggParams(f)}`);

export interface HeroDetail {
  hero: string;
  games: number;
  wins: number;
  avg_kills: number | null;
  avg_deaths: number | null;
  avg_takedowns: number | null;
  by_map: { map: string; games: number; wins: number }[];
  builds: { talents: Record<string, string>; games: number; wins: number }[];
}
export const fetchHeroDetail = (hero: string, mode?: number) =>
  get<HeroDetail>(`/api/hero/${encodeURIComponent(hero)}${mode != null ? `?mode=${mode}` : ""}`);

export interface Synergies {
  teammates: { name: string; games: number; wins: number }[];
  enemies: { hero: string; games: number; wins: number }[];
}
export const fetchSynergies = () => get<Synergies>("/api/synergies");

// ── patch notes (proxy HotsPatchNotes) ───────────────────────────────────────
export interface PatchItem {
  id: number; internalId: string; patchName: string; patchType: string;
  liveDate: string | null; heroCount: number; mapCount: number;
  officialLink: string | null; hasContent: boolean;
}
export interface PatchDetail {
  internalId: string; patchName: string; patchType: string;
  liveDate: string | null; officialLink: string | null; content: string | null;
}
export const fetchPatches = () => get<{ items: PatchItem[] }>("/api/patches");
export const fetchPatch = (id: string) => get<PatchDetail>(`/api/patches/${encodeURIComponent(id)}`);

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
  [-1]: { short: "CUSTOM", cls: "b-qm" },
  50001: { short: "QM", cls: "b-qm" },
  50021: { short: "VS IA", cls: "b-qm" },
  50031: { short: "BRAWL", cls: "b-qm" },
  50041: { short: "ENTR.", cls: "b-qm" },
  50051: { short: "UD", cls: "b-sl" },
  50061: { short: "HL", cls: "b-sl" },
  50071: { short: "TL", cls: "b-sl" },
  50091: { short: "SL", cls: "b-sl" },
  50101: { short: "ARAM", cls: "b-qm" },
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

/** Clé de jointure tolérante héros : HotsPatchNotes nomme sans ponctuation et inconstamment sur
 *  « The » (`ETC`, `LiMing`, `LostVikings`, `TheButcher`), alors que le parser garde `E.T.C.`,
 *  `Li-Ming`, `The Lost Vikings`… On normalise les deux : sans accents, alphanumérique seul,
 *  « the » initial retiré. Ainsi tous les héros composés retrouvent leur portrait/univers. */
export function heroKey(s: string): string {
  return s
    .normalize("NFD").replace(/[̀-ͯ]/g, "") // accents
    .toLowerCase().replace(/[^a-z0-9]/g, "")          // ponctuation/espaces
    .replace(/^the/, "");                             // article initial inconstant
}

/** Caches module-level peuplés par useDimHeroes — universeColor/heroIcon sont synchrones (Avatar).
 *  DIM_IDX indexe par clé normalisée pour résoudre les noms composés. */
let DIM: DimHeroes = {};
let DIM_IDX: DimHeroes = {};
export function useDimHeroes() {
  const q = useQuery({ queryKey: ["dim-heroes"], queryFn: () => get<DimHeroes>("/api/dim/heroes"), staleTime: Infinity });
  if (q.data && q.data !== DIM) {
    DIM = q.data;
    DIM_IDX = {};
    for (const [name, h] of Object.entries(DIM)) DIM_IDX[heroKey(name)] = h;
  }
  return q.data;
}
function dimHero(hero: string | null): DimHero | undefined {
  if (!hero) return undefined;
  return DIM[hero] ?? DIM_IDX[heroKey(hero)];
}
export function universeColor(hero: string | null): string {
  const u = dimHero(hero)?.universe;
  return (u && UNIVERSE_COLOR[u]) || "var(--u-nexus)";
}
/** Portrait du héros (vendorisé, servi sur /images) — null si inconnu (→ fallback initiales). */
export function heroIcon(hero: string | null): string | null {
  return dimHero(hero)?.icon || null;
}

// ── référentiel talents (dim_talents) : talentTreeId → nom/tier/héros ─────────
export interface DimTalent { name: string; tier: number; hero: string | null; icon: string | null }
export type DimTalents = Record<string, DimTalent>;
let DIMT: DimTalents = {};
export function useDimTalents() {
  const q = useQuery({ queryKey: ["dim-talents"], queryFn: () => get<DimTalents>("/api/dim/talents"), staleTime: Infinity });
  if (q.data) DIMT = q.data;
  return q.data;
}
/** Résout le `talentTreeId` stocké par le parser → nom lisible (+ tier). null si inconnu :
 *  le consommateur retombe sur l'id « décamelisé ». */
export function talentInfo(treeId: string | null): DimTalent | null {
  return (treeId && DIMT[treeId]) || null;
}
/** Image de carte : slug = nom en minuscules, apostrophes retirées, espaces → tirets.
 *  Les cartes ARAM peuvent ne pas avoir d'image (404) → le consommateur prévoit un fallback. */
export function mapImage(map: string | null): string | null {
  if (!map) return null;
  const slug = map.toLowerCase().replace(/['']/g, "").replace(/\s+/g, "-");
  return `/images/battlegrounds/${slug}.png`;
}
// ── identité opérateur (réglage app_settings.operator_names) ──────────────────
let OPERATOR_NAMES: string[] = [];
export function useSettings() {
  const q = useQuery({
    queryKey: ["settings"],
    queryFn: () => get<{ operator_names?: string[] }>("/api/settings"),
    staleTime: Infinity,
  });
  if (q.data?.operator_names) OPERATOR_NAMES = q.data.operator_names;
  return q.data;
}
export function operatorNames(): string[] {
  return OPERATOR_NAMES;
}
/** Le joueur opérateur dans une partie : `override` (?me=) prioritaire, sinon n'importe lequel
 *  des noms configurés, sinon le 1er joueur (fallback). Insensible à la casse. */
export function pickOperator<T extends { name: string | null }>(
  players: T[],
  override?: string | null,
): T | undefined {
  const candidates = [override, ...OPERATOR_NAMES].filter(Boolean).map((n) => n!.toLowerCase());
  for (const c of candidates) {
    const found = players.find((p) => (p.name ?? "").toLowerCase() === c);
    if (found) return found;
  }
  return players[0];
}

/** Strict : le joueur opérateur si l'un des noms configurés matche, sinon undefined (pas de
 *  fallback) — pour COMPTER les parties de l'opérateur sans polluer avec un joueur quelconque. */
export function matchOperator<T extends { name: string | null }>(players: T[]): T | undefined {
  const names = OPERATOR_NAMES.map((n) => n.toLowerCase());
  return players.find((p) => names.includes((p.name ?? "").toLowerCase()));
}

/**
 * Phrase « Jarvis » déterministe (voix de majordome FR), choisie dans un répertoire selon le
 * contexte de la partie. Déterministe : la variante dépend de `match_id` (pas de scintillement).
 * `streak` (optionnel, +n victoires / -n défaites d'affilée) enrichit le ton.
 */
export function jarvisPhrase(
  opts: { won: boolean; hero: string | null; deaths: number; takedowns: number; streak?: number },
): string {
  const { won, hero, deaths, takedowns, streak = 0, seed = takedowns + deaths } =
    opts as typeof opts & { seed?: number };
  const pick = (arr: string[]) => arr[Math.abs(seed) % arr.length];
  const h = hero ?? "ce héros";
  if (won) {
    if (streak >= 3) return pick([
      `Et de ${streak}, monsieur. La soirée vous appartient.`,
      `Série de ${streak}. Difficile de faire mieux.`,
      `${streak} d'affilée — je note la domination.`,
    ]);
    if (deaths === 0) return pick([
      `Aucune mort. Une partie de maître, monsieur.`,
      `Zéro mort — proprement exécuté.`,
    ]);
    if (takedowns >= 15) return pick([
      `Démonstration. ${h} était intenable.`,
      `${takedowns} participations — la carte vous a appartenu.`,
    ]);
    return pick([
      `Victoire nette. ${h} a fait le travail.`,
      `Bien joué, monsieur. On enchaîne ?`,
      `Une de plus au compteur.`,
    ]);
  }
  if (streak <= -3) return pick([
    `Série difficile, monsieur. Gardons la tête froide.`,
    `${Math.abs(streak)} revers d'affilée — une pause, peut-être ?`,
  ]);
  if (deaths >= 8) return pick([
    `Trop de morts, monsieur. On respire au prochain.`,
    `${deaths} morts — la prudence paiera la prochaine fois.`,
  ]);
  return pick([
    `Défaite serrée. ${h} méritait mieux.`,
    `On rebondit à la prochaine, monsieur.`,
    `Pas cette fois — mais la nuit est jeune.`,
  ]);
}

// ── musique (proxy Orpheus /api/now-playing) ─────────────────────────────────
export interface NowPlayingResp { authenticated?: boolean; current?: Record<string, unknown> }
export interface Track {
  playing: boolean;
  title?: string;
  artist?: string;
  art?: string;
  album?: string;
  durationMs?: number;
  progressMs?: number;
}
/** Le proxy enveloppe la réponse Orpheus dans `current`. Deux shapes possibles :
 *  - `/api/playback/now` (Spotify live) : `{ isPlaying, track:{ name, artists:[{name}], album:{images:[{url}]} } }`
 *  - ancien engine : `{ current:{ name, artist, albumArtUrl }, isPlaying }`
 *  Ce parseur gère les deux + variantes de champs. */
export function parseTrack(np: NowPlayingResp | undefined): Track {
  const o = (np?.current ?? {}) as Record<string, unknown>;
  const t = ((o.track as Record<string, unknown>) ?? (o.current as Record<string, unknown>) ?? o) as Record<string, unknown>;
  const str = (v: unknown) => (typeof v === "string" ? v : undefined);
  const title = str(t.name) ?? str(t.title) ?? str(t.track);
  // artiste : tableau Spotify [{name}] → joint, sinon string directe
  const artist = Array.isArray(t.artists)
    ? (t.artists as Array<Record<string, unknown>>).map((a) => str(a?.name)).filter(Boolean).join(", ") || undefined
    : str(t.artist) ?? str(t.artists) ?? str(t.author);
  // pochette : album.images[0].url (Spotify) sinon albumArtUrl (engine)
  const albumObj = t.album as Record<string, unknown> | undefined;
  const images = albumObj?.images;
  const art =
    (Array.isArray(images) ? str((images[0] as Record<string, unknown>)?.url) : undefined) ??
    str(t.albumArtUrl) ?? str(t.albumArt) ?? str(t.image);
  const num = (v: unknown) => (typeof v === "number" ? v : undefined);
  const album = str(albumObj?.name) ?? str(t.album);
  const durationMs = num(t.durationMs) ?? num(t.duration);
  const progressMs = num(o.progressMs);
  const isPlaying = o.isPlaying !== false; // absent → on suppose en lecture
  return { playing: !!(np?.authenticated && title && isPlaying), title, artist, art, album, durationMs, progressMs };
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
