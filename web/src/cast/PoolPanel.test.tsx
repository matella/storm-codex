import { describe, expect, it } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";
import type { CastSeries, CastTeam, GameRecord } from "../castApi";
import { PoolPanel } from "./PoolPanel";

const team = (name: string): CastTeam => ({ name, logo: null, record: "", players: [], captain: null });
const g = (n: number, map: string, winner: GameRecord["winner"], summary: GameRecord["summary"] = null): GameRecord => ({
  number: n, map, winner, takedowns: [0, 0], duration_secs: null, summary,
});
const series = (games: GameRecord[]): CastSeries => ({
  schema_version: 1, series_id: "s1", active: true, blue: team("A"), red: team("B"), bo: 5, format: "fearless",
  score_override: null, scene: "series", lineup_mode: "auto", slide: { game: null, index: 0, auto: false, interval_ms: 12000, since_ms: 0 },
  pool: [{ name: "Cursed Hollow", status: "picked", by: "blue" }, { name: "Sky Temple", status: "picked", by: "red" }, { name: "Dragon Shire", status: "decider" }],
  games,
});
const run = async () => {};
const count = (html: string, needle: string) => html.split(needle).length - 1;

describe("PoolPanel misclick recovery (R21)", () => {
  it("offers Change winner on a manual winner and Remove game on the last unfinished game", () => {
    const html = renderToStaticMarkup(<PoolPanel series={series([g(1, "Cursed Hollow", "blue"), g(2, "Sky Temple", null)])} run={run} />);
    expect(count(html, ">Change winner<")).toBe(1);
    expect(count(html, ">Remove game<")).toBe(1);
  });
  it("no Change winner once a replay summary is loaded, no Remove game on a finished game", () => {
    const html = renderToStaticMarkup(
      <PoolPanel series={series([g(1, "Cursed Hollow", "blue", {} as GameRecord["summary"])])} run={run} />,
    );
    expect(html).not.toContain(">Change winner<");
    expect(html).not.toContain(">Remove game<");
  });
});
