import { describe, it, expect } from "vitest";
import { INITIAL_REVEAL, nextRevealState, holdExpired, type RevealState } from "./revealState";

const at = (phase: RevealState["phase"], key: string | null, holdToken = 0): RevealState =>
  ({ phase, key, holdToken });

describe("nextRevealState", () => {
  it("premier sondage avec de la musique en cours → big, hold armé", () => {
    const s = nextRevealState(INITIAL_REVEAL, { playing: true, key: "a" });
    expect(s.phase).toBe("big");
    expect(s.key).toBe("a");
    expect(s.holdToken).toBe(INITIAL_REVEAL.holdToken + 1);
  });

  it("reprise après pause → big (même morceau qu'avant la pause)", () => {
    const paused = nextRevealState(at("mini", "a", 3), { playing: false, key: "a" });
    expect(paused.phase).toBe("hidden");
    const resumed = nextRevealState(paused, { playing: true, key: "a" });
    expect(resumed.phase).toBe("big");
    expect(resumed.holdToken).toBe(paused.holdToken + 1);
  });

  it("changement de piste en big → reste big, contenu remplacé, hold relancé", () => {
    const s = nextRevealState(at("big", "a", 5), { playing: true, key: "b" });
    expect(s.phase).toBe("big");
    expect(s.key).toBe("b");
    expect(s.holdToken).toBe(6);
  });

  it("changement de piste en mini → big", () => {
    const s = nextRevealState(at("mini", "a", 5), { playing: true, key: "b" });
    expect(s.phase).toBe("big");
    expect(s.key).toBe("b");
    expect(s.holdToken).toBe(6);
  });

  it("arrêt de lecture → hidden, sans incrémenter le hold", () => {
    const s = nextRevealState(at("mini", "a", 5), { playing: false, key: "a" });
    expect(s.phase).toBe("hidden");
    expect(s.key).toBeNull();
    expect(s.holdToken).toBe(5);
  });

  it("piste sans clé exploitable → hidden", () => {
    const s = nextRevealState(at("mini", "a", 5), { playing: true, key: null });
    expect(s.phase).toBe("hidden");
  });

  it("sondage identique → même objet, aucun réarmement", () => {
    const prev = at("mini", "a", 5);
    expect(nextRevealState(prev, { playing: true, key: "a" })).toBe(prev);
  });

  it("sondage identique en big → même objet (le hold ne redémarre pas)", () => {
    const prev = at("big", "a", 5);
    expect(nextRevealState(prev, { playing: true, key: "a" })).toBe(prev);
  });

  it("arrêt répété → même objet", () => {
    const prev = at("hidden", null, 5);
    expect(nextRevealState(prev, { playing: false, key: null })).toBe(prev);
  });
});

describe("holdExpired", () => {
  it("big → mini", () => {
    expect(holdExpired(at("big", "a", 5))).toEqual(at("mini", "a", 5));
  });

  it("ne touche pas mini", () => {
    const prev = at("mini", "a", 5);
    expect(holdExpired(prev)).toBe(prev);
  });

  it("ne touche pas hidden", () => {
    const prev = at("hidden", null, 5);
    expect(holdExpired(prev)).toBe(prev);
  });
});
