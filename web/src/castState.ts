// Utilitaires purs de la régie de cast (miroir de cast/series.rs : lineup_mode_effective, display_name).
import type { Side } from "./api";
import type { CastSeries, CastTeam, GameRecord, PoolMap } from "./castApi";

export interface MapCardView {
  tone: "blue" | "red" | "gold" | "muted" | "none";
  label: string;
  struck: boolean;
}

const sideLabel = (s: Side) => (s === "blue" ? "Blue" : "Red");

/** Rendu d'une carte du bandeau « Series ». Priorité : jouée (WON / PLAYING) > NEXT > statut du pool. */
export function mapCardView(m: PoolMap, games: GameRecord[], nextMap: string | null): MapCardView {
  const played = games.find((g) => g.map === m.name);
  if (played) {
    return played.winner
      ? { tone: played.winner, label: "WON", struck: false }
      : { tone: "muted", label: "PLAYING", struck: false };
  }
  if (nextMap !== null && m.name === nextMap) return { tone: "gold", label: "NEXT", struck: false };
  switch (m.status) {
    case "banned":
      return { tone: "muted", label: `banned by ${sideLabel(m.by)}`, struck: true };
    case "picked":
      return { tone: m.by, label: `picked by ${sideLabel(m.by)}`, struck: false };
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
