// Tests du rendu HTML exporté (pur, environnement node).
import { describe, expect, it } from "vitest";
import { compactEvidence, esc, humanize, exportFileName, heroesOf, mapsOf, renderReportHtml } from "./reportHtml";
import type { ExportAssets } from "./reportHtml";
import type { Analysis, Facts, ScoutingReport } from "./scouting";

const rate = (k: number, n: number) => ({ k, n });
const facts = {
  overview: { games: 2, record: rate(1, 2), excluded: 0, first_date: "2026-07-22T19:00:00Z", last_date: "2026-07-22T20:00:00Z", builds: [97605], avg_length_s: 1200 },
  players: [{ pid: "p1", toon: "t1", name: "Razhag", core: true, record: rate(1, 2),
    heroes: [{ id: "p1.hero.tyrael", hero: "Tyrael", picks: rate(2, 2), record: rate(1, 2) }],
    roles: [{ id: "p1.role.tank", key: "Tank", count: rate(2, 2) }],
    stats: { kills: 1, deaths: 2, assists: 3, kill_participation_pct: 50, hero_damage_pm: 100, siege_damage_pm: 1, healing_pm: 0, damage_taken_pm: 1, xp_pm: 1, time_dead_pct: 5 } }],
  maps: [
    { id: "map.braxis_holdout", map: "Braxis Holdout", record: rate(1, 1), wilson: [20.7, 100],
      picks: [{ id: "map.braxis_holdout.pick.tyrael", hero: "Tyrael", picks: rate(1, 1), record: rate(1, 1), by: ["p1"] }],
      bans: [{ id: "map.braxis_holdout.ban.johanna", key: "Johanna", count: rate(1, 1) }], bans_against: [] },
    { id: "map.alterac_pass", map: "Alterac Pass", record: rate(0, 1), wilson: [0, 79], picks: [], bans: [], bans_against: [] },
  ],
  draft: { games: 2, first_pick: rate(1, 2), first_pick_record: rate(1, 1), second_pick_record: rate(0, 1),
    bans_first: [{ id: "draft.ban_first.johanna", key: "Johanna", count: rate(2, 2) }], bans_mid: [], bans_against: [],
    openers: [], opener_roles: [], last_picks: [],
    faced: [{ id: "draft.faced.falstad", hero: "Falstad", picks: rate(1, 2), record: rate(0, 1) }] },
  flow: {} as Facts["flow"],
  games: [{ id: "game.1", gid: 1, date: "2026-07-22T19:00:00Z", map: "Braxis Holdout", won: true, length_s: 800, first_pick: true,
    opponents: ["X"], picks: [{ hero: "Tyrael", player: "p1" }], opp_picks: [{ hero: "Falstad", player: "X" }], bans: ["Johanna"], bans_against: [] }],
  index: {},
} as unknown as Facts;

const ev = (known = true) => [{ id: "map.braxis_holdout.record", known, label: "Braxis — record", text: "1/1 (100%)", n: 1, low: true }];
const analysis: Analysis = {
  model: "test-model",
  summary: "They love <script>alert(1)</script> Tyrael.",
  map_choice: { pick: [{ map: "Alterac Pass", why: "lost there", evidence: ev(), unsupported: false }], avoid: [] },
  maps: [{ map: "Braxis Holdout", confidence: "low", overview: "fast wins", evidence: ev(), unsupported: false,
    bans: [{ hero: "Tyrael", phase: "first", player: null, why: "their tank", evidence: ev(), unsupported: false }],
    picks: [{ hero: "Falstad", phase: null, player: null, why: "beat them", evidence: [], unsupported: true }],
    their_picks: [{ hero: "Tyrael", phase: null, player: "p1", why: "comfort", evidence: ev(), unsupported: false }],
    considerations: [{ point: "hold a counter", evidence: ev(), unsupported: false }] }],
  general: { bans: [], picks: [], considerations: [] },
};
const report = (a: Analysis | null): ScoutingReport => ({
  id: 3, title: "Series & co", target_name: "Razhag's team", roster: [], anchors: [], facts_version: 7,
  snapshot: { facts, detection: { candidates: [], ambiguous: false }, roster: ["t1"], roster_auto: true },
  analysis: a, analysis_facts_version: 7, analysis_model: a ? "test-model" : null, analysis_imported_at: null,
  created_at: "2026-09-26", updated_at: "2026-09-26", status: a ? "analyzed" : "ready", games: [],
});
const assets: ExportAssets = { hero: { Tyrael: "data:image/webp;base64,AAA" }, map: { "Braxis Holdout": "data:image/webp;base64,BBB" }, ring: {} };

