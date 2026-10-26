import { describe, expect, it } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";
import type { CastSeries, CastTeam, GameRecord, GameSummary, PoolMap } from "../castApi";
import { SeriesScene } from "./SeriesScene";
import { Scene } from "../pages/CastOverlay";

const team = (name: string): CastTeam => ({ name, logo: null, record: "", players: [], captain: null });
const g = (n: number, map: string, winner: GameRecord["winner"]): GameRecord => ({
  number: n, map, winner, takedowns: [12, 9], duration_secs: 900, summary: null,
});
const pool: PoolMap[] = [
  { name: "Cursed Hollow", status: "picked", by: "blue" },
  { name: "Dragon Shire", status: "picked", by: "red" },
  { name: "Sky Temple", status: "picked", by: "blue" },
  { name: "Hanamura Temple", status: "decider" },
  { name: "Volskaya Foundry", status: "banned", by: "red" },
  { name: "Infernal Shrines", status: "banned", by: "blue" },
];
const series = (over: Partial<CastSeries> = {}): CastSeries => ({
  schema_version: 1, active: true, blue: team("Alpha"), red: team("Omega"), bo: 5, format: "fearless",
  score_override: null, pool, games: [g(1, "Cursed Hollow", "blue"), g(2, "Dragon Shire", "red")],
  scene: "series", lineup_mode: "auto", slide: { game: null, index: 0, auto: false, interval_ms: 12000 }, ...over,
});

const count = (html: string, needle: string) => html.split(needle).length - 1;

describe("SeriesScene", () => {
  const html = renderToStaticMarkup(<SeriesScene s={series()} />);
  it("one NEXT, two WON with team tones, two struck cards", () => {
    expect(count(html, ">NEXT<")).toBe(1);
    expect(count(html, ">WON<")).toBe(2);
    expect(html).toMatch(/class="mc[^"]*tone-blue[^"]*"[^>]*>(?:(?!<\/article>).)*>WON</s);
    expect(html).toMatch(/class="mc[^"]*tone-red[^"]*"[^>]*>(?:(?!<\/article>).)*>WON</s);
    expect(count(html, "struck")).toBe(2);
    expect(count(html, ">decider<")).toBe(1);
  });
  it("labels use team names; header shows names and series score", () => {
    expect(html).toContain("banned by Omega");
    expect(html).toContain("banned by Alpha");
    expect(html).toContain("Alpha");
    expect(html).toContain("1 – 1");
  });
  it("cards carry no score digits", () => {
    const cards = html.match(/<article[\s\S]*?<\/article>/g) ?? [];
    expect(cards.length).toBe(6);
    for (const c of cards) {
      const text = c.replace(/<[^>]*>/g, " ");
      expect(text).not.toMatch(/\d/);
    }
  });
  it("10-map pool wraps into two rows of five", () => {
    const many: PoolMap[] = Array.from({ length: 10 }, (_, i) => ({ name: `Map${String.fromCharCode(65 + i)}`, status: "available" }));
    const h = renderToStaticMarkup(<SeriesScene s={series({ pool: many, games: [] })} />);
    expect(h).toContain("two-rows");
    expect(renderToStaticMarkup(<SeriesScene s={series()} />)).not.toContain("two-rows");
  });
});

describe("Scene backdrop", () => {
  it("series scene paints the opaque backdrop, ingame and hide do not", () => {
    expect(renderToStaticMarkup(<Scene s={series()} />)).toContain("scene-backdrop");
    expect(renderToStaticMarkup(<Scene s={series({ scene: "ingame" })} />)).not.toContain("scene-backdrop");
    expect(renderToStaticMarkup(<Scene s={series({ scene: "hide" })} />)).not.toContain("scene-backdrop");
  });
});

describe("SeriesScene slides (R15)", () => {
  const sm: GameSummary = {
    duration_secs: 900, winner: "red", takedowns: [9, 12], team_level: [18, 20], structures: null, merc_camps: [3, 4],
    objectives: [1, 2], bans: [[], []], xp_lead: [],
    players: [{ side: "blue", toon: "t1", name: "p1", hero: "Valla", kills: 3, takedowns: 6, deaths: 4, assists: 3, hero_damage: 40000, siege_damage: 50000, healing: 0 }],
  };
  const withSummary = (index: number, extra: Partial<CastSeries> = {}) =>
    series({ games: [g(1, "Cursed Hollow", "blue"), { ...g(2, "Dragon Shire", "red"), summary: sm }], slide: { game: null, index, auto: false, interval_ms: 12000 }, ...extra });
  it("slide 0 is the map strip", () => {
    const h = renderToStaticMarkup(<SeriesScene s={withSummary(0)} />);
    expect(count(h, "<article")).toBe(6);
    expect(h).not.toContain("pg-hd");
  });
  it("slides 1..4 replace the strip with the breakdown of the shown game, tab = index", () => {
    const h = renderToStaticMarkup(<SeriesScene s={withSummary(2)} />);
    expect(count(h, "<article")).toBe(0);
    expect(h).toContain("pg-hd");
    expect(h).toContain('<span class="on">HEALING</span>');
    expect(h).toContain("GAME 2 · DRAGON SHIRE");
    expect(renderToStaticMarkup(<SeriesScene s={withSummary(4)} />)).toContain('<span class="on">TAKEDOWNS</span>');
  });
  it("without a summary only the strip exists (index taken modulo 1)", () => {
    const h = renderToStaticMarkup(<SeriesScene s={withSummary(3, { slide: { game: 1, index: 3, auto: false, interval_ms: 12000 } })} />);
    expect(count(h, "<article")).toBe(6);
    expect(h).not.toContain("pg-hd");
  });
  it("auto mode starts at the server index", () => {
    const h = renderToStaticMarkup(<SeriesScene s={withSummary(1, { slide: { game: null, index: 1, auto: true, interval_ms: 12000 } })} />);
    expect(h).toContain('<span class="on">HERO DAMAGE</span>');
  });
});
