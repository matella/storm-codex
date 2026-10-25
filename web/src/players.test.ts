// Tests des helpers purs de recherche/identité des joueurs (environnement node).
import { describe, expect, it } from "vitest";
import { highlightSegments, likelyAlias, recentNames, seenRange } from "./players";
import type { NameSpan, PlayerIdentity } from "./api";

const span = (name: string, first: string | null = null, last: string | null = null): NameSpan => ({ name, first_seen: first, last_seen: last, games: 1 });

describe("highlightSegments", () => {
  it("surligne le fragment sans tenir compte de la casse, en gardant la casse d'origine", () => {
    expect(highlightSegments("StormyKnight", "knight")).toEqual([
      { text: "Stormy", hit: false },
      { text: "Knight", hit: true },
    ]);
  });
  it("requête vide ou absente : un seul segment non surligné", () => {
    expect(highlightSegments("Abc", "")).toEqual([{ text: "Abc", hit: false }]);
    expect(highlightSegments("Abc", "zz")).toEqual([{ text: "Abc", hit: false }]);
  });
  it("traite les caractères spéciaux littéralement (pas de regex)", () => {
    expect(highlightSegments("Kerrigan_Main", "_m")).toEqual([
      { text: "Kerrigan", hit: false },
      { text: "_M", hit: true },
      { text: "ain", hit: false },
    ]);
    expect(highlightSegments("a.b(c", ".b(")).toEqual([
      { text: "a", hit: false },
      { text: ".b(", hit: true },
      { text: "c", hit: false },
    ]);
  });
  it("correspondance en tête ou en fin : pas de segment vide", () => {
    expect(highlightSegments("Zed", "zed")).toEqual([{ text: "Zed", hit: true }]);
  });
});

describe("recentNames", () => {
  const many = Array.from({ length: 14 }, (_, i) => span(`n${i}`));
  it("garde les 3 plus récents et compte le reste", () => {
    const r = recentNames(many);
    expect(r.shown.map((n) => n.name)).toEqual(["n0", "n1", "n2"]);
    expect(r.hidden).toBe(11);
  });
  it("3 ou moins : rien de caché", () => {
    expect(recentNames(many.slice(0, 3)).hidden).toBe(0);
    expect(recentNames([]).shown).toEqual([]);
  });
  it("n personnalisable", () => {
    expect(recentNames(many, 5).hidden).toBe(9);
  });
});

describe("seenRange", () => {
  it("même mois, même année, années différentes", () => {
    expect(seenRange(span("a", "2025-01-03T10:00:00Z", "2025-01-28T10:00:00Z"))).toBe("Jan 2025");
    expect(seenRange(span("a", "2025-01-03T10:00:00Z", "2025-08-28T10:00:00Z"))).toBe("Jan–Aug 2025");
    expect(seenRange(span("a", "2024-11-03T10:00:00Z", "2025-08-28T10:00:00Z"))).toBe("Nov 2024–Aug 2025");
  });
  it("dates absentes", () => {
    expect(seenRange(span("a"))).toBe("date unknown");
  });
});

describe("likelyAlias", () => {
  const base: PlayerIdentity = { toon: "T", name: "T", names: [], suggestions: [], confirmed: [] };
  const sug = (toon: string, level: "high" | "low") => ({ toon, name: toon, level, score: 60, reasons: [] });
  it("jamais une suggestion de confiance basse", () => {
    expect(likelyAlias({ ...base, suggestions: [sug("L", "low")] })).toBeNull();
  });
  it("prend la première suggestion haute", () => {
    expect(likelyAlias({ ...base, suggestions: [sug("L", "low"), sug("H", "high")] })).toEqual({ toon: "H", name: "H" });
  });
  it("préfère un lien confirmé à une suggestion haute", () => {
    const i = { ...base, suggestions: [sug("H", "high")], confirmed: [{ toon: "C", name: "Conf", decided_at: "2026-01-01T00:00:00Z" }] };
    expect(likelyAlias(i)).toEqual({ toon: "C", name: "Conf" });
  });
  it("rien du tout : null", () => {
    expect(likelyAlias(base)).toBeNull();
  });
});
