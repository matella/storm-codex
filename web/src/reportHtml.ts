// Export HTML d'un rapport de scouting : un fichier autonome à envoyer aux coéquipiers.
// Rendu PUR (testé en vitest) : toutes les images arrivent déjà en data: URI via `assets`, tout
// texte venant du LLM ou des replays est échappé (le fichier circule hors de l'app).
// Direction artistique : « dossier de scouting » dans le langage de l'écran de draft HotS —
// chaque carte est un tableau de draft (bans barrés, picks lumineux, picks adverses nominatifs).

import type {
  Count, Evidence, Facts, HeroCall, HeroRow, MapFacts, MapPlan, Point, Rate, ScoutingReport,
} from "./scouting";
import { mapsWithPlans, pidNames } from "./scouting";

export interface ExportAssets {
  /** héros → data: URI du portrait (absent → médaillon à initiales) */
  hero: Record<string, string>;
  /** carte → data: URI de la minimap (absent → dégradé) */
  map: Record<string, string>;
  /** héros → couleur hex de l'univers (anneau du portrait) */
  ring: Record<string, string>;
}

// ── utilitaires purs ────────────────────────────────────────────────────────

export function esc(s: unknown): string {
  return String(s ?? "")
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;")
    .replace(/'/g, "&#39;");
}

const pct = (r: Rate | null | undefined) => (r && r.n ? Math.round((r.k * 100) / r.n) : null);
const rate = (r: Rate | null | undefined) => (r && r.n ? `${r.k}/${r.n}` : "—");
/** Médaillon de repli : 2 lettres (« JO » pour Johanna, « SH » pour Sgt. Hammer). */
const initials = (h: string) => {
  const words = h.replace(/[^A-Za-z0-9 ]/g, "").split(/\s+/).filter(Boolean);
  const s = words.length > 1 ? words.map((w) => w[0]).join("") : (words[0] ?? "");
  return s.slice(0, 2).toUpperCase() || "?";
};
const anchor = (map: string) => "map-" + map.toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/(^-|-$)/g, "");
const fmtLen = (s: number) => `${Math.floor(s / 60)}:${String(Math.floor(s % 60)).padStart(2, "0")}`;
const day = (iso: string | null | undefined) => (iso ? iso.slice(0, 10) : "?");

/** Nom de fichier : `scouting-<titre>.html`. */
export function exportFileName(title: string): string {
  const s = title.toLowerCase().normalize("NFD").replace(/[̀-ͯ]/g, "").replace(/[^a-z0-9]+/g, "-").replace(/(^-|-$)/g, "");
  return `scouting-${s || "report"}.html`;
}

/** Tous les héros cités par le rapport (faits + analyse) — pour charger leurs portraits. */
export function heroesOf(r: ScoutingReport): string[] {
  const set = new Set<string>();
  const f = r.snapshot?.facts;
  const add = (h: string | null | undefined) => { if (h) set.add(h); };
  for (const p of f?.players ?? []) p.heroes.forEach((h) => add(h.hero));
  for (const m of f?.maps ?? []) { m.picks.forEach((h) => add(h.hero)); m.bans.forEach((c) => add(c.key)); m.bans_against.forEach((c) => add(c.key)); }
  const d = f?.draft;
  if (d) [d.bans_first, d.bans_mid, d.bans_against, d.openers, d.last_picks].forEach((l) => l.forEach((c) => add(c.key)));
  (d?.faced ?? []).forEach((h) => add(h.hero));
  for (const g of f?.games ?? []) { g.picks.forEach((p) => add(p.hero)); g.opp_picks.forEach((p) => add(p.hero)); }
  const a = r.analysis;
  if (a) {
    const calls = (l: HeroCall[]) => l.forEach((h) => add(h.hero));
    a.maps.forEach((m) => { calls(m.bans); calls(m.picks); calls(m.their_picks); });
    calls(a.general.bans); calls(a.general.picks);
  }
  return [...set].sort();
}

/** Cartes du rapport (faits + analyse) — pour charger leurs minimaps. */
export function mapsOf(r: ScoutingReport): string[] {
  const set = new Set<string>();
  (r.snapshot?.facts.maps ?? []).forEach((m) => set.add(m.map));
  (r.analysis?.maps ?? []).forEach((m) => set.add(m.map));
  (r.analysis?.map_choice.pick ?? []).forEach((m) => set.add(m.map));
  (r.analysis?.map_choice.avoid ?? []).forEach((m) => set.add(m.map));
  return [...set].sort();
}

// ── briques visuelles ───────────────────────────────────────────────────────

type Tone = "ban" | "pick" | "them" | "plain";

/** Classe CSS d'un héros / d'une carte : l'image n'est embarquée QU'UNE fois (feuille d'assets),
 *  chaque usage la référence — sinon un portrait répété 5 fois pèse 5 fois dans le fichier. */
const cls = (prefix: string, name: string) => prefix + name.toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/(^-|-$)/g, "");

function portrait(hero: string, a: ExportAssets, size: number, tone: Tone = "plain"): string {
  const ring = a.ring[hero] ?? "#afa9ec";
  const has = !!a.hero[hero];
  const inner = has ? "" : `<span class="ini">${esc(initials(hero))}</span>`;
  return `<span class="pt pt-${tone}${has ? ` ${cls("h-", hero)}` : ""}" style="--s:${size}px;--ring:${ring}" role="img" aria-label="${esc(hero)}" title="${esc(hero)}">${inner}${tone === "ban" ? '<i class="slash"></i>' : ""}</span>`;
}

