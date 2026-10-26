import { describe, expect, it } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";
import type { CastLineup, CastPlayer, CastSeries, CastTeam, GameRecord } from "../castApi";
import { seriesPlayerHeroes } from "../castState";
import { LineupsScene } from "./LineupsScene";
import { pickPatch, linkLabel } from "./PlayerPicker";

const pl = (name: string, toon: string | null = null): CastPlayer => ({ name, tagline: "", toon });
const team = (name: string, ps: CastPlayer[]): CastTeam => ({ name, logo: null, record: "4-1", players: ps, captain: 0 });
const five = (p: string, linked = true): CastPlayer[] =>
  [1, 2, 3, 4, 5].map((i) => pl(`${p}${i}`, linked ? `toon-${p}${i}` : null));
const g = (n: number, winner: GameRecord["winner"], summary: unknown = null): GameRecord => ({
  number: n, map: "Sky Temple", winner, takedowns: [0, 0], duration_secs: 600, summary: summary as GameRecord["summary"],
});
const series = (over: Partial<CastSeries> = {}): CastSeries => ({
  schema_version: 1, active: true, blue: team("Alpha", five("B")), red: team("Omega", five("R")), bo: 5,
  format: "fearless", score_override: null, pool: [], games: [], scene: "lineups", lineup_mode: "auto",
  slide: { game: null, index: 0, auto: false, interval_ms: 12000 }, ...over,
});
const heroes = (): CastLineup => ({
  blue: [[{ hero: "Valla", games: 9 }, { hero: "Jaina", games: 6 }, { hero: "Diablo", games: 3 }], [], [], [], []],
  red: [[], [], [], [], []],
});
const summaryOf = (blue: string[], red: string[]) => ({
  players: [
    ...blue.map((hero, i) => ({ side: "blue", name: `B${i + 1}`, hero })),
    ...red.map((hero, i) => ({ side: "red", name: `R${i + 1}`, hero })),
  ],
});
const count = (html: string, needle: string) => html.split(needle).length - 1;

describe("LineupsScene mode A", () => {
  const html = renderToStaticMarkup(<LineupsScene s={series()} heroes={heroes()} />);
  it("shows 3 recent hero portraits for a linked player and nothing for empty lists", () => {
    expect(html).toContain('data-mode="A"');
    expect(count(html, 'title="Valla')).toBe(1);
    expect(count(html, "lu-hp")).toBe(1); // les autres joueurs : liste vide → pas de rangée
    expect(count(html, "hp-i")).toBe(3);
  });
  it("shows no percentage and no win wording", () => {
    expect(html).not.toContain("%");
    expect(html.toLowerCase()).not.toMatch(/win/);
  });
  it("a player without toon gets no hero row even if the server list has entries", () => {
    const s = series({ blue: team("Alpha", five("B", false)) });
    const h = renderToStaticMarkup(<LineupsScene s={s} heroes={heroes()} />);
    expect(h).not.toContain("lu-hp");
  });
  it("shows names, record, captain marker; blank team falls back to Blue", () => {
    expect(html).toContain("Alpha");
    expect(html).toContain("4-1");
    expect(count(html, "CAPTAIN")).toBe(2);
    expect(renderToStaticMarkup(<LineupsScene s={series({ blue: team("", five("B")) })} />)).toContain(">Blue<");
  });
});

