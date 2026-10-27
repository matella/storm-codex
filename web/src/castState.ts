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

/** Série décidée (R22) : un camp a atteint ceil(bo/2) victoires au score affiché (forçage compris). */
export function seriesDecided(s: CastSeries): boolean {
  const [b, r] = seriesScore(s);
  return Math.max(b, r) >= Math.ceil(s.bo / 2);
}

/** Numéro de partie de la pastille. R9 : partie en cours (dernière sans vainqueur) sinon max(number)+1
 *  (1 si aucune). R22 : série décidée → numéro de la dernière partie (jamais n+1). */
export function badgeGameNumber(s: CastSeries): number {
  const games = s.games;
  if (games.length === 0) return 1;
  const last = games[games.length - 1];
  const max = Math.max(...games.map((g) => g.number));
  if (last.winner === null || seriesDecided(s)) return max;
  return max + 1;
}

/** L'overlay rend une scène : série active, ou scène Draft (R19 : le draft s'édite sans série). */
export function overlayVisible(s: CastSeries | undefined | null): boolean {
  return !!s && (s.active || s.scene === "draft");
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

/** Vainqueur posable : la partie `n` existe et n'en a pas encore. */
export function canSetWinner(s: CastSeries, n: number): boolean {
  const g = s.games.find((x) => x.number === n);
  return g !== undefined && g.winner === null;
}

/** R21 : vainqueur saisi à la main corrigeable tant que la partie n'a pas de résumé de replay
 *  (le serveur refuse alors `WinnerAlreadySet`). */
export function canChangeWinner(s: CastSeries, n: number): boolean {
  const g = s.games.find((x) => x.number === n);
  return g !== undefined && g.winner !== null && !g.summary;
}

/** R21 : partie retirable = la dernière, sans vainqueur ni résumé (sinon 409 `NotLastGame` / `GameHasResult`). */
export function canRemoveGame(s: CastSeries, n: number): boolean {
  const g = s.games.find((x) => x.number === n);
  return g !== undefined && !s.games.some((x) => x.number > n) && g.winner === null && !g.summary;
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

/** Bornes (s) de l'intervalle du diaporama saisi dans la régie (R25) : dans la plage que le serveur
 *  accepte (`interval_ms` borné 500 ms–1 h), en secondes entières. */
export const INTERVAL_S = { min: 1, max: 3600 } as const;

/** Saisie de l'intervalle (s) → entier borné à `INTERVAL_S` ; `null` si ce n'est pas un nombre. */
export function intervalSeconds(raw: string): number | null {
  if (raw.trim() === "") return null;
  const v = Math.round(Number(raw));
  if (!Number.isFinite(v)) return null;
  return Math.min(INTERVAL_S.max, Math.max(INTERVAL_S.min, v));
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

/** Disposition du bandeau des cartes (scène Series) selon la taille du pool : ≤ 6 cartes = une rangée ;
 *  7–10 = deux rangées de cinq ; 11 et plus = rangées de cinq (trois pour 11–15) à cartes plus basses,
 *  pour que tout tienne dans le cadre 1920×1080 sans chevauchement. */
export function stripLayout(n: number): { cols: number; rows: number } {
  if (n <= 6) return { cols: Math.max(1, n), rows: 1 };
  return { cols: 5, rows: Math.ceil(n / 5) };
}

/** Libellé de la slide dans la régie : en auto, l'overlay OBS et l'aperçu font tourner chacun leur
 *  propre minuterie — la régie n'affirme donc pas quelle slide est à l'antenne. */
export function slideLabel(s: CastSeries): { text: string; title: string } {
  const n = slideCount(s);
  if (s.slide.auto && n > 1) {
    return {
      text: "auto",
      title: `Rotating ${n} slides every ${Math.round(s.slide.interval_ms / 1000)} s. The preview and the OBS source run independent timers, so they may show different slides; ‹ › set the starting slide.`,
    };
  }
  const i = slideIndex(s);
  return { text: `${i + 1} / ${n}`, title: SLIDE_NAMES[i] ?? "" };
}
export const SLIDE_NAMES = ["Maps", "Hero damage", "Healing", "Siege", "Takedowns"];

/** Réponse du dépôt de replay (résumé orienté + façon dont l'orientation a été décidée). */
export interface ReplayOrientation {
  orientation?: "evidence" | "assumed" | "forced";
  forced_against_evidence?: boolean;
}
/** Note visible après un dépôt : l'échange forcé contredit les joueurs liés (accepté, mais à vérifier). */
export function replayNote(r: ReplayOrientation): string | null {
  if (r.forced_against_evidence) {
    return "Sides swapped against the evidence: the linked players say this replay was NOT swapped. Check the winner and the player rows.";
  }
  return null;
}

/** Équipe saisie dans le rail, resynchronisée depuis le serveur (autre onglet, WS) : adoptée si la
 *  saisie locale n'a pas divergé de la dernière version serveur vue (`base`), sinon conservée. Une
 *  modification annulée (revenue à `base`) se resynchronise donc à nouveau. */
export function resyncTeam(local: CastTeam, base: CastTeam, server: CastTeam): CastTeam {
  return JSON.stringify(local) === JSON.stringify(base) ? server : local;
}

/** Navigation clavier de la barre de scènes (activation manuelle : les flèches déplacent seulement le
 *  focus — jamais une scène à l'antenne). `null` = touche sans effet. */
export function tabKeyTarget(i: number, key: string, n: number): number | null {
  switch (key) {
    case "ArrowRight": return (i + 1) % n;
    case "ArrowLeft": return (i + n - 1) % n;
    case "Home": return 0;
    case "End": return n - 1;
    default: return null;
  }
}

/** Texte d'un joueur en mode A dans l'aperçu de la régie, selon l'état de la requête des héros récents. */
export function modeAHint(linked: boolean, heroes: number, q: { isLoading: boolean; isError: boolean }): string | null {
  if (!linked) return "hidden (unlinked)";
  if (q.isLoading) return "loading…";
  if (q.isError) return "recent heroes unavailable (error)";
  return heroes > 0 ? null : "hidden (under 5 recent games)";
}

/** Une couche de l'overlay : la scène affichée et l'état de série qui la rend. */
export interface SceneLayer {
  key: CastScene;
  s: CastSeries;
}
/** Scène rendue par l'overlay (`null` = rien : hide, ou série inactive hors scène Draft). */
export function shownScene(s: CastSeries | undefined | null): SceneLayer | null {
  return s && overlayVisible(s) && s.scene !== "hide" ? { key: s.scene, s } : null;
}
/** Fondu de sortie : quand la scène change, l'ancienne couche reste montée (instantané de l'état qui
 *  la rendait — jamais la nouvelle série dans l'ancienne scène) le temps du fondu. Renvoie `st` tel
 *  quel si rien ne change (stable pour un setState pendant le rendu). */
export function sceneTransition(
  st: { cur: SceneLayer | null; leaving: SceneLayer | null },
  s: CastSeries | undefined | null,
): { cur: SceneLayer | null; leaving: SceneLayer | null } {
  const next = shownScene(s);
  if (next?.key === st.cur?.key) {
    if (next === null || next.s === st.cur?.s) return st;
    return { cur: next, leaving: st.leaving };
  }
  return { cur: next, leaving: st.cur };
}