/** Feuille d'assets : une règle par portrait et par carte. Les data: URI (générées par l'export,
 *  jamais par le LLM) sont filtrées par précaution sur un alphabet base64. */
function assetCss(a: ExportAssets): string {
  const safe = (u: string) => /^data:image\/[a-z+]+;base64,[A-Za-z0-9+/=]+$/.test(u);
  const heroes = Object.entries(a.hero).filter(([, u]) => safe(u)).map(([h, u]) => `.${cls("h-", h)}{background-image:url('${u}')}`);
  const maps = Object.entries(a.map).filter(([, u]) => safe(u)).map(([m, u]) => `.${cls("m-", m)}{--img:url('${u}')}`);
  return [...heroes, ...maps].join("\n");
}

/** Contexte de lecture : noms des joueurs (pN → pseudo) et carte du tableau courant. */
interface Ctx { names: Record<string, string>; map?: string }

/** Preuve lisible par un joueur : sans parenthèses techniques, sans répéter la carte du tableau,
 *  pseudos à la place des `pN`. */
export function compactEvidence(e: { label?: string; text?: string }, ctx: Ctx): { label: string; text: string } {
  const pid = (t: string) => t.replace(/\bp(\d+)\b/g, (m) => ctx.names[m] ?? m);
  let label = (e.label ?? "").replace(/\s*\([^)]*\)/g, "").trim();
  if (ctx.map && label.startsWith(`${ctx.map} — `)) label = label.slice(ctx.map.length + 3);
  return { label: pid(label), text: pid(e.text ?? "") };
}

/** Texte libre du LLM : « Solo (p5) » → « Solo », « p3 » seul → son pseudo. */
export function humanize(text: string, names: Record<string, string>): string {
  return text
    .replace(/\s*\(p\d+\)/g, "")
    .replace(/\bp(\d+)\b/g, (m) => names[m] ?? m);
}

function evidenceLine(ev: Evidence[], unsupported: boolean, ctx: Ctx): string {
  if (unsupported) return `<div class="ev ev-bad">⚠ unverified — no supporting fact</div>`;
  const known = ev.filter((e) => e.known).slice(0, 2);
  if (!known.length) return "";
  return `<div class="ev">${known.map((e) => {
    const c = compactEvidence(e, ctx);
    return `<span${e.low ? ' class="low"' : ""}>${esc(c.label)} <b>${esc(c.text)}</b></span>`;
  }).join("")}</div>`;
}

function pips(games: { won: boolean }[]): string {
  return `<span class="pips">${games.map((g) => `<i class="${g.won ? "w" : "l"}"></i>`).join("")}</span>`;
}

function meter(conf: string | null): string {
  const lv = conf === "high" ? 3 : conf === "medium" ? 2 : conf === "low" ? 1 : 0;
  if (!lv) return "";
  return `<span class="meter" title="confidence: ${esc(conf)}"><i class="${lv >= 1 ? "on" : ""}"></i><i class="${lv >= 2 ? "on" : ""}"></i><i class="${lv >= 3 ? "on" : ""}"></i><em>${esc(conf)} confidence</em></span>`;
}

function callCard(h: HeroCall, tone: Tone, a: ExportAssets, names: Record<string, string>, map?: string): string {
  const tag = tone === "ban"
    ? `<span class="tag tag-ban">${h.phase === "mid" ? "MID BAN" : h.phase === "first" ? "1ST PHASE" : "BAN"}</span>`
    : tone === "them" && h.player ? `<span class="tag tag-them">${esc(names[h.player] ?? h.player)}</span>` : "";
  return `<div class="call call-${tone}">
    ${portrait(h.hero, a, 64, tone)}
    <div class="call-body">
      <div class="call-hd"><strong>${esc(h.hero)}</strong>${tag}</div>
      <p>${esc(humanize(h.why, names))}</p>
      ${evidenceLine(h.evidence, h.unsupported, { names, map })}
    </div>
  </div>`;
}

function lane(title: string, cls: string, items: string[], empty: string): string {
  return `<div class="lane ${cls}"><div class="lane-hd">${title}</div>${items.length ? items.join("") : `<div class="lane-empty">${empty}</div>`}</div>`;
}

function points(ps: Point[], ctx: Ctx): string {
  if (!ps.length) return "";
  return `<ul class="consider">${ps.map((p) => `<li><span>${esc(humanize(p.point, ctx.names))}</span>${evidenceLine(p.evidence, p.unsupported, ctx)}</li>`).join("")}</ul>`;
}

function chips(list: Count[], a: ExportAssets, tone: Tone, max = 6): string {
  if (!list.length) return `<span class="none">—</span>`;
  return list.slice(0, max).map((c) =>
    `<span class="chip">${portrait(c.key, a, 26, tone)}<span>${esc(c.key)}</span><b>${c.count.k}×</b></span>`).join("");
}

function pickChips(list: HeroRow[], a: ExportAssets, names: Record<string, string>): string {
  if (!list.length) return `<span class="none">—</span>`;
  return list.map((h) => {
    const who = (h.by ?? []).map((p) => names[p] ?? p);
    return `<span class="chip">${portrait(h.hero, a, 26)}<span>${esc(h.hero)}${who.length ? ` <em>${esc([...new Set(who)].join(", "))}</em>` : ""}</span><b>${h.picks.k}×</b></span>`;
  }).join("");
}

// ── sections ────────────────────────────────────────────────────────────────

