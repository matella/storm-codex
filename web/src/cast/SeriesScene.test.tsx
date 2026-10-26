import { describe, expect, it } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";
import type { CastSeries, CastTeam, GameRecord, PoolMap } from "../castApi";
import { SeriesScene } from "./SeriesScene";

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
