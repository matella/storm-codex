// Tests de la vague de corrections 2 de la régie de cast (items 15–25) : helpers purs + rendus statiques.
import { describe, expect, it } from "vitest";
import castStateSrc from "./castState.ts?raw";
import overlaySrc from "./pages/CastOverlay.tsx?raw";
import apiSrc from "./api.ts?raw";
import appSrc from "./App.tsx?raw";
import draftWsSrc from "./cast/DraftWorkspace.tsx?raw";
import railSrc from "./cast/SeriesRail.tsx?raw";
import seriesWsSrc from "./cast/SeriesWorkspace.tsx?raw";
import overlayCss from "./cast/cast-overlay.css?raw";
import castCss from "./cast/cast.css?raw";
import { renderToStaticMarkup } from "react-dom/server";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import type { DraftState } from "./api";
import type { CastSeries, CastTeam, GameRecord, PoolMap } from "./castApi";
import {
  mapCardView, modeAHint, replayNote, resyncTeam, sceneFromSearch, sceneTransition, shownScene, slideLabel,
  stripLayout, tabKeyTarget,
} from "./castState";
import { SeriesScene } from "./cast/SeriesScene";
import { PoolPanel } from "./cast/PoolPanel";
import { ScenePreview, previewStyles } from "./cast/ScenePreview";
import { canSearch, latestGuard } from "./cast/PlayerPicker";
import { DraftWorkspace } from "./cast/DraftWorkspace";
import { SceneTabs } from "./pages/Cast";
import { LEGACY_REDIRECTS } from "./legacyRoutes";

const team = (name: string): CastTeam => ({ name, logo: null, record: "", players: [], captain: null });
const g = (n: number, map: string, winner: GameRecord["winner"], summary: GameRecord["summary"] = null): GameRecord => ({
  number: n, map, winner, takedowns: [0, 0], duration_secs: null, summary,
});
const series = (over: Partial<CastSeries> = {}): CastSeries => ({
  schema_version: 1, series_id: "s1", active: true, blue: team("Alpha"), red: team("Omega"), bo: 5, format: "fearless",
  score_override: null, pool: [], games: [], scene: "series", lineup_mode: "auto",
  slide: { game: null, index: 0, auto: false, interval_ms: 12000, since_ms: 0 }, ...over,
});
const count = (html: string, needle: string) => html.split(needle).length - 1;
const card = (html: string, map: string) => (html.match(/<article[\s\S]*?<\/article>/g) ?? []).find((c) => c.includes(`<b>${map}</b>`)) ?? "";

describe("15 · map card priorities", () => {
  const banned: PoolMap = { name: "A", status: "banned", by: "red" };
  it("NEXT wins over a pool status (banned) when the map is the next one", () => {
    expect(mapCardView(banned, [], "A")).toMatchObject({ tone: "gold", label: "NEXT", struck: false });
  });
  it("an unknown nextMap (not in the pool) marks nothing NEXT", () => {
    const pool: PoolMap[] = [{ name: "A", status: "picked", by: "blue" }, { name: "B", status: "decider" }];
    expect(pool.map((m) => mapCardView(m, [], "Nowhere").label)).toEqual(["picked by Blue", "decider"]);
  });
});

describe("15 · Series strip states", () => {
  const pool: PoolMap[] = [
    { name: "Cursed Hollow", status: "picked", by: "blue" },
    { name: "Sky Temple", status: "picked", by: "red" },
    { name: "Dragon Shire", status: "decider" },
    { name: "Volskaya Foundry", status: "banned", by: "blue" },
  ];
  it("NEXT lands on the first picked/decider map without a game, in gold", () => {
    const html = renderToStaticMarkup(<SeriesScene s={series({ pool, games: [g(1, "Cursed Hollow", "blue")] })} />);
    expect(count(html, ">NEXT<")).toBe(1);
    expect(card(html, "Sky Temple")).toMatch(/class="mc tone-gold"/);
    expect(card(html, "Sky Temple")).toContain(">NEXT<");
  });
  it("no NEXT once every chosen map has a game", () => {
    const games = [g(1, "Cursed Hollow", "blue"), g(2, "Sky Temple", "red"), g(3, "Dragon Shire", "red")];
    expect(renderToStaticMarkup(<SeriesScene s={series({ pool, games })} />)).not.toContain(">NEXT<");
  });
  it("in-progress game: PLAYING on its map, NEXT on a later map", () => {
    const html = renderToStaticMarkup(<SeriesScene s={series({ pool, games: [g(1, "Cursed Hollow", "blue"), g(2, "Sky Temple", null)] })} />);
    expect(card(html, "Sky Temple")).toContain(">PLAYING<");
    expect(card(html, "Dragon Shire")).toContain(">NEXT<");
  });
  it("pools under 6 cards: one row, as many columns as cards", () => {
    const html = renderToStaticMarkup(<SeriesScene s={series({ pool })} />);
    expect(html).toContain("--n:4");
    expect(html).toContain('data-rows="1"');
  });
});

