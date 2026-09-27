// Tests de la logique pure de la page Session (environnement node).
import { describe, expect, it } from "vitest";
import { byHero, byMode, kda, lastSession, myGames, pctOf } from "./session";
import type { MatchPlayer, MatchSummary } from "./api";

const P = (name: string, team: number, hero: string, extra: Partial<MatchPlayer> = {}): MatchPlayer =>
  ({ toon: name, name, hero, team, win: null, kills: 2, deaths: 1, takedowns: 7, ...extra });
const M = (id: number, at: string, winner: number, me: MatchPlayer, mode = 50091): MatchSummary =>
  ({ id, map: "Braxis Holdout", mode, played_at: at, length: 900, winner, build: 1, players: [me, P("x", 1 - (me.team ?? 0), "Valla")] });
const pick = (ps: MatchPlayer[]) => ps.find((p) => p.name === "me");

describe("myGames / lastSession", () => {
  const ms = [
    M(5, "2026-06-10T00:40:00Z", 0, P("me", 0, "Gul'dan")),
    M(4, "2026-06-09T22:10:00Z", 1, P("me", 0, "Mei")),
    M(3, "2026-06-09T20:00:00Z", 0, P("me", 0, "Mei")),
    M(2, "2026-06-01T20:00:00Z", 0, P("me", 1, "Blaze")),
    { ...M(1, "2026-06-09T21:00:00Z", 0, P("other", 0, "Tyrael")), players: [P("other", 0, "Tyrael")] },
  ];
  it("ne garde que les parties de l'opérateur, avec le résultat de son point de vue", () => {
    const g = myGames(ms, pick);
    expect(g.map((x) => x.m.id)).toEqual([5, 4, 3, 2]);
    expect(g.map((x) => x.won)).toEqual([true, false, true, false]);
  });
  it("la session regroupe les parties séparées de moins de 3 h, y compris après minuit", () => {
    const s = lastSession(myGames(ms, pick));
    expect(s.map((x) => x.m.id)).toEqual([3, 4, 5]);
  });
  it("aucune partie → session vide", () => expect(lastSession([])).toEqual([]));
});

describe("kda / tallies", () => {
  it("assists = takedowns − kills, jamais négatif", () => {
    expect(kda(P("me", 0, "X", { kills: 5, deaths: 1, takedowns: 17 }))).toEqual({ k: 5, d: 1, a: 12 });
    expect(kda(P("me", 0, "X", { kills: 3, deaths: 0, takedowns: 2 })).a).toBe(0);
  });
  it("bilans par héros et par mode, triés par nombre de parties", () => {
    const g = myGames([
      M(1, "2026-06-09T20:00:00Z", 0, P("me", 0, "Mei"), 50101),
      M(2, "2026-06-09T21:00:00Z", 1, P("me", 0, "Mei"), 50091),
      M(3, "2026-06-09T22:00:00Z", 0, P("me", 0, "Blaze"), 50101),
    ], pick);
    expect(byHero(g)).toEqual([{ key: "Mei", games: 2, wins: 1 }, { key: "Blaze", games: 1, wins: 1 }]);
    expect(byMode(g, (m) => (m === 50101 ? "ARAM" : "SL"))).toEqual([{ key: "ARAM", games: 2, wins: 2 }, { key: "SL", games: 1, wins: 0 }]);
    expect(pctOf(2, 3)).toBe(67);
    expect(pctOf(0, 0)).toBe(0);
  });
});
