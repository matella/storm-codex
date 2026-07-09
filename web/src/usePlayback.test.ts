import { describe, it, expect } from "vitest";
import { advance } from "./usePlayback";

describe("advance", () => {
  it("avance t de dt*speed tant que < duration", () => {
    expect(advance(10, 1, 2, 100)).toEqual({ t: 12, playing: true });
  });
  it("reste dans la durée (milieu) → playing true", () => {
    expect(advance(50, 1, 4, 100)).toEqual({ t: 54, playing: true });
  });
  it("clampe à duration et auto-pause en fin de clip", () => {
    expect(advance(99.5, 1, 8, 100)).toEqual({ t: 100, playing: false });
  });
});