function cover(r: ScoutingReport, f: Facts | null, a: ExportAssets, generatedAt: string): string {
  const o = f?.overview;
  const bgMap = f?.maps[0]?.map;
  const bg = bgMap && a.map[bgMap] ? cls("m-", bgMap) : "";
  const games = f?.games ?? [];
  const low = (o?.games ?? 0) < 5;
  const wl = o ? `${o.record.k}<span>–</span>${o.record.n - o.record.k}` : "—";
  const roster = (f?.players ?? []).filter((p) => p.core);
  return `<header class="cover ${bg}">
    <div class="cover-grain"></div>
    <div class="cover-in">
      <div class="kicker">Scouting dossier · report #${r.id}</div>
      <h1>${esc(r.target_name ?? "Unnamed team")}</h1>
      <div class="subtitle">${esc(r.title)}</div>
      <div class="statline">
        <div class="stat big"><label>Record</label><strong>${wl}</strong>${pips(games)}</div>
        <div class="stat"><label>Games</label><strong>${o?.games ?? 0}</strong><small>${o ? `${day(o.first_date)} → ${day(o.last_date)}` : ""}</small></div>
        <div class="stat"><label>Had first pick</label><strong>${pct(f?.draft.first_pick) ?? "—"}${pct(f?.draft.first_pick) != null ? "<span>%</span>" : ""}</strong><small>${rate(f?.draft.first_pick)}</small></div>
        <div class="stat"><label>Avg length</label><strong>${o ? fmtLen(o.avg_length_s) : "—"}</strong><small>builds ${esc(o?.builds.join(", ") || "?")}</small></div>
        ${low ? `<div class="sample">⚠ Small sample — ${o?.games ?? 0} game${o?.games === 1 ? "" : "s"}. Treat trends as hints, not certainties.</div>` : ""}
      </div>
      ${roster.length ? `<div class="roster">${roster.map((p) => {
        const top = p.heroes[0]?.hero;
        const role = p.roles[0]?.key ?? "";
        return `<div class="rm">${top ? portrait(top, a, 58) : ""}<div><strong>${esc(p.name)}</strong><span>${esc(role)}</span><em>${p.heroes.slice(0, 3).map((h) => esc(h.hero)).join(" · ")}</em></div></div>`;
      }).join("")}</div>` : ""}
    </div>
    <div class="cover-meta">Generated ${esc(generatedAt)} · facts v${r.facts_version}${r.analysis_model ? ` · plan by ${esc(r.analysis_model)}` : ""}</div>
  </header>`;
}

function identity(r: ScoutingReport, f: Facts, a: ExportAssets, names: Record<string, string>): string {
  const d = f.draft;
  const summary = r.analysis?.summary;
  return `<section class="sec reveal" id="identity">
    <div class="sec-hd"><span class="num">01</span><h2>Draft identity</h2></div>
    <div class="identity">
      ${summary ? `<blockquote>${esc(humanize(summary, names))}</blockquote>` : `<blockquote class="muted">No draft plan imported yet — the numbers below come straight from the replays.</blockquote>`}
      <div class="tells">
        <div class="tell"><label>They ban first</label><div class="chips">${chips(d.bans_first, a, "ban")}</div></div>
        <div class="tell"><label>They ban mid-draft</label><div class="chips">${chips(d.bans_mid, a, "ban")}</div></div>
        <div class="tell"><label>Their first pick</label><div class="chips">${chips(d.openers, a, "them")}</div></div>
        <div class="tell"><label>Banned against them</label><div class="chips">${chips(d.bans_against, a, "plain")}</div></div>
      </div>
    </div>
  </section>`;
}

function mapChoice(r: ScoutingReport, a: ExportAssets, names: Record<string, string>): string {
  const c = r.analysis?.map_choice;
  if (!c || (!c.pick.length && !c.avoid.length)) return "";
  const tile = (m: { map: string; why: string; evidence: Evidence[]; unsupported: boolean }, kind: "go" | "no") =>
    `<a class="mtile mtile-${kind}${a.map[m.map] ? ` ${cls("m-", m.map)}` : ""}" href="#${anchor(m.map)}">
      <span class="mtile-badge">${kind === "go" ? "PICK" : "AVOID"}</span>
      <strong>${esc(m.map)}</strong>
      <p>${esc(humanize(m.why, names))}</p>
      ${evidenceLine(m.evidence, m.unsupported, { names, map: m.map })}
    </a>`;
  return `<section class="sec reveal" id="map-choice">
    <div class="sec-hd"><span class="num">02</span><h2>Map choice</h2><small>when we get to pick the map</small></div>
    <div class="mtiles">${c.pick.map((m) => tile(m, "go")).join("")}${c.avoid.map((m) => tile(m, "no")).join("")}</div>
  </section>`;
}

