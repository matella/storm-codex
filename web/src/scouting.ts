// Rapports de scouting : types de /api/scouting/* + helpers purs d'affichage (testés en vitest).
// Spec : docs/specs/2026-09-26-rapports-scouting-design.md.

export interface Rate { k: number; n: number }
export interface FactRef { label: string; text: string; n: number; low: boolean }
export interface HeroRow { id: string; hero: string; picks: Rate; record: Rate; by?: string[] }
export interface Count { id: string; key: string; count: Rate }
export interface AvgStats {
  kills: number; deaths: number; assists: number; kill_participation_pct: number;
  hero_damage_pm: number; siege_damage_pm: number; healing_pm: number; damage_taken_pm: number;
  xp_pm: number; time_dead_pct: number;
}
export interface PlayerFacts {
  pid: string; toon: string; name: string; core: boolean; record: Rate;
  heroes: HeroRow[]; roles: Count[]; stats: AvgStats;
}
export interface MapFacts {
  id: string; map: string; record: Rate; wilson: [number, number] | null;
  picks: HeroRow[]; bans: Count[]; bans_against: Count[];
}
export interface DraftFacts {
  games: number; first_pick: Rate; first_pick_record: Rate; second_pick_record: Rate;
  bans_first: Count[]; bans_mid: Count[]; bans_against: Count[];
  openers: Count[]; opener_roles: Count[]; last_picks: Count[];
  /** Héros joués contre eux : picks = parties affrontées, record = leur bilan dans ces parties. */
  faced?: HeroRow[];
}
export interface FlowFacts {
  first_to_10: Rate; first_to_10_record: Rate; first_fort: Rate; first_fort_record: Rate;
  first_objective: Rate; first_objective_record: Rate;
  level_diff: { id: string; minute: number; avg: number; n: number }[];
  length: { id: string; label: string; record: Rate }[];
  comebacks: Rate; throws: Rate;
}
export interface GamePick { hero: string; player: string }
export interface GameRow {
  id: string; gid: number; date: string | null; map: string; won: boolean; length_s: number;
  first_pick: boolean | null; opponents: string[]; picks: GamePick[]; opp_picks: GamePick[];
  bans: string[]; bans_against: string[];
}
export interface Facts {
  overview: {
    games: number; record: Rate; excluded: number; first_date: string | null;
    last_date: string | null; builds: number[]; avg_length_s: number;
  };
  players: PlayerFacts[]; maps: MapFacts[]; draft: DraftFacts; flow: FlowFacts;
  games: GameRow[]; index: Record<string, FactRef>;
}
export interface Candidate { toon: string; name: string; games: number }
export interface Snapshot {
  facts: Facts;
  detection: { candidates: Candidate[]; ambiguous: boolean };
  roster: string[];
  roster_auto: boolean;
}

/** Preuve résolue par le serveur contre les faits courants. */
export interface Evidence { id: string; known: boolean; label?: string; text?: string; n?: number; low?: boolean }
interface Cited { evidence: Evidence[]; unsupported: boolean }
export type HeroCall = Cited & { hero: string; phase: string | null; player: string | null; why: string };
export type MapCall = Cited & { map: string; why: string };
export type Point = Cited & { point: string };
export type MapPlan = Cited & {
  map: string; confidence: string | null; overview: string;
  bans: HeroCall[]; picks: HeroCall[]; their_picks: HeroCall[]; considerations: Point[];
};
/** Plan de draft importé (format v2) : choix de carte, plan par carte, plan général. */
export interface Analysis {
  model: string | null;
  summary: string;
  map_choice: { pick: MapCall[]; avoid: MapCall[] };
  maps: MapPlan[];
  general: { bans: HeroCall[]; picks: HeroCall[]; considerations: Point[] };
}

