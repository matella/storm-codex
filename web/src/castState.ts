// Utilitaires purs de la régie de cast (miroir de cast/series.rs : lineup_mode_effective, display_name).
import type { Side } from "./api";
import type { CastScene, CastSeries, CastTeam, GameRecord, PoolMap } from "./castApi";

export interface MapCardView {
  tone: "blue" | "red" | "gold" | "muted" | "none";
  label: string;
  struck: boolean;
}

const sideLabel = (s: Side) => (s === "blue" ? "Blue" : "Red");

/** Rendu d'une carte du bandeau « Series ». Priorité : jouée (WON / PLAYING) > NEXT > statut du pool.
 *  `names` (optionnel) : noms affichés des équipes pour « banned/picked by … » ; sinon « Blue » / « Red ». */
export function mapCardView(
  m: PoolMap,
  games: GameRecord[],
  nextMap: string | null,
  names?: { blue: string; red: string },
): MapCardView {
  const by = (s: Side) => (names ? names[s] : sideLabel(s));
  const played = games.find((g) => g.map === m.name);
  if (played) {
    return played.winner
      ? { tone: played.winner, label: "WON", struck: false }
      : { tone: "muted", label: "PLAYING", struck: false };
  }
  if (nextMap !== null && m.name === nextMap) return { tone: "gold", label: "NEXT", struck: false };
  switch (m.status) {
    case "banned":
      return { tone: "muted", label: `banned by ${by(m.by)}`, struck: true };
    case "picked":
      return { tone: m.by, label: `picked by ${by(m.by)}`, struck: false };
    case "decider":
      return { tone: "none", label: "decider", struck: false };
    default:
      return { tone: "none", label: "", struck: false };
  }
}

/** Mode de lineup effectif : forcé tel quel ; en `auto`, A tant qu'aucune partie n'a de vainqueur, puis B. */
export function effectiveLineupMode(s: CastSeries): "A" | "B" {
  if (s.lineup_mode === "a") return "A";
  if (s.lineup_mode === "b") return "B";
  return s.games.some((g) => g.winner !== null) ? "B" : "A";
}

/** Nom affiché d'une équipe ; vide (ou blancs) → « Blue » / « Red ». */
export function displayName(t: CastTeam, side: Side): string {
  return t.name.trim() === "" ? sideLabel(side) : t.name;
}

/** Score de série affiché [blue, red] : le forçage opérateur l'emporte, sinon décompte des vainqueurs. */
export function seriesScore(s: CastSeries): [number, number] {
  if (s.score_override) return s.score_override;
  return [
    s.games.filter((g) => g.winner === "blue").length,
    s.games.filter((g) => g.winner === "red").length,
  ];
}

/** Numéro de partie de la pastille : partie en cours (dernière sans vainqueur) sinon max(number)+1 (1 si aucune). */
export function badgeGameNumber(games: GameRecord[]): number {
  if (games.length === 0) return 1;
  const last = games[games.length - 1];
  if (last.winner === null) return last.number;
  return Math.max(...games.map((g) => g.number)) + 1;
}

/** Cartes sélectionnables (liste unique, partagée avec la console Draft). */
export const MAPS = [
  "Alterac Pass", "Battlefield of Eternity", "Braxis Holdout", "Blackheart's Bay",
  "Cursed Hollow", "Dragon Shire", "Garden of Terror", "Hanamura Temple",
  "Infernal Shrines", "Sky Temple", "Tomb of the Spider Queen", "Towers of Doom",
  "Volskaya Foundry", "Warhead Junction",
];

/** Ajoute ou retire `map` du pool en cours de saisie ; sans mutation, ordre des autres entrées conservé. */
export function togglePool(names: string[], map: string): string[] {
  return names.includes(map) ? names.filter((n) => n !== map) : [...names, map];
}

/** Cartes que « Start game » peut lancer : pickées ou decider, sans partie associée (ordre du pool). */
export function startableMaps(s: CastSeries): string[] {
  return s.pool
    .filter((m) => (m.status === "picked" || m.status === "decider") && !s.games.some((g) => g.map === m.name))
    .map((m) => m.name);
}

/** Vainqueur posable : la partie `n` existe et n'en a pas encore (le serveur refuse WinnerAlreadySet). */
export function canSetWinner(s: CastSeries, n: number): boolean {
  const g = s.games.find((x) => x.number === n);
  return g !== undefined && g.winner === null;
}

/** Réconcilie la saisie locale d'une équipe avec l'état serveur : les modifications locales (nom, joueurs…)
 *  sont conservées, mais le logo du serveur (envoi de fichier) l'emporte sur un logo local plus ancien.
 *  `touchedLogo` = l'opérateur a lui-même édité le champ URL du logo depuis la dernière synchro. */