function mapBoard(map: string, m: MapFacts | null, plan: MapPlan | null, f: Facts, a: ExportAssets, names: Record<string, string>, i: number): string {
  const games = f.games.filter((g) => g.map === map);
  const img = a.map[map] ? cls("m-", map) : "";
  const record = m ? `<span class="rec">${m.record.k}–${m.record.n - m.record.k}</span>${pips(games)}` : `<span class="rec none">not in these replays</span>`;
  const wil = m?.wilson ? `<span class="ci">95% CI ${Math.round(m.wilson[0])}–${Math.round(m.wilson[1])}%</span>` : "";
  const lastTime = m ? `<div class="lasttime">
      <div><label>They played</label><div class="chips">${pickChips(m.picks, a, names)}</div></div>
      <div><label>They banned</label><div class="chips">${chips(m.bans, a, "ban")}</div></div>
      <div><label>Banned against them</label><div class="chips">${chips(m.bans_against, a, "plain")}</div></div>
    </div>` : "";
  const board = plan
    ? `<div class="board">
        ${lane("We ban", "l-ban", plan.bans.map((h) => callCard(h, "ban", a, names, map)), "no ban suggested")}
        ${lane("We pick", "l-pick", plan.picks.map((h) => callCard(h, "pick", a, names, map)), "no pick suggested")}
        ${lane("They will likely pick", "l-them", plan.their_picks.map((h) => callCard(h, "them", a, names, map)), "—")}
      </div>
      ${plan.considerations.length ? `<div class="consider-wrap"><div class="lane-hd">Consider</div>${points(plan.considerations, { names, map })}</div>` : ""}`
    : `<div class="noplan">No plan for this map — see <a href="#general">Any map</a>.</div>`;
  return `<article class="mboard reveal" id="${anchor(map)}">
    <div class="mboard-hd ${img}">
      <div class="mboard-title">
        <span class="idx">MAP ${String(i + 1).padStart(2, "0")}</span>
        <h3>${esc(map)}</h3>
        <div class="mboard-rec">${record}${wil}</div>
      </div>
      ${plan ? meter(plan.confidence) : ""}
    </div>
    ${plan?.overview ? `<p class="overview">${esc(humanize(plan.overview, names))}</p>${evidenceLine(plan.evidence, plan.unsupported, { names, map })}` : ""}
    ${board}
    ${lastTime}
  </article>`;
}

function general(r: ScoutingReport, a: ExportAssets, names: Record<string, string>): string {
  const g = r.analysis?.general;
  if (!g || g.bans.length + g.picks.length + g.considerations.length === 0) return "";
  return `<section class="sec reveal" id="general">
    <div class="sec-hd"><span class="num">04</span><h2>Any map</h2><small>applies everywhere, including maps missing from the replays</small></div>
    <div class="board board-2">
      ${lane("We ban", "l-ban", g.bans.map((h) => callCard(h, "ban", a, names)), "—")}
      ${lane("We pick", "l-pick", g.picks.map((h) => callCard(h, "pick", a, names)), "—")}
    </div>
    ${g.considerations.length ? `<div class="consider-wrap"><div class="lane-hd">Consider</div>${points(g.considerations, { names })}</div>` : ""}
  </section>`;
}

function players(f: Facts, a: ExportAssets): string {
  if (!f.players.length) return "";
  return `<section class="sec reveal" id="players">
    <div class="sec-hd"><span class="num">05</span><h2>Their players</h2></div>
    <div class="pcards">${f.players.map((p) => {
      const max = Math.max(1, ...p.heroes.map((h) => h.picks.k));
      return `<div class="pcard">
        <div class="pcard-hd">${p.heroes[0] ? portrait(p.heroes[0].hero, a, 72) : ""}
          <div><strong>${esc(p.name)}</strong><span>${esc(p.roles.map((r) => r.key).join(" / "))}${p.core ? "" : " · substitute"}</span><em>${rate(p.record)} wins</em></div>
        </div>
        <div class="pool">${p.heroes.map((h) => `<div class="pool-row">${portrait(h.hero, a, 24)}<span>${esc(h.hero)}</span>
            <i class="bar"><b style="width:${Math.round((h.picks.k * 100) / max)}%"></b></i><em>${h.picks.k}g · ${pct(h.record)}%</em></div>`).join("")}</div>
        <div class="pstats">K/D/A <b>${p.stats.kills.toFixed(1)}/${p.stats.deaths.toFixed(1)}/${p.stats.assists.toFixed(1)}</b> · dmg <b>${p.stats.hero_damage_pm.toFixed(0)}</b>/min · dead <b>${p.stats.time_dead_pct.toFixed(1)}%</b></div>
      </div>`;
    }).join("")}</div>
  </section>`;
}

function faced(f: Facts, a: ExportAssets): string {
  const list = [...(f.draft.faced ?? [])].sort((x, y) => (pct(x.record) ?? 0) - (pct(y.record) ?? 0) || y.picks.k - x.picks.k);
  if (!list.length) return "";
  return `<section class="sec reveal" id="faced">
    <div class="sec-hd"><span class="num">06</span><h2>What they faced</h2><small>heroes picked against them · their record in those games · <b class="lg-hot">green</b> = they never beat it</small></div>
    <div class="faced">${list.map((h) => {
      const p = pct(h.record) ?? 0;
      return `<div class="fc ${p === 0 ? "fc-hot" : p === 100 ? "fc-cold" : ""}">${portrait(h.hero, a, 44)}<span>${esc(h.hero)}</span><em>${rate(h.record)}</em></div>`;
    }).join("")}</div>
  </section>`;
}

function gamesLog(f: Facts, a: ExportAssets, names: Record<string, string>): string {
  if (!f.games.length) return "";
  const row = (ps: { hero: string; player: string }[], mine: boolean) =>
    `<div class="gl-picks">${ps.map((p) => `<span>${portrait(p.hero, a, 30)}<em>${esc(mine ? names[p.player] ?? p.player : p.player)}</em></span>`).join("")}</div>`;
  return `<section class="sec reveal" id="games">
    <div class="sec-hd"><span class="num">07</span><h2>Game log</h2></div>
    <div class="glog">${f.games.map((g) => `<div class="gl ${g.won ? "gl-w" : "gl-l"}">
      <div class="gl-meta"><strong>${g.won ? "WIN" : "LOSS"}</strong><span>${esc(g.map)}</span><em>${day(g.date)} · ${fmtLen(g.length_s)} · ${g.first_pick === true ? "they had first pick" : g.first_pick === false ? "opponent had first pick" : ""}</em></div>
      <div class="gl-side"><label>Them</label>${row(g.picks, true)}<div class="gl-bans"><label>bans</label>${g.bans.map((b) => portrait(b, a, 22, "ban")).join("")}</div></div>
      <div class="gl-side"><label>Opponent</label>${row(g.opp_picks, false)}<div class="gl-bans"><label>bans</label>${g.bans_against.map((b) => portrait(b, a, 22, "ban")).join("")}</div></div>
    </div>`).join("")}</div>
  </section>`;
}