describe("LineupsScene mode B", () => {
  const games = [
    g(1, "blue", summaryOf(["Valla", "Jaina", "Diablo", "Arthas", "Zeratul"], ["A1", "A2", "A3", "A4", "A5"])),
    g(2, "red", summaryOf(["Tychus", "Kael'thas", "Muradin", "Li Li", "Illidan"], ["B1", "B2", "B3", "B4", "B5"])),
  ];
  it("auto switches to B after a finished game and dims heroes of earlier games (fearless)", () => {
    const html = renderToStaticMarkup(<LineupsScene s={series({ games })} />);
    expect(html).toContain('data-mode="B"');
    expect(count(html, "hp-i ph lk")).toBe(10); // G1 de chaque joueur, des deux équipes, verrouillé
    expect(count(html, "hp-i ph cur")).toBe(10); // G2 en évidence
    expect(html).toContain("G1 Valla · G2 Tychus");
    expect(html).toContain("FEARLESS HEROES LOCKED");
  });
  it("does not dim in a non-fearless format", () => {
    const html = renderToStaticMarkup(<LineupsScene s={series({ games, format: "standard" })} />);
    expect(count(html, " lk")).toBe(0);
  });
  it("shows a pending state while summaries are absent", () => {
    const html = renderToStaticMarkup(<LineupsScene s={series({ games: [g(1, "blue")] })} />);
    expect(html).toContain('data-mode="B"');
    expect(count(html, "awaiting game data")).toBe(10);
    expect(html).not.toContain("hp-i");
  });
  it("forced mode B/A wins over auto", () => {
    expect(renderToStaticMarkup(<LineupsScene s={series({ lineup_mode: "b" })} />)).toContain('data-mode="B"');
    expect(renderToStaticMarkup(<LineupsScene s={series({ games, lineup_mode: "a" })} heroes={heroes()} />)).toContain('data-mode="A"');
  });
});

describe("seriesPlayerHeroes (R14: toon, then exact name, never position)", () => {
  const row = (side: string, name: string, hero: string, toon?: string) => ({ side, name, hero, ...(toon ? { toon } : {}) });
  const withRows = (rows: unknown[]) => series({ games: [g(1, "blue", { players: rows })] });
  it("a row whose name matches no player stays unattributed (no positional fallback)", () => {
    const out = seriesPlayerHeroes(withRows([row("blue", "Totally Different", "Valla")]), "blue");
    expect(out.every((l) => l.length === 0)).toBe(true);
  });
  it("name match is trimmed and case-insensitive", () => {
    const out = seriesPlayerHeroes(withRows([row("blue", "  b3 ", "Muradin")]), "blue");
    expect(out[2]).toEqual([{ game: 1, hero: "Muradin" }]);
    expect(out.flat()).toHaveLength(1);
  });
  it("toon match wins over a differing name", () => {
    const rows = [row("blue", "B1", "Valla", "toon-B2"), row("blue", "OtherName", "Jaina", "toon-B1")];
    const out = seriesPlayerHeroes(withRows(rows), "blue");
    expect(out[1]).toEqual([{ game: 1, hero: "Valla" }]); // toon-B2 = slot 2 malgré le nom « B1 »
    expect(out[0]).toEqual([{ game: 1, hero: "Jaina" }]);
  });
  it("a same-name row on the other side is not used", () => {
    const out = seriesPlayerHeroes(withRows([row("red", "B1", "Valla")]), "blue");
    expect(out.flat()).toHaveLength(0);
  });
  it("tolerates malformed summaries; unmatched players render pending", () => {
    const s = series({ games: [g(1, "blue", { players: "nope" }), g(2, "red", {}), g(3, "red", { players: [row("blue", "Nobody", "Valla")] })] });
    expect(seriesPlayerHeroes(s, "blue").flat()).toHaveLength(0);
    const html = renderToStaticMarkup(<LineupsScene s={s} />);
    expect(html).toContain('data-mode="B"');
    expect(count(html, "awaiting game data")).toBe(10);
    expect(html).not.toContain("hp-i");
  });
});

describe("PlayerPicker helpers", () => {
  it("choosing a hit sets toon and current name; unlinked until chosen", () => {
    const hit = { toon: "1-S2-1-42", name: "NewName", matched_name: "OldName", matched_old: true, games: 12, wins: 6, roles: [], last_seen: null };
    expect(pickPatch(hit)).toEqual({ toon: "1-S2-1-42", name: "NewName" });
    expect(pickPatch({ ...hit, name: null }).name).toBe("OldName");
    expect(linkLabel(pl("x"))).toBe("unlinked");
    expect(linkLabel(pl("x", "t"))).toBe("linked");
  });
});