export function reconcileTeam(local: CastTeam, server: CastTeam, touchedLogo: boolean): CastTeam {
  return touchedLogo ? local : { ...local, logo: server.logo };
}

/** Héros joué par un joueur de la série à une partie donnée. */
export interface PlayedHero {
  game: number;
  hero: string;
}

/** Forme lue (tolérante) de `GameRecord.summary.players` (`PlayerSummary`) : un état persisté ou
 *  partiel ne doit jamais casser l'overlay, on lit donc de façon défensive. */
interface SummaryPlayerLike {
  side?: string;
  name?: string;
  toon?: string;
  hero?: string;
}
const summaryPlayers = (g: GameRecord): SummaryPlayerLike[] => {
  const p = (g.summary as { players?: unknown } | null)?.players;
  return Array.isArray(p) ? (p as SummaryPlayerLike[]) : [];
};

/** Héros joués dans la série, par joueur (indice d'équipe 0-4), dans l'ordre des parties. R14 : une
 *  ligne de résumé (de la même équipe) n'est attribuée à un joueur que par (1) toon lié identique,
 *  puis (2) pseudo identique (trim, insensible à la casse), sauf si joueur et ligne ont chacun un
 *  toon et qu'ils diffèrent (homonyme d'un autre compte) — JAMAIS par position. Joueur sans
 *  ligne correspondante : liste vide (état « en attente »). Parties sans résumé : ignorées. */
export function seriesPlayerHeroes(s: CastSeries, side: Side): PlayedHero[][] {
  const team = side === "blue" ? s.blue : s.red;
  const out: PlayedHero[][] = team.players.map(() => []);
  for (const g of s.games) {
    const rows = summaryPlayers(g).filter((r) => r.side === side && typeof r.hero === "string" && r.hero !== "");
    const used = new Set<number>();
    const attach = (i: number, k: number) => {
      used.add(k);
      out[i].push({ game: g.number, hero: rows[k].hero as string });
    };
    const done = new Set<number>();
    team.players.forEach((p, i) => {
      if (!p.toon) return;
      const k = rows.findIndex((r, k) => !used.has(k) && r.toon === p.toon);
      if (k >= 0) { attach(i, k); done.add(i); }
    });
    team.players.forEach((p, i) => {
      const name = p.name.trim().toLowerCase();
      if (done.has(i) || name === "") return;
      const k = rows.findIndex(
        (r, k) => !used.has(k) && !(p.toon && r.toon && r.toon !== p.toon) && (r.name ?? "").trim().toLowerCase() === name,
      );
      if (k >= 0) attach(i, k);
    });
  }
  return out;
}

/** Partie montrée par la scène Series : épinglée (`slide.game`, si elle existe) sinon la dernière. */
export function shownGame(s: CastSeries): GameRecord | null {
  if (s.slide.game !== null) return s.games.find((g) => g.number === s.slide.game) ?? null;
  return s.games[s.games.length - 1] ?? null;
}

/** Onglets du post-game breakdown = slides 1..4 (0 = bandeau des cartes). */
export const BREAKDOWN_TABS = ["hero_damage", "healing", "siege_damage", "takedowns"] as const;
export type BreakdownTab = (typeof BREAKDOWN_TABS)[number];

/** Slides disponibles : le bandeau seul, + les 4 onglets du breakdown si la partie montrée a un résumé. */
export function slideCount(s: CastSeries): number {
  return shownGame(s)?.summary ? 1 + BREAKDOWN_TABS.length : 1;
}

/** Slide affichée : manuel = `slide.index` du serveur modulo le nombre de slides ; auto = départ à
 *  `slide.index`, avancée de `tick` (minuterie de l'overlay, `interval_ms`). */
export function slideIndex(s: CastSeries, tick = 0): number {
  const n = slideCount(s);
  return (s.slide.index + (s.slide.auto ? tick : 0)) % n;
}

/** Message affiché à l'opérateur pour un refus du dépôt de replay ; `side_mismatch` (R16) reçoit
 *  une consigne, les autres codes serveur restent tels quels. */
export function replayErrorText(code: string): string {
  if (code === "side_mismatch") {
    return `side_mismatch: the replay's winner contradicts the recorded winner and no linked player tells the sides apart. Tick "Swap sides" if this series' Blue team played as the replay's second team, otherwise check the recorded winner.`;
  }
  return code;
}

const SCENE_NAMES: readonly CastScene[] = ["lineups", "series", "draft", "ingame", "hide"];

/** Scène locale demandée par `?scene=` (cible de la redirection `/draft` → `/cast?scene=draft`) ; tout autre
 *  valeur est ignorée. Ne change jamais la scène à l'antenne. */
export function sceneFromSearch(search: string): CastScene | null {
  const v = new URLSearchParams(search).get("scene");
  return SCENE_NAMES.find((s) => s === v) ?? null;
}
