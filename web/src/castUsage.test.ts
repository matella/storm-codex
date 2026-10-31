// Ergonomie de la régie : URL OBS, messages d'erreur en clair, étapes et prochaine action.
import { describe, expect, it } from "vitest";
import { obsOverlayUrl } from "./cast/ObsSource";
import { friendlyError, nextHint, setupSteps } from "./castState";
import type { CastSeries, CastTeam } from "./castApi";

const team = (name: string, named = true): CastTeam => ({
  name: named ? name : "", logo: null, record: "", captain: null,
  players: Array.from({ length: 5 }, (_, i) => ({ name: named ? `p${i}` : "", tagline: "", toon: null })),
});

const base = (over: Partial<CastSeries> = {}): CastSeries =>
  ({ active: false, blue: team("Blue", false), red: team("Red", false), games: [], ...over }) as unknown as CastSeries;

describe("OBS source URL", () => {
  it("builds the cast overlay URL for a skin", () => {
    expect(obsOverlayUrl("http://box:8080", "glass")).toBe("http://box:8080/cast/overlay?skin=glass");
  });
});

describe("friendly errors", () => {
  it("turns known codes into plain sentences", () => {
    expect(friendlyError("NotInPool")).toMatch(/pool/);
    expect(friendlyError("SeriesInactive")).toMatch(/No series/);
  });
  it("keeps unknown codes as they are", () => {
    expect(friendlyError("some_new_code")).toBe("some_new_code");
  });
});

describe("setup steps and next hint", () => {
  it("a fresh régie points to the series first", () => {
    expect(setupSteps(base()).map((x) => x.done)).toEqual([false, false, false]);
    expect(nextHint(base())).toMatch(/New series/);
  });
  it("an active series with named teams moves on to maps and games", () => {
    const s = base({ active: true, blue: team("Blue"), red: team("Red"), games: [] });
    expect(setupSteps(s).map((x) => x.done)).toEqual([true, true, false]);
    expect(nextHint(s)).toMatch(/Start game/);
  });
  it("no hint once everything is done", () => {
    const s = base({ active: true, blue: team("Blue"), red: team("Red"), games: [{ number: 1 } as never] });
    expect(nextHint(s)).toBeNull();
  });
});