// ── document ────────────────────────────────────────────────────────────────

export function renderReportHtml(r: ScoutingReport, a: ExportAssets, generatedAt: string): string {
  const f = r.snapshot?.facts ?? null;
  const names = pidNames(f);
  const rows = mapsWithPlans(f, r.analysis?.maps);
  const nav = rows.map((x) => `<a href="#${anchor(x.map)}">${esc(x.map)}</a>`).join("");
  const body = f && f.overview.games > 0
    ? `${identity(r, f, a, names)}
       ${mapChoice(r, a, names)}
       <section class="sec" id="maps">
         <div class="sec-hd reveal"><span class="num">03</span><h2>Map by map</h2><small>bans · picks · what they will play</small></div>
         ${rows.map((x, i) => mapBoard(x.map, x.facts, x.plan, f, a, names, i)).join("")}
       </section>
       ${general(r, a, names)}
       ${players(f, a)}
       ${faced(f, a)}
       ${gamesLog(f, a, names)}`
    : `<section class="sec"><p class="muted">This report has no analysed game yet.</p></section>`;
  return `<!doctype html>
<html lang="en"><head>
<meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1">
<title>${esc(r.target_name ?? "Scouting")} — ${esc(r.title)}</title>
<link rel="preconnect" href="https://fonts.googleapis.com"><link rel="preconnect" href="https://fonts.gstatic.com" crossorigin>
<link href="https://fonts.googleapis.com/css2?family=Big+Shoulders+Display:wght@600;800;900&family=Figtree:wght@400;500;600&family=JetBrains+Mono:wght@400;600&display=swap" rel="stylesheet">
<style>${CSS}</style>
<style>${assetCss(a)}</style>
</head><body>
<nav class="top"><span class="brand">STORM CODEX <i>·</i> SCOUTING</span><div class="top-maps">${nav}</div></nav>
${cover(r, f, a, generatedAt)}
<main>${body}</main>
<footer>Numbers computed by Storm Codex from ${f?.overview.games ?? 0} replay${f?.overview.games === 1 ? "" : "s"} · report #${r.id} · facts v${r.facts_version}${r.analysis ? ` · draft plan ${r.analysis_model ? `by ${esc(r.analysis_model)}` : "imported"}${r.analysis_imported_at ? ` on ${day(r.analysis_imported_at)}` : ""}` : ""}.<br>Recommendations marked ⚠ cite no supporting fact. Dashed facts rest on fewer than 3 games.</footer>
</body></html>`;
}

// ── styles ──────────────────────────────────────────────────────────────────

