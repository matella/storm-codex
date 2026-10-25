import { describe, expect, it } from "vitest";
import type { DraftState, ReplayState } from "./api";
import { isReplayReady, timerFraction } from "./draftOverlay";

const replay = (over: Partial<ReplayState> = {}): ReplayState =>
  ({ queue: [], current: 0, fearless: false, playing: false, interval_ms: 30_000, next_at_ms: null, ...over });
const draft = (cursor: number, r?: ReplayState) => ({ cursor, replay: r }) as unknown as DraftState;

describe("isReplayReady", () => {
  it("vrai seulement avec une partie de replay courante et le curseur à 0", () => {
    expect(isReplayReady(draft(0, replay()))).toBe(true);
    expect(isReplayReady(draft(1, replay()))).toBe(false);
    expect(isReplayReady(draft(0, replay({ current: null })))).toBe(false);
    expect(isReplayReady(draft(0))).toBe(false);
  });
});

describe("timerFraction", () => {
  it("hors replay : décompte local", () => {
    expect(timerFraction(draft(2), 0, 30, 45)).toBeCloseTo(30 / 45);
  });
  it("replay en pause ou manuel : barre vide", () => {
    expect(timerFraction(draft(2, replay()), 0, 30, 45)).toBe(0);
  });
  it("lecture auto : (next_at - now) / interval, borné à [0,1]", () => {
    const d = draft(2, replay({ playing: true, next_at_ms: 40_000 }));
    expect(timerFraction(d, 25_000, 0, 45)).toBeCloseTo(0.5);
    expect(timerFraction(d, 50_000, 0, 45)).toBe(0);
    expect(timerFraction(d, 0, 0, 45)).toBe(1);
  });
});
