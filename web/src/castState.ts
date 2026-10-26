// Utilitaires purs de la régie de cast (miroir de cast/series.rs : lineup_mode_effective, display_name).
import type { Side } from "./api";
import type { CastSeries, CastTeam, GameRecord, PoolMap } from "./castApi";

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

/** Forme lue (tolérante) de `GameRecord.summary.players` : `GameSummary` est encore `{}` côté TS
 *  (la tâche 9 l'étend) ; on lit donc de façon défensive, sans rien supposer d'autre que side/name/hero. */
interface SummaryPlayerLike {
  side?: string;
  name?: string;
  hero?: string;
}
const summaryPlayers = (g: GameRecord): SummaryPlayerLike[] => {
  const p = (g.summary as { players?: unknown } | null)?.players;
  return Array.isArray(p) ? (p as SummaryPlayerLike[]) : [];
};

/** Héros joués dans la série, par joueur (indice d'équipe 0-4), dans l'ordre des parties. Une ligne
 *  de résumé est rattachée à un joueur par son pseudo (insensible à la casse), sinon aux places
 *  restantes de l'équipe dans l'ordre. Parties sans résumé : ignorées. */
export function seriesPlayerHeroes(s: CastSeries, side: Side): PlayedHero[][] {
  const team = side === "blue" ? s.blue : s.red;
  const out: PlayedHero[][] = team.players.map(() => []);
  for (const g of s.games) {
    const rows = summaryPlayers(g).filter((r) => r.side === side && typeof r.hero === "string" && r.hero !== "");
    const taken = new Set<number>();
    const slotOf = rows.map((r) => {
      const name = (r.name ?? "").trim().toLowerCase();
      const i = name === "" ? -1 : team.players.findIndex((p, j) => !taken.has(j) && p.name.trim().toLowerCase() === name);
      if (i >= 0) taken.add(i);
      return i;
    });
    rows.forEach((r, k) => {
      let i = slotOf[k];
      if (i < 0) {
        i = team.players.findIndex((_, j) => !taken.has(j));
        if (i >= 0) taken.add(i);
      }
      if (i >= 0) out[i].push({ game: g.number, hero: r.hero as string });
    });
  }
  return out;
}
