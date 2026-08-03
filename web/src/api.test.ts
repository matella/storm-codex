// Tests unitaires des helpers purs de la couche API (affichage, URLs, parsing).
// `npm test` (vitest, environnement node — aucun DOM requis).
import { describe, expect, it } from "vitest";
import {
  announceLabel,
  awardLabel,
  classBadge,
  fmtClock,
  fmtDur,
  heroKey,
  initials,
  matchesUrl,
  mapImage,
  modeBadge,
  parseTrack,
  pickOperator,
  sideOfStep,
  universeColor,
  type DraftState,
} from "./api";

describe("fmtDur / fmtClock", () => {
  it("formate mm:ss avec zéro non signifiant", () => {
    expect(fmtDur(536.2)).toBe("8:56");
    expect(fmtDur(60)).toBe("1:00");
    expect(fmtDur(null)).toBe("—");
  });
  it("fmtClock signe les temps négatifs (draft/chargement)", () => {
    expect(fmtClock(13.3125)).toBe("0:13");
    expect(fmtClock(-11.9375)).toBe("−0:11");
    expect(fmtClock(538.5)).toBe("8:58");
    expect(fmtClock(0)).toBe("0:00");
  });
});

describe("announceLabel", () => {
  it("mappe les types d'annonce du parser", () => {
    expect(announceLabel({ Ability: { m_abilLink: 1283 } })).toBe("ability callout");
    expect(announceLabel({ Vitals: 2 })).toBe("vitals callout (help)");
    expect(announceLabel({ Behavior: {} })).toBe("behavior callout");
    expect(announceLabel(null)).toBe("callout");
    expect(announceLabel(7)).toBe("callout");
  });
});

describe("modeBadge", () => {
  it("mappe les modes connus et retombe sur — sinon", () => {
    expect(modeBadge(50101).short).toBe("ARAM");
    expect(modeBadge(50071).short).toBe("TL");
    expect(modeBadge(50091).short).toBe("SL");
    expect(modeBadge(-1).short).toBe("CUSTOM");
    expect(modeBadge(12345).short).toBe("—");
    expect(modeBadge(null).short).toBe("—");
  });
});

describe("matchesUrl", () => {
  it("ne sérialise que les filtres présents + limit par défaut", () => {
    expect(matchesUrl({})).toBe("/api/matches?limit=50");
    const u = new URL("http://x" + matchesUrl({ map: "Sky Temple", mode: 50101, mvp: true, limit: 10 }));
    expect(u.searchParams.get("map")).toBe("Sky Temple");
    expect(u.searchParams.get("mode")).toBe("50101");
    expect(u.searchParams.get("mvp")).toBe("true");
    expect(u.searchParams.get("limit")).toBe("10");
    expect(u.searchParams.has("hero")).toBe(false);
  });
});

describe("heroKey", () => {
  it("normalise les noms composés vers une clé de jointure commune", () => {
    expect(heroKey("E.T.C.")).toBe(heroKey("ETC"));
    expect(heroKey("Li-Ming")).toBe(heroKey("LiMing"));
    expect(heroKey("The Lost Vikings")).toBe(heroKey("LostVikings"));
    expect(heroKey("Lúcio")).toBe("lucio");
  });
});

describe("awardLabel", () => {
  it("détecte le MVP et décamelise les autres awards", () => {
    expect(awardLabel("EndOfMatchAwardMVPBoolean")).toEqual({ label: "MVP", icon: "👑", mvp: true });
    const a = awardLabel("EndOfMatchAwardMostKillsBoolean");
    expect(a?.mvp).toBe(false);
    expect(a?.label).toBe("Kills");
    expect(awardLabel(null)).toBeNull();
  });
});

describe("classBadge", () => {
  it("badge par classification, null si inconnue", () => {
    expect(classBadge("BUFF")?.label).toBe("BUFF");
    expect(classBadge("nerf")?.label).toBe("NERF");
    expect(classBadge("")).toBeNull();
    expect(classBadge(undefined)).toBeNull();
  });
});

describe("initials / universeColor / mapImage", () => {
  it("fallbacks sûrs sans référentiel chargé", () => {
    expect(initials("matella")).toBe("MA");
    expect(initials(null)).toBe("··");
    // DIM vide (pas de useDimHeroes en test) → couleur Nexus par défaut, jamais d'exception
    expect(universeColor("Jaina")).toBe("var(--u-nexus)");
    expect(universeColor(null)).toBe("var(--u-nexus)");
    expect(mapImage("Blackheart's Bay")).toBe("/images/battlegrounds/blackhearts-bay.png");
    expect(mapImage(null)).toBeNull();
  });
});

describe("pickOperator", () => {
  const players = [
    { name: "Alice" },
    { name: "matella" },
    { name: null },
  ];
  it("priorise l'override, insensible à la casse, fallback 1er joueur", () => {
    expect(pickOperator(players, "MATELLA")?.name).toBe("matella");
    // sans override ni réglage chargé → fallback premier joueur
    expect(pickOperator(players)?.name).toBe("Alice");
    expect(pickOperator([], "x")).toBeUndefined();
  });
});

describe("parseTrack", () => {
  it("shape Spotify /api/playback/now", () => {
    const t = parseTrack({
      authenticated: true,
      current: {
        isPlaying: true,
        track: {
          name: "One More Time",
          artists: [{ name: "Daft Punk" }],
          album: { name: "Discovery", images: [{ url: "http://img" }] },
        },
      },
    });
    expect(t).toMatchObject({ playing: true, title: "One More Time", artist: "Daft Punk", art: "http://img", album: "Discovery" });
  });
  it("shape ancien engine + arrêts", () => {
    const t = parseTrack({
      authenticated: true,
      current: { isPlaying: false, current: { name: "X", artist: "Y", albumArtUrl: "z" } },
    });
    expect(t.playing).toBe(false);
    expect(t.title).toBe("X");
    // non authentifié → jamais "playing"
    expect(parseTrack({ authenticated: false, current: { track: { name: "X" } } }).playing).toBe(false);
    expect(parseTrack(undefined).playing).toBe(false);
  });
});

describe("sideOfStep", () => {
  it("résout le rôle d'ordre en côté visuel via first_pick", () => {
    const d = { first_pick: "red" } as DraftState;
    expect(sideOfStep(d, { order: "first", action: "pick" })).toBe("red");
    expect(sideOfStep(d, { order: "second", action: "ban" })).toBe("blue");
  });
});
