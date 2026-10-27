import { describe, expect, it } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";
import type { CastSeries, CastTeam } from "../castApi";
import { InGameScene } from "./InGameScene";

const team = (name: string): CastTeam => ({ name, logo: null, record: "", players: [], captain: null });
const series = (over: Partial<CastSeries> = {}): CastSeries => ({
  schema_version: 1, series_id: "s1", active: true, blue: team(""), red: team(""), bo: 5, format: "fearless",
  score_override: null, pool: [], games: [], scene: "ingame", lineup_mode: "auto",
  slide: { game: null, index: 0, auto: false, interval_ms: 12000 }, ...over,
});

describe("InGameScene", () => {
  it("falls back to Blue / Red and shows badge", () => {
    const html = renderToStaticMarkup(<InGameScene s={series()} />);
    expect(html).toContain(">Blue<");
    expect(html).toContain(">Red<");
    expect(html).toContain("BO5");
    expect(html).toContain("GAME 1");
    expect(html).toContain("FEARLESS");
  });
  it("long name goes in an ellipsis element inside a max-width-bounded tab", () => {
    const long = "X".repeat(40);
    const html = renderToStaticMarkup(<InGameScene s={series({ blue: team(long) })} />);
    expect(html).toContain(long);
    expect(html).toContain('class="n"');
    expect(html).toContain("max-width:26.00%");
  });
  it("score override wins; otherwise counts winners; game number follows games", () => {
    const g = (n: number, w: "blue" | "red" | null) => ({ number: n, map: "M", winner: w, takedowns: [0, 0] as [number, number], duration_secs: null, summary: null });
    const html = renderToStaticMarkup(<InGameScene s={series({ games: [g(1, "blue"), g(2, "blue"), g(3, "red")] })} />);
    expect(html).toContain('<span class="s">2</span>');
    expect(html).toContain('<span class="s">1</span>');
    expect(html).toContain("GAME 4");
    // R22 : Bo3 décidée 2-1 → dernière partie, jamais GAME 4
    const done = renderToStaticMarkup(<InGameScene s={series({ bo: 3, games: [g(1, "blue"), g(2, "red"), g(3, "red")] })} />);
    expect(done).toContain("GAME 3");
    expect(done).not.toContain("GAME 4");
    const ip = renderToStaticMarkup(<InGameScene s={series({ games: [g(1, null)] })} />);
    expect(ip).toContain("GAME 1");
    expect(ip).not.toContain("GAME 2");
    const o = renderToStaticMarkup(<InGameScene s={series({ score_override: [3, 0] })} />);
    expect(o).toContain('<span class="s">3</span>');
  });
});
