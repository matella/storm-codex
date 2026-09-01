import { describe, it, expect } from "vitest";
import { parseTrack, trackKey } from "./api";

describe("parseTrack", () => {
  it("extrait l'id Spotify de la forme /api/playback/now", () => {
    const t = parseTrack({
      authenticated: true,
      current: { isPlaying: true, track: { id: "4uLU6hMCjMI75M1A2tKUQC", name: "Ghosts", artists: [{ name: "Foundry" }] } },
    });
    expect(t.id).toBe("4uLU6hMCjMI75M1A2tKUQC");
    expect(t.playing).toBe(true);
  });

  it("retombe sur l'uri quand id est absent", () => {
    const t = parseTrack({
      authenticated: true,
      current: { isPlaying: true, track: { uri: "spotify:track:abc", name: "Ghosts" } },
    });
    expect(t.id).toBe("spotify:track:abc");
  });

  it("laisse id indéfini quand la source n'en fournit aucun", () => {
    const t = parseTrack({
      authenticated: true,
      current: { isPlaying: true, current: { name: "Ghosts", artist: "Foundry" } },
    });
    expect(t.id).toBeUndefined();
    expect(t.title).toBe("Ghosts");
  });
});

describe("trackKey", () => {
  it("préfère l'id quand il existe", () => {
    expect(trackKey({ playing: true, id: "abc", title: "Ghosts", artist: "Foundry" })).toBe("abc");
  });

  it("retombe sur titre|artiste sans id", () => {
    expect(trackKey({ playing: true, title: "Ghosts", artist: "Foundry" })).toBe("Ghosts|Foundry");
  });

  it("tolère un artiste manquant", () => {
    expect(trackKey({ playing: true, title: "Ghosts" })).toBe("Ghosts|");
  });

  it("rend null sans id ni titre", () => {
    expect(trackKey({ playing: false })).toBeNull();
  });
});
