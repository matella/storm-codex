// Pièces du design system propres aux parties : mode, verdict, carte de partie, courbe d'avance de
// niveau. Styles : src/ds.css (lot 1).

import type { CSSProperties } from "react";
import { Link } from "react-router-dom";
import { awardLabel, fmtDur, minimapImage, modeBadge } from "../../api";
import type { MatchPlayer, MatchSummary } from "../../api";
import { kda } from "../../session";
import { Portrait } from "./index";

/** `--img` pour un fond d'art de carte (minimap in-game) ; indéfini si la carte n'en a pas. */
export function mapArt(map: string | null | undefined): CSSProperties | undefined {
  const u = minimapImage(map ?? null);
  return u ? ({ "--img": `url('${u}')` } as CSSProperties) : undefined;
}

/** Les cartes ARAM n'ont pas de minimap bakée : liste des slugs connus (assets/minimaps). */
const HAS_MINIMAP = new Set([
  "alterac-pass", "battlefield-of-eternity", "blackhearts-bay", "braxis-holdout", "cursed-hollow", "dragon-shire",
  "garden-of-terror", "hanamura-temple", "haunted-mines", "infernal-shrines", "sky-temple", "tomb-of-the-spider-queen",
  "towers-of-doom", "volskaya-foundry", "warhead-junction",
]);
export const hasMapArt = (map: string | null | undefined) =>
  !!map && HAS_MINIMAP.has(map.toLowerCase().replace(/['']/g, "").replace(/\s+/g, "-"));

/** Étiquette de mode (Storm League, ARAM…) ; classe de couleur selon la famille. */
export function ModeTag({ mode, short = false }: { mode: number | null; short?: boolean }) {
  const b = modeBadge(mode);
  const fam = b.cls === "b-sl" ? "sl" : b.short === "ARAM" ? "aram" : "qm";
  const long: Record<string, string> = { SL: "Storm League", HL: "Hero League", TL: "Team League", UD: "Unranked", QM: "Quick Match", ARAM: "ARAM", CUSTOM: "Custom" };
  return <span className={`ds-mode ${fam}`} title={long[b.short] ?? b.short}>{short ? b.short : long[b.short] ?? b.short}</span>;
}

/** Étiquette d'award (MVP en or, les autres en contour). */
export function AwardTag({ raw }: { raw: string | null | undefined }) {
  const aw = awardLabel(raw);
  if (!aw) return null;
  return aw.mvp ? <span className="ds-tag mvp">👑 MVP</span> : <span className="ds-tag award" title={aw.label}>{aw.icon} {aw.label}</span>;
}

export function Kda({ p }: { p: MatchPlayer }) {
  const { k, d, a } = kda(p);
  return <span className="ds-kda">{k}<i>/</i>{d}<i>/</i>{a}</span>;
}

/** Carte d'une partie de l'opérateur (Session) : bandeau de carte, héros, K/D/A, résultat, award. */
export function GameCard({ m, me, won }: { m: MatchSummary; me: MatchPlayer; won: boolean }) {
  const art = hasMapArt(m.map);
  const t = m.played_at ? new Date(m.played_at).toLocaleTimeString("fr-FR", { hour: "2-digit", minute: "2-digit" }) : "";
  const aw = awardLabel(me.award);
  return (
    <Link to={`/match/${m.id}`} className={`ds-gcard ${won ? "w" : "l"}`}>
      <div className={art ? "ds-gcard-map" : "ds-gcard-map noart"} style={art ? mapArt(m.map) : undefined}>
        <ModeTag mode={m.mode} /><span className="time">{t}</span>
      </div>
      <div className="ds-gcard-body">
        {me.hero && <Portrait hero={me.hero} size={56} tone={won ? "pick" : "loss"} />}
        <div style={{ minWidth: 0 }}>
          <strong>{m.map ?? "?"}</strong>
          <em>{me.hero ?? "?"} · {fmtDur(m.length)}</em>
          <Kda p={me} />
        </div>
        <span className="ds-res">{won ? "WIN" : "LOSS"}</span>
      </div>
      {aw ? <div className="ds-gcard-aw">{aw.mvp ? "👑 MVP" : `◆ ${aw.label}`}</div> : <div className="ds-gcard-aw none">no award</div>}
    </Link>
  );
}

interface Seg { start: number; end: number; levelDiff: number }

/** Avance de niveau (match.levelAdvTimeline) en escalier : au-dessus de la ligne = `side` devant.
 *  `side` 0 = équipe bleue (diff du parser = bleue − rouge). Marqueurs verticaux optionnels. */
export function LevelAdvantage({ timeline, length, side = 0, marks = [] }: {
  timeline: Seg[]; length: number; side?: 0 | 1; marks?: { t: number; label: string }[];
}) {
  if (!timeline?.length || !length) return null;
  const W = 1000, H = 160, mid = H / 2;
  const maxAbs = Math.max(2, ...timeline.map((s) => Math.abs(s.levelDiff)));
  const k = (mid - 12) / maxAbs;
  const sgn = side === 0 ? 1 : -1;
  const pts: [number, number][] = [[0, mid]];
  for (const s of timeline) {
    const y = mid - sgn * s.levelDiff * k;
    pts.push([(s.start / length) * W, y], [(s.end / length) * W, y]);
  }
  pts.push([W, mid]);
  const d = "M" + pts.map(([x, y]) => `${x.toFixed(1)},${y.toFixed(1)}`).join(" L");
  const minutes = [];
  for (let t = 120; t < length; t += 120) minutes.push(t);
  return (
    <>
      <svg className="ds-lvl" viewBox={`0 0 ${W} ${H}`} preserveAspectRatio="none" role="img" aria-label="Level advantage over time">
        <defs>
          <clipPath id="lvl-up"><rect x="0" y="0" width={W} height={mid} /></clipPath>
          <clipPath id="lvl-dn"><rect x="0" y={mid} width={W} height={mid} /></clipPath>
        </defs>
        <line x1="0" x2={W} y1={mid} y2={mid} className="axis" />
        {minutes.map((t) => <line key={t} x1={(t / length) * W} x2={(t / length) * W} y1={H - 6} y2={H} className="axis" />)}
        <path d={`${d} Z`} className="up" clipPath="url(#lvl-up)" />
        <path d={`${d} Z`} className="dn" clipPath="url(#lvl-dn)" />
        <path d={d} className="line" />
        {marks.map((m) => <line key={m.label + m.t} x1={(m.t / length) * W} x2={(m.t / length) * W} y1={4} y2={H - 4} className="mark"><title>{m.label}</title></line>)}
        {timeline.map((s, i) => (
          <rect key={i} x={(s.start / length) * W} width={Math.max(1, ((s.end - s.start) / length) * W)} y={0} height={H} fill="transparent">
            <title>{`${fmtDur(s.start)}–${fmtDur(s.end)} · ${s.levelDiff === 0 ? "even" : `${Math.abs(s.levelDiff)} level${Math.abs(s.levelDiff) > 1 ? "s" : ""} ${sgn * s.levelDiff > 0 ? "ahead" : "behind"}`}`}</title>
          </rect>
        ))}
      </svg>
      <div className="ds-lvl-legend">
        <span>0:00</span>
        {marks.map((m) => <span key={m.label + m.t}><b>┆</b> {m.label} · {fmtDur(m.t)}</span>)}
        <span>{fmtDur(length)}</span>
      </div>
    </>
  );
}
