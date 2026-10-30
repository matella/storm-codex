import { describe, expect, it } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";
import type { DraftState } from "../api";
import { DraftConsoleView } from "./DraftWorkspace";

const draft: DraftState = {
  schema_version: 1, format: "fearless", first_pick: "blue", map: "Cursed Hollow",
  blue: { name: "Alpha", players: ["p1", "p2", "p3", "p4", "p5"] }, red: { name: "Omega", players: [] },
  score: [1, 0], bo: 5,
  steps: [{ order: "first", action: "ban" }, { order: "first", action: "pick" }, { order: "second", action: "pick" }],
  cursor: 0, assignments: [null, null, null], manual_unavailable: [], series_bans: [], series_games: [],
};
const view = (locked: boolean) => renderToStaticMarkup(
  <DraftConsoleView d={draft} dim={undefined} role="All" setRole={() => {}} search="" setSearch={() => {}} locked={locked} />,
);

describe("DraftConsoleView", () => {
  it("active series: team names, pseudos, score and Bo are read-only, format is fixed", () => {
    const html = view(true);
    expect(html).not.toContain('placeholder="Team name"');
    expect(html).not.toMatch(/<input[^>]*class="pl"/);
    expect(html).not.toContain('aria-label="increase"');
    expect(html).not.toMatch(/<option[^>]*>Bo5</);
    expect(html).toMatch(/<select[^>]*disabled=""[^>]*>\s*<option[^>]*>Standard/);
    expect(html).toContain("Alpha");
    expect(html).toContain("p1");
    expect(html).toContain("Edit in the series rail");
  });
  it("no series: team names, pseudos, score and Bo stay editable", () => {
    const html = view(false);
    expect(html).toContain('placeholder="Team name"');
    expect(html).toContain('class="pl"');
    expect(html).toContain('aria-label="increase"');
    expect(html).toMatch(/<option[^>]*>Bo5</);
    expect(html).not.toContain(`disabled=""`);
  });
  it("M3: active series disables the draft's New series (the rail owns the series), Next game stays usable", () => {
    expect(view(true)).toMatch(/<button[^>]*disabled=""[^>]*>New series</);
    expect(view(true)).not.toMatch(/<button[^>]*disabled=""[^>]*>Next game/);
    expect(view(false)).not.toMatch(/<button[^>]*disabled=""[^>]*>New series</);
  });
  it("M4: the OBS overlay link points at the cast overlay", () => {
    expect(view(false)).toContain("/cast/overlay?skin=nexus");
    expect(view(false)).not.toContain("/draft/overlay");
  });
});

describe("format selector", () => {
  it("active series: shows the series format, disabled", () => {
    const html = renderToStaticMarkup(
      <DraftConsoleView d={{ ...draft, format: "standard" }} dim={undefined} role="All" setRole={() => {}} search="" setSearch={() => {}} locked seriesFormat="fearless" />,
    );
    expect(html).toMatch(/<select disabled="" [^>]*>(?:(?!<\/select>).)*<option value="fearless" selected="">/);
  });
});