describe("24 · strip rows computed from the pool size", () => {
  it("stripLayout", () => {
    expect(stripLayout(0)).toEqual({ cols: 1, rows: 1 });
    expect(stripLayout(6)).toEqual({ cols: 6, rows: 1 });
    expect(stripLayout(7)).toEqual({ cols: 5, rows: 2 });
    expect(stripLayout(10)).toEqual({ cols: 5, rows: 2 });
    expect(stripLayout(11)).toEqual({ cols: 5, rows: 3 });
    expect(stripLayout(14)).toEqual({ cols: 5, rows: 3 });
  });
  it("a 14-map pool renders three lower rows", () => {
    const many: PoolMap[] = Array.from({ length: 14 }, (_, i) => ({ name: `Map${i}`, status: "available" }));
    const html = renderToStaticMarkup(<SeriesScene s={series({ pool: many })} />);
    expect(html).toContain("three-rows");
    expect(html).toContain('data-rows="3"');
    expect(count(html, "<article")).toBe(14);
    const css = overlayCss;
    expect(css).toMatch(/\.three-rows \.mc \{ height: 6\.2cqw;/);
  });
});

describe("15 · MAPS is a single list", () => {
  it("defined once, in castState.ts; the Draft workspace and the rail import it", () => {
    expect(castStateSrc).toMatch(/export const MAPS = \[/);
    for (const src of [draftWsSrc, railSrc]) {
      expect(src).toMatch(/import \{[^}]*\bMAPS\b[^}]*\} from "\.\.\/castState"/);
      expect(src).not.toMatch(/const MAPS\s*=/);
    }
  });
});

describe("16 · overlay scene transition (fade-out keeps the previous snapshot)", () => {
  const a = series({ scene: "series" });
  it("first scene: no leaving layer", () => {
    expect(sceneTransition({ cur: null, leaving: null }, a)).toEqual({ cur: { key: "series", s: a, id: 1 }, leaving: null });
  });
  it("same scene, same state: returns the same object (stable)", () => {
    const st = { cur: shownScene(a), leaving: null };
    expect(sceneTransition(st, a)).toBe(st);
  });
  it("same scene, new state: current layer updated, nothing leaves", () => {
    const st = { cur: shownScene(a), leaving: null };
    const b = { ...a, bo: 3 };
    expect(sceneTransition(st, b)).toEqual({ cur: { key: "series", s: b, id: 1 }, leaving: null });
  });
  it("scene change: the old layer leaves with ITS snapshot", () => {
    const st = { cur: shownScene(a), leaving: null };
    const b = { ...a, scene: "lineups" as const };
    const next = sceneTransition(st, b);
    expect(next.cur).toEqual({ key: "lineups", s: b, id: 2 });
    expect(next.leaving).toEqual({ key: "series", s: a, id: 1 });
  });
  it("to hide / inactive: fades out to nothing", () => {
    const st = { cur: shownScene(a), leaving: null };
    expect(sceneTransition(st, { ...a, scene: "hide" })).toEqual({ cur: null, leaving: { key: "series", s: a, id: 1 } });
    expect(sceneTransition(st, { ...a, active: false })).toEqual({ cur: null, leaving: { key: "series", s: a, id: 1 } });
  });
  it("the leaving layer keeps the id of the layer that was current; a new current layer gets a new id", () => {
    const st = { cur: shownScene(a), leaving: null };
    const lineups = { ...a, scene: "lineups" as const };
    const n1 = sceneTransition(st, lineups);
    expect(n1.leaving?.id).toBe(st.cur?.id);
    expect(n1.cur?.id).not.toBe(st.cur?.id);
    // same-scene update keeps the id (no remount)
    expect(sceneTransition(n1, { ...lineups, bo: 3 }).cur?.id).toBe(n1.cur?.id);
    // second change while the first still fades: ids never collide
    const n2 = sceneTransition(n1, { ...a, scene: "draft" as const });
    expect(n2.leaving?.id).toBe(n1.cur?.id);
    expect(n2.cur?.id).not.toBe(n2.leaving?.id);
    // back to the first scene: fresh id, not the dropped one
    expect(new Set([n2.cur?.id, n2.leaving?.id]).size).toBe(2);
  });
  it("overlay keys both layer divs by the layer id (same instance while fading)", () => {
    expect(overlaySrc).toMatch(/className="scene out" key=\{leaving\.id\}/);
    expect(overlaySrc).toMatch(/className="scene" key=\{cur\.id\}/);
  });
  it("draft scene stays visible without an active series (R19)", () => {
    expect(shownScene({ ...a, active: false, scene: "draft" })?.key).toBe("draft");
  });
});

