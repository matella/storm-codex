import { describe, it, expect } from "vitest";
import { sampleAt, type HeroTrack } from "./replay2d";

const h: HeroTrack = {
  playerId: 1,
  samples: [ {t:0,x:0,y:0,exact:true}, {t:10,x:1,y:1,exact:false} ],
  life: [ {from:0,to:6}, {from:8,to:10} ],
};

describe("sampleAt", () => {
  it("lerp entre deux samples vivants", () => {
    const p = sampleAt(h, 5); // vivant (0..6)
    expect(p).not.toBeNull();
    expect(p!.x).toBeCloseTo(0.5); expect(p!.alive).toBe(true);
  });
  it("fige la position pendant l'intervalle mort (pas de lerp à travers)", () => {
    const p = sampleAt(h, 7); // mort (6..8)
    expect(p!.alive).toBe(false);
    expect(p!.x).toBeCloseTo(0.6); // dernière position vivante à t=6, pas 0.7
  });
  it("borne avant le premier / après le dernier sample", () => {
    expect(sampleAt(h, -1)!.x).toBeCloseTo(0);
    expect(sampleAt(h, 99)!.x).toBeCloseTo(1);
  });
});
