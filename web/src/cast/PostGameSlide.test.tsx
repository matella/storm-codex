import { describe, expect, it } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";
import type { CastSeries, CastTeam, GameRecord, GameSummary, PlayerSummary } from "../castApi";
import { PostGameSlide } from "./PostGameSlide";

const team = (name: string): CastTeam => ({ name, logo: null, record: "", players: [], captain: null });
const p = (side: "blue" | "red", i: number, hero: string, v: Partial<PlayerSummary> = {}): PlayerSummary => ({
  side, toon: `t-${side}-${i}`, name: `${side}${i}`, hero, kills: 1, takedowns: 3, deaths: 2, assists: 2,
  hero_damage: 10_000 * i, siege_damage: 20_000 * i, healing: 0, ...v,
});
const summary =(over: Partial<GameSummary> = {}): GameSummary => ({
  duration_secs: 1576, winner: "blue", takedowns: [28, 15], team_level: [25, 24], structures: [5, 4],
  merc_camps: [12, 6], objectives: [3, 3], bans: [["Johanna", "Yrel"], ["Falstad"]],
  players: [
    p("blue", 1, "Hogger"), p("blue", 2, "Malfurion", { healing: 161_589 }), p("blue", 3, "Arthas"), p("blue", 4, "Maiev"), p("blue", 5, "Junkrat", { takedowns: 23 }),
    p("red", 1, "Blaze"), p("red", 2, "Garrosh"), p("red", 3, "Mephisto"), p("red", 4, "Alexstrasza", { healing: 137_188 }), p("red", 5, "Greymane"),
  ],
  xp_lead: [[60, 710], [120, 1259], [1583, 13120]],
  ...over,
});
const game = (sm: GameSummary | null): GameRecord => ({
  number: 2, map: "Tomb of the Spider Queen", winner: "blue", takedowns: [28, 15], duration_secs: 1576, summary: sm,
});
const series = (): CastSeries => ({
  schema_version: 1, series_id: "s1", active: true, blue: team("Alpha"), red: team("Omega"), bo: 5, format: "fearless",
  score_override: null, pool: [], games: [game(summary())], scene: "series", lineup_mode: "auto",
  slide: { game: null, index: 1, auto: false, interval_ms: 12000, since_ms: 0 },
});
const render = (sm: GameSummary, tab: Parameters<typeof PostGameSlide>[0]["tab"] = "hero_damage") =>
  renderToStaticMarkup(<PostGameSlide s={series()} game={game(sm)} summary={sm} tab={tab} />);

describe("PostGameSlide", () => {
  it("header: WIN/LOSS, takedown score, duration, game and map", () => {
    const html = render(summary());
    expect(html).toMatch(/Alpha[\s\S]*WIN[\s\S]*>28<[\s\S]*26:16[\s\S]*GAME 2 · TOMB OF THE SPIDER QUEEN[\s\S]*Omega[\s\S]*LOSS[\s\S]*>15</);
  });
  it("comparatives come from the summary; team K/D/A = player sums", () => {
    const html = render(summary());
    for (const label of ["K / D / A", "TEAM LEVEL", "FORTS · KEEPS", "MERC CAMPS", "MAP OBJECTIVES", "SIEGE DAMAGE", "HERO DAMAGE", "BANS"]) {
      expect(html).toContain(`>${label}<`);
    }
    expect(html).toContain(">5 / 10 / 10<");
    expect(html).toContain(">25<");
    expect(html).toContain(">12<");
  });
  it("a value absent from the summary removes its row (never shown as 0)", () => {
    const html = render(summary({ structures: null, merc_camps: null, objectives: null, team_level: null, bans: [[], []], xp_lead: [] }));
    for (const label of ["FORTS · KEEPS", "MERC CAMPS", "MAP OBJECTIVES", "TEAM LEVEL", "BANS"]) expect(html).not.toContain(label);
    expect(html).not.toContain("EXPERIENCE ADVANTAGE");
    expect(html).toContain(">K / D / A<");
  });
  it("tabs: the active one is lit and drives the bars", () => {
    const tabs = (h: string) => (h.match(/<span class="on">([^<]+)</) ?? [])[1];
    expect(tabs(render(summary(), "hero_damage"))).toBe("HERO DAMAGE");
    expect(tabs(render(summary(), "healing"))).toBe("HEALING");
    expect(tabs(render(summary(), "siege_damage"))).toBe("SIEGE");
    expect(tabs(render(summary(), "takedowns"))).toBe("TAKEDOWNS");
    expect(render(summary(), "healing")).toContain(">161.6k<");
    expect(render(summary(), "healing")).toContain(">137.2k<");
    expect(render(summary(), "takedowns")).toContain(">23<");
    // dégâts héros : barres triées par valeur, la plus haute pleine largeur
    const hd = render(summary(), "hero_damage");
    expect(hd).toMatch(/>50.0k<[\s\S]*width:100%/);
    expect((hd.match(/class="pg-dm"/g) ?? []).length).toBe(5);
  });
  it("xp advantage curve only with at least two samples", () => {
    expect(render(summary())).toContain("EXPERIENCE ADVANTAGE OVER TIME");
    expect(render(summary({ xp_lead: [[60, 10]] }))).not.toContain("EXPERIENCE ADVANTAGE");
  });
});