describe("17 · lineups: picker race guard and loading/error hints", () => {
  it("only the last issued search applies; typing invalidates an in-flight search", () => {
    const gd = latestGuard();
    const first = gd.issue();
    const second = gd.issue();
    expect(gd.isLatest(first)).toBe(false);
    expect(gd.isLatest(second)).toBe(true);
    gd.invalidate();
    expect(gd.isLatest(second)).toBe(false);
  });
  it("Enter follows the Find button's disabled state", () => {
    expect(canSearch("a", null)).toBe(false);
    expect(canSearch("ab", null)).toBe(true);
    expect(canSearch("ab", "loading")).toBe(false);
  });
  it("mode A hint: loading / error instead of 'hidden' while the query is not settled", () => {
    const ok = { isLoading: false, isError: false };
    expect(modeAHint(false, 0, { isLoading: true, isError: false })).toBe("hidden (unlinked)");
    expect(modeAHint(true, 0, { isLoading: true, isError: false })).toBe("loading…");
    expect(modeAHint(true, 0, { isLoading: false, isError: true })).toMatch(/error/);
    expect(modeAHint(true, 0, ok)).toBe("hidden (under 5 recent games)");
    expect(modeAHint(true, 3, ok)).toBeNull();
  });
  it("cast.updated also invalidates the lineup query", () => {
    const api = apiSrc;
    expect(api).toMatch(/cast\.updated[\s\S]{0,400}queryKey: \["cast-lineup"\]/);
  });
});

describe("18 · rail resync", () => {
  const base = team("Server v1");
  it("untouched (or reverted) local copy adopts the new server version", () => {
    expect(resyncTeam(base, base, team("Server v2"))).toEqual(team("Server v2"));
    expect(resyncTeam({ ...base }, base, team("Server v2")).name).toBe("Server v2");
  });
  it("diverged local copy is kept", () => {
    expect(resyncTeam(team("Mine"), base, team("Server v2")).name).toBe("Mine");
  });
});

describe("19 · accessibility", () => {
  it("map buttons are labelled with action + map and expose aria-pressed", () => {
    const s = series({ pool: [{ name: "Sky Temple", status: "banned", by: "red" }] });
    const html = renderToStaticMarkup(<PoolPanel series={s} run={async () => {}} />);
    expect(html).toMatch(/aria-label="Ban Red Sky Temple" aria-pressed="true"/);
    expect(html).toMatch(/aria-label="Ban Blue Sky Temple" aria-pressed="false"/);
    expect(html).toContain('aria-label="Reset Sky Temple"');
  });
  it("winner buttons are labelled and disabled consistently with canSetWinner", () => {
    const s = series({ pool: [{ name: "Sky Temple", status: "picked", by: "red" }], games: [g(1, "Sky Temple", null)] });
    const html = renderToStaticMarkup(<PoolPanel series={s} run={async () => {}} />);
    expect(html).toContain('aria-label="Game 1: Blue won"');
    expect(html).not.toMatch(/aria-label="Game 1: Blue won"[^>]*disabled/);
  });
  it("scene tabs: aria-selected = local workspace, ON AIR in text, roving tabindex, tabpanel link", () => {
    const html = renderToStaticMarkup(<SceneTabs onAir="series" local="draft" onPick={() => {}} />);
    expect(html).toMatch(/id="cr-tab-draft" aria-controls="cr-workspace" aria-selected="true" tabindex="0"/);
    expect(html).toMatch(/id="cr-tab-series" aria-controls="cr-workspace" aria-selected="false" tabindex="-1"/);
    expect(html).toMatch(/id="cr-tab-series"[^>]*>Series<i class="cr-air">ON AIR<\/i>/);
  });
  it("arrow keys only move focus (manual activation)", () => {
    expect(tabKeyTarget(0, "ArrowRight", 5)).toBe(1);
    expect(tabKeyTarget(0, "ArrowLeft", 5)).toBe(4);
    expect(tabKeyTarget(3, "Home", 5)).toBe(0);
    expect(tabKeyTarget(1, "End", 5)).toBe(4);
    expect(tabKeyTarget(1, "Enter", 5)).toBeNull();
  });
});