describe("renderReportHtml", () => {
  const html = renderReportHtml(report(analysis), assets, "27 Sep 2026");
  it("échappe tout texte venu du LLM ou des replays", () => {
    expect(html).not.toContain("<script>alert(1)</script>");
    expect(html).toContain("&lt;script&gt;alert(1)&lt;/script&gt;");
    expect(html).toContain("Razhag&#39;s team");
    expect(html).toContain("Series &amp; co");
  });
  it("un tableau de draft par carte, ancré, avec la navigation", () => {
    expect(html).toContain('id="map-braxis-holdout"');
    expect(html).toContain('id="map-alterac-pass"');
    expect(html).toContain('href="#map-braxis-holdout"');
    expect(html).toContain("1ST PHASE");
    expect(html).toContain('class="pt pt-ban"');
    expect(html).toContain("low confidence");
  });
  it("embarque les images fournies et retombe sur les initiales sinon", () => {
    // chaque image embarquée une seule fois, référencée par classe
    expect(html.split("data:image/webp;base64,AAA").length - 1).toBe(1);
    expect(html).toContain(".h-tyrael{background-image:url('data:image/webp;base64,AAA')}");
    expect(html).toContain(".m-braxis-holdout{--img:url('data:image/webp;base64,BBB')}");
    expect(html).toContain('class="mboard-hd m-braxis-holdout"');
    expect(html).toMatch(/class="pt pt-ban h-tyrael"/);
    expect(html).toMatch(/<span class="ini">FA<\/span>/); // Falstad sans portrait
  });
  it("signale les recommandations sans preuve", () => {
    expect(html).toContain("⚠ unverified");
  });
  it("refuse une data: URI malformée dans la feuille d'assets", () => {
    const bad = renderReportHtml(report(analysis), { hero: { Tyrael: "data:image/webp;base64,A')}</style><script>x" }, map: {}, ring: {} }, "now");
    expect(bad).not.toContain("<script>x");
  });
  it("fonctionne sans analyse importée", () => {
    const bare = renderReportHtml(report(null), { hero: {}, map: {}, ring: {} }, "now");
    expect(bare).toContain("No draft plan imported yet");
    expect(bare).toContain('id="map-braxis-holdout"');
    expect(bare).toContain("No plan for this map");
    expect(bare.startsWith("<!doctype html>")).toBe(true);
  });
});

describe("helpers d'export", () => {
  it("preuve compacte : sans parenthèses, sans répéter la carte, pseudos au lieu des pN", () => {
    const e = { label: "Alterac Pass — Hogger picked by them (games, record, players)", text: "1 of 1 game, 1/1 (100%), played by p5" };
    expect(compactEvidence(e, { names: { p5: "vmatom" }, map: "Alterac Pass" }))
      .toEqual({ label: "Hogger picked by them", text: "1 of 1 game, 1/1 (100%), played by vmatom" });
    expect(compactEvidence({ label: "Razhag (p4) — averages", text: "x" }, { names: {} }).label).toBe("Razhag — averages");
  });
  it("humanize : pseudos au lieu des pN dans le texte du LLM", () => {
    expect(humanize("bruiser for vmatom (p5), then p3 carries", { p3: "MrTyCo", p5: "vmatom" })).toBe("bruiser for vmatom, then MrTyCo carries");
  });
  it("esc", () => expect(esc(`<a href="x">'&'</a>`)).toBe("&lt;a href=&quot;x&quot;&gt;&#39;&amp;&#39;&lt;/a&gt;"));
  it("nom de fichier", () => {
    expect(exportFileName("Série 2026-07-22 — Razhag's team")).toBe("scouting-serie-2026-07-22-razhag-s-team.html");
    expect(exportFileName("!!!")).toBe("scouting-report.html");
  });
  it("héros et cartes à charger", () => {
    const r = report(analysis);
    expect(heroesOf(r)).toEqual(["Falstad", "Johanna", "Tyrael"]);
    expect(mapsOf(r)).toEqual(["Alterac Pass", "Braxis Holdout"]);
  });
});