export type ReportStatus = "empty" | "ready" | "analyzed" | "stale";
export interface ScoutTeamPlayer { toon: string; name: string; hero: string }
export interface ScoutGame {
  gid: number; filename: string | null; played_at: string | null; map: string | null;
  length_s: number | null; winner: number | null;
  target_team: number | null; target_source: "roster" | "anchor" | "manual" | null;
  teams: [ScoutTeamPlayer[], ScoutTeamPlayer[]];
}
export interface ScoutingReport {
  id: number; title: string; target_name: string | null;
  roster: string[]; anchors: string[];
  facts_version: number; snapshot: Snapshot | null;
  analysis: Analysis | null; analysis_facts_version: number | null;
  analysis_model: string | null; analysis_imported_at: string | null;
  analysis_tally?: { claims: number; unsupported: number; unknown_ids: number };
  created_at: string; updated_at: string; status: ReportStatus; games: ScoutGame[];
}
export interface ScoutingListItem {
  id: number; title: string; target_name: string | null; created_at: string; updated_at: string;
  games: number; record: Rate | null; first_date: string | null; last_date: string | null;
  builds: number[] | null; analysis_imported_at: string | null; status: ReportStatus;
}

/** `7/9 · 78%` ; `—` sans effectif. */
export function fmtRate(r: Rate | null | undefined): string {
  if (!r || r.n === 0) return "—";
  return `${r.k}/${r.n} · ${Math.round((r.k * 100) / r.n)}%`;
}

/** Intervalle de Wilson affiché : `95% CI 44–100%`. */
export function fmtWilson(w: [number, number] | null | undefined): string {
  return w ? `95% CI ${Math.round(w[0])}–${Math.round(w[1])}%` : "";
}

/** Libellé + classe de badge d'un statut de rapport. */
export function statusMeta(s: ReportStatus): { label: string; cls: string } {
  switch (s) {
    case "empty": return { label: "no replays", cls: "b-qm" };
    case "ready": return { label: "pack ready", cls: "b-live" };
    case "analyzed": return { label: "analyzed", cls: "b-win" };
    case "stale": return { label: "analysis outdated", cls: "b-mvp" };
  }
}

/** État de preuve d'une affirmation : toutes connues / certaines inconnues / aucune connue. */
export function claimState(c: Cited): "supported" | "partial" | "unsupported" {
  if (c.unsupported) return "unsupported";
  return c.evidence.some((e) => !e.known) ? "partial" : "supported";
}

/** pN → nom (joueurs de l'équipe cible). */
export function pidNames(facts: Facts | null | undefined): Record<string, string> {
  const out: Record<string, string> = {};
  for (const p of facts?.players ?? []) out[p.pid] = p.name;
  return out;
}

/** Date courte `2026-07-22` (ou `?`). */
export function day(iso: string | null | undefined): string {
  return iso ? iso.slice(0, 10) : "?";
}

/** Résultat d'un dépôt de replay → libellé pour la file de dépôt. */
export function uploadLabel(r: { status?: string; error_class?: string; target_team?: number | null }): string {
  switch (r.status) {
    case "added": return r.target_team === null || r.target_team === undefined ? "added · team not found" : "added";
    case "duplicate": return "already in this report";
    case "parse_failed": return `could not read (${r.error_class ?? "error"})`;
    default: return r.status ?? "error";
  }
}

/** Côté de l'équipe cible dans une partie, pour l'affichage (`null` = non trouvé). */
export function targetNames(g: ScoutGame): string[] | null {
  if (g.target_team !== 0 && g.target_team !== 1) return null;
  return g.teams[g.target_team].map((p) => p.name);
}

/** Regroupe les faits par carte et les plans de l'analyse par carte (cartes des deux côtés, les
 *  cartes jouées d'abord, dans l'ordre des faits). Rapprochement insensible à la casse. */
export function mapsWithPlans(facts: Facts | null | undefined, plans: MapPlan[] | undefined): { map: string; facts: MapFacts | null; plan: MapPlan | null }[] {
  const key = (m: string) => m.trim().toLowerCase();
  const byKey = new Map((plans ?? []).map((p) => [key(p.map), p]));
  const out: { map: string; facts: MapFacts | null; plan: MapPlan | null }[] = (facts?.maps ?? []).map((m) => {
    const plan = byKey.get(key(m.map)) ?? null;
    byKey.delete(key(m.map));
    return { map: m.map, facts: m, plan };
  });
  for (const p of byKey.values()) out.push({ map: p.map, facts: null, plan: p });
  return out;
}