describe("20 · preview first frame", () => {
  it("unmeasured: 16:9 box and hidden iframe (no 0.4-scale flash)", () => {
    expect(previewStyles(null)).toEqual({ box: { aspectRatio: "1920 / 1080" }, frame: { visibility: "hidden" } });
    expect(previewStyles(0.5)).toEqual({ box: { height: 540 }, frame: { transform: "scale(0.5)" } });
    const html = renderToStaticMarkup(<ScenePreview onAir="Series" />);
    expect(html).toContain("visibility:hidden");
    expect(html).not.toContain("scale(0.4)");
  });
});

describe("21 · slide label in auto mode", () => {
  const withSummary = series({ games: [g(1, "A", "blue", {} as GameRecord["summary"])] });
  it("auto: shows the slide really on air (server clock) with an auto tag", () => {
    const l = slideLabel({ ...withSummary, slide: { ...withSummary.slide, auto: true, index: 3 } }, 2);
    expect(l.text).toBe("1 / 5 · auto");
    expect(l.title).toMatch(/server/);
  });
  it("manual: k / n", () => {
    expect(slideLabel({ ...withSummary, slide: { ...withSummary.slide, index: 2 } }).text).toBe("3 / 5");
    expect(slideLabel(series({ slide: { game: null, index: 0, auto: true, interval_ms: 12000, since_ms: 0 } })).text).toBe("1 / 1");
  });
});

describe("22 · forced swap against evidence", () => {
  it("shows a note only when the server reports forced_against_evidence", () => {
    expect(replayNote({ orientation: "forced", forced_against_evidence: true })).toMatch(/against the evidence/);
    expect(replayNote({ orientation: "forced" })).toBeNull();
    expect(replayNote({ orientation: "evidence" })).toBeNull();
  });
  it("the Swap sides checkbox is reset after each load attempt", () => {
    const src = seriesWsSrc;
    expect(src).toMatch(/finally \{[\s\S]{0,80}setSwap\(false\)/);
  });
});

describe("23 · Live/Replay toggle styled in cast.css", () => {
  it("cast.css owns the .cr-subtabs rules (not the Live console's inline sheet)", () => {
    const css = castCss;
    expect(css).toMatch(/\.cr-subtabs button\.on \{/);
    expect(draftWsSrc).not.toContain(".cr-draft>.cr-subtabs");
  });
});

describe("25 · redirects and DraftWorkspace lock", () => {
  it("/draft and /draft/replay redirect to the cast draft workspace", () => {
    expect(LEGACY_REDIRECTS.map((r) => r.path)).toEqual(["draft", "draft/replay"]);
    for (const r of LEGACY_REDIRECTS) {
      const [path, search] = r.to.split("?");
      expect(path).toBe("/cast");
      expect(sceneFromSearch(`?${search}`)).toBe("draft");
    }
    expect(appSrc).toMatch(/LEGACY_REDIRECTS\.map/);
  });
  const draft: DraftState = {
    schema_version: 1, format: "fearless", first_pick: "blue", map: "Cursed Hollow",
    blue: { name: "Alpha", players: ["p1", "p2", "p3", "p4", "p5"] }, red: { name: "Omega", players: [] },
    score: [1, 0], bo: 5,
    steps: [{ order: "first", action: "ban" }, { order: "first", action: "pick" }],
    cursor: 0, assignments: [null, null], manual_unavailable: [], series_bans: [], series_games: [],
  };
  const render = (active: boolean) => {
    const qc = new QueryClient();
    qc.setQueryData(["draft"], draft);
    return renderToStaticMarkup(
      <QueryClientProvider client={qc}><DraftWorkspace series={series({ active })} /></QueryClientProvider>,
    );
  };
  it("locked is derived from series.active", () => {
    expect(render(true)).toMatch(/<button[^>]*disabled=""[^>]*>New series</);
    expect(render(true)).not.toContain('placeholder="Team name"');
    expect(render(false)).not.toMatch(/<button[^>]*disabled=""[^>]*>New series</);
    expect(render(false)).toContain('placeholder="Team name"');
  });
  it("Live/Replay toggle is a pressed-button group", () => {
    expect(render(false)).toMatch(/<div class="cr-subtabs" role="group" aria-label="Draft mode"><button type="button" aria-pressed="true" class="on">Live/);
  });
});
