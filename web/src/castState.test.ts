import { describe, expect, it } from "vitest";
import type { CastSeries, CastTeam, GameRecord, PoolMap } from "./castApi";
import { badgeGameNumber, displayName, effectiveLineupMode, mapCardView, seriesScore } from "./castState";

const game = (map: string, winner: GameRecord["winner"]): GameRecord => ({
  number: 1, map, winner, takedowns: [0, 0], duration_secs: null, summary: null,
});
const team = (name: string): CastTeam => ({ name, logo: null, record: "", players: [], captain: null });
const series = (over: Partial<CastSeries>): CastSeries => ({
  schema_version: 1, active: true, blue: team(""), red: team(""), bo: 5, format: "fearless",
  score_override: null, pool: [], games: [], scene: "hide", lineup_mode: "auto",
  slide: { game: null, index: 0, auto: false, interval_ms: 12000 }, ...over,
});

describe("mapCardView", () => {
  const picked: PoolMap = { name: "A", status: "picked", by: "blue" };
  it("played map won by blue", () => {
    expect(mapCardView(picked, [game("A", "blue")], null)).toMatchObject({ tone: "blue", label: "WON", struck: false });
  });
  it("won by red", () => {
    expect(mapCardView(picked, [game("A", "red")], null)).toMatchObject({ tone: "red", label: "WON" });
  });
  it("played map without winner yet is not WON", () => {
    expect(mapCardView(picked, [game("A", null)], null).label).toBe("PLAYING");
  });
  it("next map is gold NEXT", () => {
    expect(mapCardView(picked, [], "A")).toMatchObject({ tone: "gold", label: "NEXT", struck: false });
  });
  it("banned map is struck", () => {
    const v = mapCardView({ name: "B", status: "banned", by: "red" }, [], "A");
    expect(v).toMatchObject({ struck: true, label: "banned by Red" });
  });
  it("picked, decider, available", () => {
    expect(mapCardView(picked, [], "Z")).toMatchObject({ tone: "blue", label: "picked by Blue", struck: false });
    expect(mapCardView({ name: "D", status: "decider" }, [], "Z").label).toBe("decider");
    expect(mapCardView({ name: "E", status: "available" }, [], "Z")).toMatchObject({ tone: "none", label: "" });
  });
});

describe("effectiveLineupMode", () => {
  it("auto: A until a game has a winner, then B", () => {
    expect(effectiveLineupMode(series({}))).toBe("A");
    expect(effectiveLineupMode(series({ games: [game("A", null)] }))).toBe("A");
    expect(effectiveLineupMode(series({ games: [game("A", "blue")] }))).toBe("B");
  });
  it("forced mode wins", () => {
    const g = [game("A", "blue")];
    expect(effectiveLineupMode(series({ lineup_mode: "a", games: g }))).toBe("A");
    expect(effectiveLineupMode(series({ lineup_mode: "b" }))).toBe("B");
  });
});

describe("displayName", () => {
  it("falls back to the side label when empty", () => {
    expect(displayName(team(""), "blue")).toBe("Blue");
    expect(displayName(team("  "), "red")).toBe("Red");
    expect(displayName(team("Nexus"), "blue")).toBe("Nexus");
  });
});

describe("badgeGameNumber", () => {
  const g = (number: number, winner: GameRecord["winner"]): GameRecord => ({ ...game("M", winner), number });
  it("no games -> 1", () => expect(badgeGameNumber([])).toBe(1));
  it("game in progress -> its number", () => expect(badgeGameNumber([g(1, null)])).toBe(1));
  it("one finished game -> 2", () => expect(badgeGameNumber([g(1, "blue")])).toBe(2));
  it("third started without winner -> 3", () =>
    expect(badgeGameNumber([g(1, "blue"), g(2, "red"), g(3, null)])).toBe(3));
  it("all finished -> max+1", () => expect(badgeGameNumber([g(1, "blue"), g(2, "red"), g(3, "blue")])).toBe(4));
});

describe("seriesScore", () => {
  const g = (winner: GameRecord["winner"]) => game("M", winner);
  it("counts winners per side", () =>
    expect(seriesScore(series({ games: [g("blue"), g("blue"), g("red"), g(null)] }))).toEqual([2, 1]));
  it("override wins", () =>
    expect(seriesScore(series({ games: [g("red")], score_override: [3, 0] }))).toEqual([3, 0]));
});
