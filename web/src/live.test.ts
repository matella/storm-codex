import { describe, expect, it } from "vitest";
import {
  abilityLabel,
  currentTalents,
  abilityLagSec,
  feedText,
  gameClock,
  isStale,
  recentBins,
  sortAbilities,
  splitTeams,
  type LivePlayer,
} from "./live";

const player = (id: number): LivePlayer => ({
  player_id: id,
  team: id > 5 ? 1 : 0,
  hero: `H${id}`,
  hero_unit: null,
  level: 1,
  talents: [],
  deaths: 0,
  takedowns: 0,
  abilities: null,
});

describe("gameClock", () => {
  it("counts from gates open", () => {
    expect(gameClock(610, 610)).toBe("0:00");
    expect(gameClock(610 + 16 * 75, 610)).toBe("1:15");
    expect(gameClock(610 - 16 * 12, 610)).toBe("-0:12");
  });
  it("falls back to loop 0 without gates", () => {
    expect(gameClock(16 * 61, null)).toBe("1:01");
  });
});

describe("abilityLagSec", () => {
  it("is null before any game events", () => {
    expect(abilityLagSec({ tracker_loop: 500, game_loop: 0, bytes: { tracker: 4096, game: 0 } })).toBeNull();
  });
  it("is the tracker lead in game seconds, never negative", () => {
    expect(abilityLagSec({ tracker_loop: 8643, game_loop: 7482, bytes: { tracker: 1, game: 1 } })).toBe(73);
    expect(abilityLagSec({ tracker_loop: 10, game_loop: 50, bytes: { tracker: 1, game: 1 } })).toBe(0);
  });
});

describe("isStale", () => {
  const now = Date.parse("2026-10-24T16:30:00Z");
  it("flags a silent live session after 60 s", () => {
    expect(isStale({ status: "live", last_chunk_at: "2026-10-24T16:28:00Z" }, now)).toBe(true);
    expect(isStale({ status: "live", last_chunk_at: "2026-10-24T16:29:30Z" }, now)).toBe(false);
  });
  it("never flags an ended game", () => {
    expect(isStale({ status: "ended", last_chunk_at: "2026-10-24T15:00:00Z" }, now)).toBe(false);
  });
});

describe("abilities", () => {
  it("labels unnamed abilities", () => {
    expect(abilityLabel({ link: 491, cmd: 0 })).toBe("#491");
    expect(abilityLabel({ link: 491, cmd: 2 })).toBe("#491·2");
  });
  it("sorts by presses then id", () => {
    const s = sortAbilities([
      { link: 9, cmd: 0, count: 2, recent: [] },
      { link: 3, cmd: 0, count: 5, recent: [] },
      { link: 1, cmd: 0, count: 2, recent: [] },
    ]);
    expect(s.map((a) => a.link)).toEqual([3, 1, 9]);
  });
  it("bins recent presses, newest in the last bin", () => {
    const now = 16 * 600;
    const bins = recentBins([now, now - 1, now - 16 * 179, now - 16 * 400], now, 36);
    expect(bins).toHaveLength(36);
    expect(bins[35]).toBe(2);
    expect(bins[0]).toBe(1);
    expect(bins.reduce((a, b) => a + b, 0)).toBe(3); // celle d'il y a 400 s est hors fenêtre
  });
});

describe("splitTeams", () => {
  it("splits 1–5 / 6–10 sorted", () => {
    const [a, b] = splitTeams([7, 2, 10, 1, 6].map(player));
    expect(a.map((p) => p.player_id)).toEqual([1, 2]);
    expect(b.map((p) => p.player_id)).toEqual([6, 7, 10]);
  });
});

describe("feedText", () => {
  const hero = (p: number) => `H${p}`;
  const talent = (t: string) => `«${t}»`;
  it("describes deaths with killers", () => {
    expect(feedText({ loop: 1, kind: "death", player: 4, team: 0, detail: { killers: [6, 9] } }, hero, talent)).toBe("H4 killed by H6, H9");
  });
  it("describes talents, levels, camps and objectives", () => {
    expect(feedText({ loop: 1, kind: "talent", player: 1, detail: { tree_id: "MonkTranscendence" } }, hero, talent)).toBe("H1 took «MonkTranscendence»");
    expect(feedText({ loop: 1, kind: "level", team: 1, detail: { level: 10 } }, hero, talent)).toBe("Team 2 reached level 10");
    expect(feedText({ loop: 1, kind: "camp", team: 0, detail: { camp: "Bruiser Camp" } }, hero, talent)).toBe("Team 1 captured Bruiser Camp");
    expect(feedText({ loop: 1, kind: "objective", detail: { name: "VolskayaCapturePointComplete" } }, hero, talent)).toBe("Volskaya Capture Point Complete");
  });
});

describe("currentTalents", () => {
  const tiers: Record<string, number> = { A1: 1, B1: 1, A4: 4, B4: 4, A7: 7 };
  const tierOf = (id: string) => tiers[id] ?? null;
  const t = (id: string, loop = 0) => ({ tree_id: id, loop });
  it("keeps picks in tier order", () => {
    expect(currentTalents([t("A1"), t("A4"), t("A7")], tierOf).map((x) => x.tree_id)).toEqual(["A1", "A4", "A7"]);
  });
  it("a talent reset replaces earlier picks tier by tier (Sandbox)", () => {
    const picks = [t("A1"), t("A4"), t("A7"), t("B1"), t("B4")];
    expect(currentTalents(picks, tierOf).map((x) => x.tree_id)).toEqual(["B1", "B4", "A7"]);
  });
  it("keeps unknown talents in arrival order, after known tiers", () => {
    expect(currentTalents([t("X"), t("A1"), t("Y")], tierOf).map((x) => x.tree_id)).toEqual(["A1", "X", "Y"]);
  });
});
