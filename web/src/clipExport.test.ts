// US-25 : export d'un sous-intervalle [start, end] du replay en vidéo webm, en enregistrant le
// canvas LIVE pendant une lecture pilotée start→end (pas de rendu offscreen/accéléré). Seule la
// partie pure (nombre de frames) est testable en unit — le reste (MediaRecorder) est E2E-only.
import { describe, expect, it } from "vitest";
import { clipFrames } from "./clipExport";

describe("clipFrames", () => {
  it("compte les frames d'un intervalle valide", () => {
    expect(clipFrames(10, 15, 30)).toBe(150);
  });

  it("retourne 0 si start == end (intervalle vide)", () => {
    expect(clipFrames(5, 5, 30)).toBe(0);
  });

  it("retourne 0 si end < start (intervalle invalide)", () => {
    expect(clipFrames(15, 10, 30)).toBe(0);
  });
});
