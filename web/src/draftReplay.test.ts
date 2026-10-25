import { describe, expect, it } from "vitest";
import type { DraftStep } from "./api";
import { currentPhase, loadErrorMessage, phaseLabel, phasesOf, secondsLeft } from "./draftReplay";

// Séquence Standard (rôle d'ordre, action) — miroir de `draft/formats.rs`.
const F = "first", S = "second";
const STANDARD: DraftStep[] = [
  [F, "ban"], [S, "ban"], [F, "ban"], [S, "ban"], [F, "pick"], [S, "pick"], [S, "pick"], [F, "pick"],
  [F, "pick"], [S, "ban"], [F, "ban"], [S, "pick"], [S, "pick"], [F, "pick"], [F, "pick"], [S, "pick"],
].map(([order, action]) => ({ order, action }) as DraftStep);

describe("phasesOf", () => {
  it("découpe le Standard en 12 phases : ban×4, pick, 2 picks, 2 picks, ban×2, 2 picks, 2 picks, pick", () => {
    const p = phasesOf(STANDARD);
    expect(p.map((x) => x.len)).toEqual([1, 1, 1, 1, 1, 2, 2, 1, 1, 2, 2, 1]);
    expect(p.map((x) => x.action)).toEqual([
      "ban", "ban", "ban", "ban", "pick", "pick", "pick", "ban", "ban", "pick", "pick", "pick",
    ]);
    expect(p.map(phaseLabel)[5]).toBe("pick ×2");
  });

  it("currentPhase suit le curseur et vaut −1 une fois fini", () => {
    const p = phasesOf(STANDARD);
    expect(currentPhase(p, 0)).toBe(0);
    expect(currentPhase(p, 6)).toBe(5); // 2e étape de la phase « pick ×2 »
    expect(currentPhase(p, 7)).toBe(6);
    expect(currentPhase(p, 16)).toBe(-1);
  });
});

describe("secondsLeft / loadErrorMessage", () => {
  it("compte à rebours borné à 0", () => {
    expect(secondsLeft(null, 1000)).toBe(0);
    expect(secondsLeft(31_000, 1_000)).toBe(30);
    expect(secondsLeft(500, 1_000)).toBe(0);
  });
  it("messages d'erreur connus et repli", () => {
    expect(loadErrorMessage("not_custom")).toMatch(/custom/);
    expect(loadErrorMessage("???")).toBe("upload failed");
  });
});
