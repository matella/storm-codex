// Tests des helpers purs des patch notes (environnement node).
import { describe, expect, it } from "vitest";
import { patchHeadings, ptagClass } from "./patchHtml";

describe("patchHeadings", () => {
  it("titres markdown → balises hN, avec ancre quand elle existe", () => {
    const out = patchHeadings("## Heroes {#heroes}\n<ul><li>x</li></ul>\n### Gul'dan {#guldan}\n#### Talents");
    expect(out).toContain('<h2 id="heroes">Heroes</h2>');
    expect(out).toContain('<h3 id="guldan">Gul\'dan</h3>');
    expect(out).toContain("<h4>Talents</h4>");
    expect(out).toContain("<ul><li>x</li></ul>"); // l'HTML existant est laissé tel quel
  });
  it("ne touche pas un # hors début de ligne", () => {
    expect(patchHeadings("Rank #1 hero")).toBe("Rank #1 hero");
  });
});

describe("ptagClass", () => {
  it("classifications connues et repli", () => {
    expect(ptagClass("buff")).toBe("buff");
    expect(ptagClass("NERF")).toBe("nerf");
    expect(ptagClass("REWORK")).toBe("upd");
    expect(ptagClass(null)).toBe("upd");
  });
});
