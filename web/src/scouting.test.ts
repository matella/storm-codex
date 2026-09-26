// Tests des helpers purs des rapports de scouting (environnement node).
import { describe, expect, it } from "vitest";
import { claimState, day, fmtRate, fmtWilson, pidNames, statusMeta, targetNames, uploadLabel } from "./scouting";
import type { Facts, ScoutGame } from "./scouting";

describe("fmtRate / fmtWilson", () => {
  it("k/n et pourcentage arrondi", () => {
    expect(fmtRate({ k: 7, n: 9 })).toBe("7/9 · 78%");
    expect(fmtRate({ k: 0, n: 3 })).toBe("0/3 · 0%");
  });
  it("sans effectif : tiret", () => {
    expect(fmtRate({ k: 0, n: 0 })).toBe("—");
    expect(fmtRate(null)).toBe("—");
  });
  it("intervalle arrondi, vide si absent", () => {
    expect(fmtWilson([43.85, 100])).toBe("95% CI 44–100%");
    expect(fmtWilson(null)).toBe("");
  });
});

describe("statusMeta", () => {
  it("un libellé par statut", () => {
    expect(statusMeta("empty").label).toBe("no replays");
    expect(statusMeta("stale").label).toBe("analysis outdated");
    expect(statusMeta("analyzed").cls).toBe("b-win");
  });
});

describe("claimState", () => {
  it("aucune preuve connue → unsupported ; une inconnue → partial", () => {
    expect(claimState({ evidence: [], unsupported: true })).toBe("unsupported");
    expect(claimState({ evidence: [{ id: "a", known: true }, { id: "b", known: false }], unsupported: false })).toBe("partial");
    expect(claimState({ evidence: [{ id: "a", known: true }], unsupported: false })).toBe("supported");
  });
});

describe("pidNames / day / uploadLabel / targetNames", () => {
  it("pN → nom", () => {
    const facts = { players: [{ pid: "p1", name: "Razhag" }, { pid: "p2", name: "Grolg" }] } as unknown as Facts;
    expect(pidNames(facts)).toEqual({ p1: "Razhag", p2: "Grolg" });
    expect(pidNames(null)).toEqual({});
  });
  it("date courte", () => {
    expect(day("2026-07-22T19:21:46Z")).toBe("2026-07-22");
    expect(day(null)).toBe("?");
  });
  it("résultat de dépôt", () => {
    expect(uploadLabel({ status: "added", target_team: 1 })).toBe("added");
    expect(uploadLabel({ status: "added", target_team: null })).toBe("added · team not found");
    expect(uploadLabel({ status: "duplicate" })).toBe("already in this report");
    expect(uploadLabel({ status: "parse_failed", error_class: "incomplete" })).toBe("could not read (incomplete)");
  });
  it("noms du côté cible", () => {
    const g = {
      target_team: 1,
      teams: [[{ toon: "a", name: "A", hero: "x" }], [{ toon: "b", name: "B", hero: "y" }]],
    } as unknown as ScoutGame;
    expect(targetNames(g)).toEqual(["B"]);
    expect(targetNames({ ...g, target_team: null })).toBeNull();
  });
});
