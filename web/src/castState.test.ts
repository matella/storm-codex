import { describe, expect, it } from "vitest";
import type { CastSeries, CastTeam, GameRecord, PoolMap } from "./castApi";
import { badgeGameNumber, canSetWinner, displayName, effectiveLineupMode, MAPS, mapCardView, reconcileTeam, replayErrorText, seriesScore, shownGame, slideCount, slideIndex, startableMaps, togglePool } from "./castState";

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

describe("mapCardView with team names", () => {
  const names = { blue: "Team Liquid", red: "Gen.G" };
  it("uses display names for banned / picked", () => {
    expect(mapCardView({ name: "B", status: "banned", by: "red" }, [], null, names).label).toBe("banned by Gen.G");
    expect(mapCardView({ name: "P", status: "picked", by: "blue" }, [], null, names)).toMatchObject({ tone: "blue", label: "picked by Team Liquid" });
  });
  it("WON / NEXT / decider unaffected", () => {
    expect(mapCardView({ name: "A", status: "picked", by: "blue" }, [game("A", "red")], null, names).label).toBe("WON");
    expect(mapCardView({ name: "D", status: "decider" }, [], "D", names).label).toBe("NEXT");
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

describe("form helpers", () => {
  const pm = (name: string, o: Partial<PoolMap> = {}): PoolMap => ({ name, status: "available", ...o }) as PoolMap;
  it("togglePool adds then removes, keeping a stable order", () => {
    expect(togglePool(["A"], "B")).toEqual(["A", "B"]);
    expect(togglePool(["A", "B", "C"], "B")).toEqual(["A", "C"]);
    expect(togglePool([], "A")).toEqual(["A"]);
  });
  it("togglePool does not mutate its input", () => {
    const a = ["A"];
    togglePool(a, "B");
    expect(a).toEqual(["A"]);
  });
  it("startableMaps = picked/decider maps without a game", () => {
    const s = series({
      pool: [pm("A", { status: "picked", by: "blue" }), pm("B", { status: "banned", by: "red" }), pm("C", { status: "decider" }), pm("D")],
      games: [game("A", null)],
    });
    expect(startableMaps(s)).toEqual(["C"]);
  });
  it("canSetWinner only for an existing game without winner", () => {
    const s = series({ games: [{ ...game("A", "blue"), number: 1 }, { ...game("B", null), number: 2 }] });
    expect(canSetWinner(s, 1)).toBe(false);
    expect(canSetWinner(s, 2)).toBe(true);
    expect(canSetWinner(s, 3)).toBe(false);
  });
  it("MAPS is a single, duplicate-free list", () => {
    expect(new Set(MAPS).size).toBe(MAPS.length);
    expect(MAPS).toContain("Dragon Shire");
  });
});

describe("reconcileTeam", () => {
  const mk = (over: Partial<CastTeam>): CastTeam => ({ ...team("Old"), ...over });
  it("keeps dirty local edits but adopts a newly uploaded server logo", () => {
    const local = mk({ name: "Typed", record: "2-0", logo: null });
    const server = mk({ name: "Old", logo: "/api/cast/logo/blue?v=2" });
    const r = reconcileTeam(local, server, false);
    expect(r.name).toBe("Typed");
    expect(r.record).toBe("2-0");
    expect(r.logo).toBe("/api/cast/logo/blue?v=2");
  });
  it("never lets Save send a logo older than the server's (untouched logo field)", () => {
    const r = reconcileTeam(mk({ logo: "/old" }), mk({ logo: "/new" }), false);
    expect(r.logo).toBe("/new");
  });
  it("keeps a logo URL the operator typed", () => {
    expect(reconcileTeam(mk({ logo: "https://x/y.png" }), mk({ logo: "/new" }), true).logo).toBe("https://x/y.png");
  });
});

describe("slides (R15: 0 = map strip, 1..4 = breakdown tabs when the shown game has a summary)", () => {
  const withSummary = (n: number, map: string): GameRecord => ({ ...game(map, "blue"), number: n, summary: {} as GameRecord["summary"] });
  const plain = (n: number, map: string): GameRecord => ({ ...game(map, "red"), number: n });
  it("shown game: pinned if it exists, else the latest; unknown pin = none", () => {
    const games = [withSummary(1, "A"), plain(2, "B")];
    expect(shownGame(series({ games }))?.number).toBe(2);
    expect(shownGame(series({ games, slide: { game: 1, index: 0, auto: false, interval_ms: 1000 } }))?.number).toBe(1);
    expect(shownGame(series({ games, slide: { game: 9, index: 0, auto: false, interval_ms: 1000 } }))).toBeNull();
    expect(shownGame(series({}))).toBeNull();
  });
  it("count: 5 with a summary, 1 without (latest game has none, or no game)", () => {
    expect(slideCount(series({ games: [withSummary(1, "A")] }))).toBe(5);
    expect(slideCount(series({ games: [withSummary(1, "A"), plain(2, "B")] }))).toBe(1);
    expect(slideCount(series({ games: [withSummary(1, "A"), plain(2, "B")], slide: { game: 1, index: 0, auto: false, interval_ms: 1000 } }))).toBe(5);
    expect(slideCount(series({}))).toBe(1);
  });
  it("manual index is the server index modulo the slide count; tick is ignored", () => {
    const s = (index: number) => series({ games: [withSummary(1, "A")], slide: { game: null, index, auto: false, interval_ms: 1000 } });
    expect(slideIndex(s(3), 7)).toBe(3);
    expect(slideIndex(s(6))).toBe(1);
    expect(slideIndex(series({ games: [plain(1, "A")], slide: { game: null, index: 4, auto: false, interval_ms: 1000 } }))).toBe(0);
  });
  it("auto starts at the server index and cycles every available slide", () => {
    const s = series({ games: [withSummary(1, "A")], slide: { game: null, index: 3, auto: true, interval_ms: 1000 } });
    expect([0, 1, 2, 3, 4, 5].map((t) => slideIndex(s, t))).toEqual([3, 4, 0, 1, 2, 3]);
  });
});

describe("replayErrorText (R16)", () => {
  it("side_mismatch gets the Swap sides hint; other codes pass through", () => {
    expect(replayErrorText("side_mismatch")).toContain('"Swap sides"');
    expect(replayErrorText("side_mismatch")).toMatch(/^side_mismatch/);
    expect(replayErrorText("not_custom")).toBe("not_custom");
  });
});