const CSS = `
:root{--bg:#06070b;--panel:#0d0f16;--panel2:#12141d;--line:#1f2231;--line2:#2a2e42;--text:#e8eaf2;--text2:#c5c9d8;--muted:#8d93a8;--kicker:#5d6275;
--accent:#7f77dd;--accent2:#afa9ec;--win:#5dcaa5;--loss:#e24b4a;--loss2:#f7c1c1;--gold:#fac775;--them:#f0a35e;
--display:"Big Shoulders Display","Arial Narrow","Roboto Condensed",sans-serif;--body:"Figtree","Segoe UI",sans-serif;--mono:"JetBrains Mono",Consolas,monospace}
*{box-sizing:border-box}html{scroll-behavior:smooth;-webkit-print-color-adjust:exact;print-color-adjust:exact}
body{margin:0;background:var(--bg);color:var(--text);font:14px/1.55 var(--body);
background-image:radial-gradient(1200px 600px at 85% -10%,rgba(127,119,221,.14),transparent 60%),radial-gradient(900px 500px at -10% 30%,rgba(93,202,165,.05),transparent 60%)}
a{color:inherit}
.top{position:sticky;top:0;z-index:20;display:flex;align-items:center;gap:18px;padding:10px 28px;background:rgba(6,7,11,.82);backdrop-filter:blur(10px);border-bottom:1px solid var(--line)}
.brand{font:800 15px var(--display);letter-spacing:.14em;white-space:nowrap}.brand i{color:var(--accent);font-style:normal}
.top-maps{display:flex;gap:6px;overflow-x:auto;scrollbar-width:none}.top-maps a{font:600 11px var(--mono);color:var(--muted);text-decoration:none;padding:4px 10px;border:1px solid var(--line2);border-radius:20px;white-space:nowrap}
.top-maps a:hover{color:var(--text);border-color:var(--accent)}
/* cover */
.cover{position:relative;overflow:hidden;padding:64px 28px 28px;border-bottom:1px solid var(--line2);isolation:isolate}
.cover::before{content:"";position:absolute;inset:-40px;background:transparent center/cover no-repeat;background-image:var(--img,none);filter:blur(3px) saturate(.9);opacity:.28;z-index:-2;
mask-image:linear-gradient(100deg,transparent 25%,#000 75%);-webkit-mask-image:linear-gradient(100deg,transparent 25%,#000 75%)}
.cover-grain{position:absolute;inset:0;z-index:-1;background:repeating-linear-gradient(0deg,rgba(255,255,255,.018) 0 1px,transparent 1px 3px),linear-gradient(180deg,transparent 60%,var(--bg))}
.cover-in{max-width:1180px;margin:0 auto}
.kicker{font:600 11px var(--mono);letter-spacing:.2em;text-transform:uppercase;color:var(--accent2);display:flex;align-items:center;gap:10px}
.kicker::before{content:"";width:28px;height:2px;background:var(--accent)}
h1{font:900 clamp(56px,10vw,120px)/.86 var(--display);text-transform:uppercase;letter-spacing:.01em;margin:14px 0 6px;
background:linear-gradient(180deg,#fff 30%,#b9b4ee);-webkit-background-clip:text;background-clip:text;color:transparent}
.subtitle{font:500 16px var(--body);color:var(--text2)}
.statline{display:flex;flex-wrap:wrap;gap:12px;margin:30px 0 26px;align-items:stretch}
.stat{background:rgba(13,15,22,.72);border:1px solid var(--line2);border-radius:12px;padding:12px 18px;min-width:140px;backdrop-filter:blur(4px)}
.stat label,.tell label,.lasttime label,.gl-side label{display:block;font:600 10px var(--mono);letter-spacing:.14em;text-transform:uppercase;color:var(--kicker);margin-bottom:4px}
.stat strong{font:800 34px/1 var(--display);display:block}.stat strong span{color:var(--muted);font-weight:600;margin:0 2px;font-size:.7em}
.stat small{font:11px var(--mono);color:var(--muted)}.stat.big strong{font-size:46px}
.pips{display:inline-flex;gap:4px;margin-top:8px}.pips i{width:14px;height:6px;border-radius:2px;background:var(--loss)}.pips i.w{background:var(--win)}
.sample{align-self:center;font:500 12px var(--body);color:var(--gold);background:rgba(250,199,117,.07);border:1px dashed rgba(250,199,117,.35);padding:10px 14px;border-radius:10px;max-width:320px}
.roster{display:flex;flex-wrap:wrap;gap:10px}
.rm{display:flex;gap:12px;align-items:center;background:linear-gradient(135deg,rgba(18,20,29,.9),rgba(13,15,22,.6));border:1px solid var(--line2);border-radius:40px 14px 14px 40px;padding:6px 18px 6px 6px;min-width:210px}
.rm strong{display:block;font:800 19px/1.05 var(--display);letter-spacing:.03em}.rm span{font:600 10px var(--mono);color:var(--accent2);text-transform:uppercase;letter-spacing:.1em}
.rm em{display:block;font-style:normal;font-size:11px;color:var(--muted)}
.cover-meta{max-width:1180px;margin:22px auto 0;font:11px var(--mono);color:var(--kicker)}
/* portraits */
.pt{--s:40px;position:relative;display:inline-grid;place-items:center;width:var(--s);height:var(--s);border-radius:50%;flex-shrink:0;
background:radial-gradient(circle at 30% 25%,#23263a,#0d0f16);box-shadow:0 0 0 2px var(--ring),0 4px 14px rgba(0,0,0,.45);overflow:hidden;transition:transform .2s}
.pt[class*=" h-"]{background-size:cover;background-position:center}.pt .ini{font:800 calc(var(--s)*.36) var(--display);color:var(--ring);letter-spacing:.02em}
.pt-ban{filter:grayscale(1) brightness(.62);box-shadow:0 0 0 2px var(--loss),0 4px 14px rgba(0,0,0,.45)}
.pt-ban .slash{position:absolute;inset:0;background:linear-gradient(135deg,transparent calc(50% - 1.5px),var(--loss) calc(50% - 1.5px),var(--loss) calc(50% + 1.5px),transparent calc(50% + 1.5px))}
.pt-pick{box-shadow:0 0 0 2px var(--win),0 0 18px rgba(93,202,165,.45)}
.pt-them{box-shadow:0 0 0 2px var(--them),0 0 16px rgba(240,163,94,.35)}
/* sections */
main{max-width:1180px;margin:0 auto;padding:10px 28px 40px}
.sec{margin:54px 0 0}
.sec-hd{display:flex;align-items:baseline;gap:14px;margin-bottom:18px;border-bottom:1px solid var(--line);padding-bottom:10px}
.sec-hd .num{font:600 12px var(--mono);color:var(--accent)}.sec-hd h2{font:800 34px/1 var(--display);text-transform:uppercase;letter-spacing:.04em;margin:0}
.sec-hd small{margin-left:auto;font:11px var(--mono);color:var(--muted)}
.identity{display:grid;grid-template-columns:1.1fr 1fr;gap:24px}
blockquote{margin:0;font:500 19px/1.6 var(--body);color:var(--text);border-left:3px solid var(--accent);padding:4px 0 4px 22px}
blockquote.muted,.muted{color:var(--muted)}
.tells{display:grid;grid-template-columns:1fr 1fr;gap:12px}
.tell,.lasttime>div{background:var(--panel);border:1px solid var(--line);border-radius:12px;padding:12px 14px}
.chips{display:flex;flex-wrap:wrap;gap:6px}
.chip{display:inline-flex;align-items:center;gap:7px;background:var(--panel2);border:1px solid var(--line);border-radius:30px;padding:3px 10px 3px 3px;font-size:12px}
.chip b{font:600 11px var(--mono);color:var(--accent2)}.chip em{font-style:normal;color:var(--muted);font-size:11px}.none{color:var(--kicker)}
/* map choice */
.mtiles{display:grid;grid-template-columns:repeat(auto-fill,minmax(260px,1fr));gap:14px}
.mtile{position:relative;display:block;text-decoration:none;border-radius:14px;padding:18px 18px 16px;border:1px solid var(--line2);overflow:hidden;isolation:isolate;min-height:170px}
.mtile::before{content:"";position:absolute;inset:0;z-index:-1;background:var(--panel) center/cover no-repeat;background-image:var(--img,none);opacity:.35;transition:opacity .25s,transform .4s}
.mtile::after{content:"";position:absolute;inset:0;z-index:-1;background:linear-gradient(160deg,rgba(6,7,11,.2),rgba(6,7,11,.92) 65%)}
.mtile:hover::before{opacity:.5;transform:scale(1.04)}
.mtile-go{box-shadow:inset 0 3px 0 var(--win)}.mtile-no{box-shadow:inset 0 3px 0 var(--loss)}
.mtile-badge{font:700 10px var(--mono);letter-spacing:.16em;padding:3px 8px;border-radius:4px}
.mtile-go .mtile-badge{background:rgba(93,202,165,.16);color:var(--win)}.mtile-no .mtile-badge{background:rgba(226,75,74,.16);color:var(--loss2)}
.mtile strong{display:block;font:800 28px/1 var(--display);text-transform:uppercase;margin:12px 0 6px}.mtile p{margin:0;color:var(--text2);font-size:13px}
/* map boards */
.mboard{margin:22px 0 34px;background:var(--panel);border:1px solid var(--line2);border-radius:18px;overflow:hidden}
.mboard-hd{position:relative;display:flex;align-items:flex-end;justify-content:space-between;gap:18px;padding:34px 24px 20px;min-height:170px;isolation:isolate;border-bottom:1px solid var(--line2)}
.mboard-hd::before{content:"";position:absolute;inset:0;z-index:-2;background:linear-gradient(135deg,#1a1c2b,#0d0f16) center/cover no-repeat;background-image:var(--img,none);
clip-path:polygon(38% 0,100% 0,100% 100%,24% 100%);opacity:.75}
.mboard-hd::after{content:"";position:absolute;inset:0;z-index:-1;background:linear-gradient(90deg,var(--panel) 30%,rgba(13,15,22,.55) 60%,rgba(13,15,22,.15)),linear-gradient(0deg,var(--panel),transparent 55%)}
.idx{font:600 11px var(--mono);letter-spacing:.2em;color:var(--accent2)}
.mboard h3{font:900 clamp(38px,6vw,60px)/.9 var(--display);text-transform:uppercase;margin:6px 0 10px;letter-spacing:.01em}
.mboard-rec{display:flex;align-items:center;gap:12px;font:600 13px var(--mono)}.rec{font:800 24px var(--display)}.rec.none{font:500 12px var(--mono);color:var(--muted)}
.mboard-rec .pips{margin:0}.ci{color:var(--muted);font-size:11px}
.meter{display:inline-flex;align-items:center;gap:4px}.meter i{width:22px;height:8px;border-radius:2px;background:var(--line2)}.meter i.on{background:var(--gold)}
.meter em{font:600 10px var(--mono);letter-spacing:.12em;text-transform:uppercase;color:var(--gold);margin-left:6px;font-style:normal}
.overview{margin:18px 24px 0;font:500 15px/1.6 var(--body);color:var(--text)}
.overview + .ev{margin:6px 24px 0}
.board{display:grid;grid-template-columns:repeat(3,1fr);gap:1px;background:var(--line);margin-top:18px;border-top:1px solid var(--line);border-bottom:1px solid var(--line)}
.board-2{grid-template-columns:repeat(2,1fr);border:1px solid var(--line);border-radius:14px;overflow:hidden}
.lane{background:var(--panel);padding:16px 16px 18px}
.lane-hd{font:800 15px var(--display);letter-spacing:.16em;text-transform:uppercase;margin-bottom:12px;display:flex;align-items:center;gap:8px}
.lane-hd::before{content:"";width:8px;height:8px;transform:rotate(45deg);background:var(--accent)}
.l-ban .lane-hd{color:var(--loss2)}.l-ban .lane-hd::before{background:var(--loss)}
.l-pick .lane-hd{color:var(--win)}.l-pick .lane-hd::before{background:var(--win)}
.l-them .lane-hd{color:var(--them)}.l-them .lane-hd::before{background:var(--them)}
.lane-empty{color:var(--kicker);font:12px var(--mono)}
.call{display:flex;gap:14px;align-items:flex-start;padding:10px 0;border-top:1px dashed var(--line)}.call:first-of-type{border-top:0;padding-top:0}
.call:hover .pt{transform:translateY(-2px) scale(1.04)}
.call-hd{display:flex;align-items:center;gap:8px;flex-wrap:wrap}.call-hd strong{font:800 21px/1 var(--display);letter-spacing:.02em;text-transform:uppercase}
.call p{margin:4px 0 0;font-size:13px;color:var(--text2)}
.tag{font:700 9px var(--mono);letter-spacing:.12em;padding:2px 7px;border-radius:4px;text-transform:uppercase}
.tag-ban{background:rgba(226,75,74,.15);color:var(--loss2)}.tag-them{background:rgba(240,163,94,.14);color:var(--them)}
.ev{display:flex;flex-direction:column;gap:2px;margin-top:6px;font:11px/1.4 var(--mono);color:var(--muted)}
.ev span{border-left:2px solid var(--line2);padding-left:7px}.ev b{color:var(--text2);font-weight:600}
.ev .low{border-left-style:dashed;opacity:.75}.ev-bad{color:var(--gold)}
.consider-wrap{padding:16px 24px 4px}.consider{list-style:none;margin:0;padding:0;display:grid;gap:8px}
.consider li{position:relative;padding:10px 14px 10px 30px;background:var(--panel2);border:1px solid var(--line);border-radius:10px;font-size:13.5px}
.consider li::before{content:"";position:absolute;left:12px;top:17px;width:7px;height:7px;transform:rotate(45deg);background:var(--gold)}
.lasttime{display:grid;grid-template-columns:repeat(3,1fr);gap:10px;padding:16px 24px 22px}
.lasttime>div{background:var(--panel2)}
.noplan{padding:18px 24px;color:var(--muted);font-size:13px}.noplan a{color:var(--accent2)}
/* players */
.pcards{display:grid;grid-template-columns:repeat(auto-fill,minmax(270px,1fr));gap:14px}
.pcard{background:var(--panel);border:1px solid var(--line2);border-radius:16px;padding:16px}
.pcard-hd{display:flex;gap:14px;align-items:center;margin-bottom:14px}
.pcard-hd strong{display:block;font:800 26px/1 var(--display);letter-spacing:.02em}.pcard-hd span{display:block;font:600 10px var(--mono);letter-spacing:.1em;text-transform:uppercase;color:var(--accent2);margin:3px 0}
.pcard-hd em{font:11px var(--mono);color:var(--muted);font-style:normal}
.pool-row{display:grid;grid-template-columns:24px 1fr 70px 70px;gap:9px;align-items:center;padding:4px 0;font-size:12.5px}
.bar{height:6px;background:var(--line);border-radius:3px;overflow:hidden}.bar b{display:block;height:100%;background:linear-gradient(90deg,var(--accent),var(--accent2))}
.pool-row em{font:11px var(--mono);color:var(--muted);font-style:normal;text-align:right}
.pstats{margin-top:12px;padding-top:10px;border-top:1px solid var(--line);font:11px var(--mono);color:var(--muted)}.pstats b{color:var(--text2)}
/* faced */
.faced{display:flex;flex-wrap:wrap;gap:10px}
.fc{display:flex;flex-direction:column;align-items:center;gap:6px;width:92px;padding:12px 6px;background:var(--panel);border:1px solid var(--line);border-radius:12px;font-size:12px;text-align:center}
.fc em{font:600 11px var(--mono);font-style:normal;color:var(--muted)}
.fc-hot{border-color:rgba(93,202,165,.5);background:linear-gradient(180deg,rgba(93,202,165,.10),var(--panel))}.fc-hot em{color:var(--win)}
.fc-cold em{color:var(--loss2)}.lg-hot{color:var(--win);font-weight:600}
/* games */
.glog{display:grid;gap:10px}
.gl{display:grid;grid-template-columns:200px 1fr 1fr;gap:16px;align-items:center;background:var(--panel);border:1px solid var(--line);border-left:4px solid var(--loss);border-radius:12px;padding:14px 16px}
.gl-w{border-left-color:var(--win)}
.gl-meta strong{display:block;font:900 22px/1 var(--display);letter-spacing:.06em}.gl-w .gl-meta strong{color:var(--win)}.gl-l .gl-meta strong{color:var(--loss2)}
.gl-meta span{display:block;font:700 14px var(--body);margin-top:4px}.gl-meta em{display:block;font:11px var(--mono);color:var(--muted);font-style:normal;margin-top:2px}
.gl-picks{display:flex;gap:8px;flex-wrap:wrap}.gl-picks span{display:flex;flex-direction:column;align-items:center;gap:3px}
.gl-picks em{font:10px var(--mono);font-style:normal;color:var(--muted);max-width:64px;overflow:hidden;text-overflow:ellipsis;white-space:nowrap}
.gl-bans{display:flex;gap:5px;margin-top:8px;align-items:center}.gl-bans label{font:600 9px var(--mono);letter-spacing:.12em;text-transform:uppercase;color:var(--kicker);margin-right:4px}
footer{max-width:1180px;margin:30px auto 0;padding:22px 28px 40px;border-top:1px solid var(--line);font:11px/1.7 var(--mono);color:var(--kicker)}
/* motion */
@keyframes rise{from{opacity:0;transform:translateY(14px)}to{opacity:1;transform:none}}
.cover-in>*{animation:rise .7s cubic-bezier(.2,.8,.2,1) both}
.cover-in>*:nth-child(2){animation-delay:.08s}.cover-in>*:nth-child(3){animation-delay:.14s}.cover-in>*:nth-child(4){animation-delay:.22s}.cover-in>*:nth-child(5){animation-delay:.3s}
.reveal{animation:rise .7s cubic-bezier(.2,.8,.2,1) both;animation-timeline:view();animation-range:entry 0% entry 30%}
@media (prefers-reduced-motion:reduce){*{animation:none!important;transition:none!important}}
/* responsive */
@media (max-width:860px){.identity{grid-template-columns:1fr}.board{grid-template-columns:1fr}.board-2{grid-template-columns:1fr}.lasttime{grid-template-columns:1fr}
.gl{grid-template-columns:1fr}.tells{grid-template-columns:1fr}.top{padding:10px 16px}main,.cover{padding-left:16px;padding-right:16px}.sec-hd small{display:none}
.mboard-hd{flex-direction:column;align-items:flex-start}.mboard-hd::before{clip-path:none;opacity:.35}}
/* print */
@media print{.top{position:static}.mboard,.pcard,.gl,.mtile{break-inside:avoid}.reveal,.cover-in>*{animation:none}body{background-image:none}}
`;
