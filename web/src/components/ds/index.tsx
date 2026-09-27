// Design system « Nexus Codex · Dossier » — composants partagés par toute l'app (spec
// docs/specs/2026-09-27-design-dossier-app-design.md). Styles : src/ds.css (classes ds-), tokens :
// theme.css. Même langage que l'export HTML de scouting (reportHtml.ts), qui reste autonome.

import { useState } from "react";
import type { CSSProperties, ReactNode } from "react";
import { heroIcon, heroUniverse } from "../../api";
import { compactEvidence, humanize } from "../../reportHtml";
import { UNIVERSE_HEX } from "../../reportExport";
import type { Count, Evidence, HeroCall, HeroRow, Point } from "../../scouting";

export type Tone = "ban" | "pick" | "them" | "plain";
export type Names = Record<string, string>;

/** Ancre d'une carte (#map-braxis-holdout) — identique à celle de l'export. */
export const mapAnchor = (map: string) => "map-" + map.toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/(^-|-$)/g, "");

const initials = (h: string) => {
  const words = h.replace(/[^A-Za-z0-9 ]/g, "").split(/\s+/).filter(Boolean);
  const s = words.length > 1 ? words.map((w) => w[0]).join("") : (words[0] ?? "");
  return s.slice(0, 2).toUpperCase() || "?";
};

export function Portrait({ hero, size = 40, tone = "plain" }: { hero: string; size?: number; tone?: Tone }) {
  const [broken, setBroken] = useState(false);
  const icon = heroIcon(hero);
  const ring = UNIVERSE_HEX[heroUniverse(hero) ?? "Nexus"] ?? UNIVERSE_HEX.Nexus;
  const style = { "--s": `${size}px`, "--ring": ring } as CSSProperties;
  return (
    <span className={`ds-pt ds-pt-${tone}`} style={style} title={hero}>
      {icon && !broken
        ? <img src={icon} alt={hero} loading="lazy" onError={() => setBroken(true)} />
        : <span className="ds-ini">{initials(hero)}</span>}
      {tone === "ban" && <i className="ds-slash" />}
    </span>
  );
}

export function Pips({ games }: { games: { won: boolean }[] }) {
  return <span className="ds-pips">{games.map((g, i) => <i key={i} className={g.won ? "w" : "l"} />)}</span>;
}

export function Meter({ conf }: { conf: string | null }) {
  const lv = conf === "high" ? 3 : conf === "medium" ? 2 : conf === "low" ? 1 : 0;
  if (!lv) return null;
  return (
    <span className="ds-meter" title={`confidence: ${conf}`}>
      {[1, 2, 3].map((n) => <i key={n} className={lv >= n ? "on" : ""} />)}
      <em>{conf} confidence</em>
    </span>
  );
}

export function EvLine({ ev, unsupported, names, map }: { ev: Evidence[]; unsupported: boolean; names: Names; map?: string }) {
  if (unsupported) return <div className="ds-ev bad">⚠ unverified — no supporting fact</div>;
  const known = ev.filter((e) => e.known).slice(0, 2);
  const unknown = ev.filter((e) => !e.known);
  if (!known.length && !unknown.length) return null;
  return (
    <div className="ds-ev">
      {known.map((e) => {
        const c = compactEvidence(e, { names, map });
        return <span key={e.id} className={e.low ? "low" : undefined} title={e.id}>{c.label} <b>{c.text}</b></span>;
      })}
      {unknown.length > 0 && <span className="bad" title={unknown.map((e) => e.id).join(", ")}>⚠ {unknown.length} unknown fact id{unknown.length > 1 ? "s" : ""}</span>}
    </div>
  );
}

export function CallCard({ h, tone, names, map }: { h: HeroCall; tone: Tone; names: Names; map?: string }) {
  return (
    <div className="ds-call">
      <Portrait hero={h.hero} size={64} tone={tone} />
      <div>
        <div className="ds-call-hd">
          <strong>{h.hero}</strong>
          {tone === "ban" && <span className="ds-tag ban">{h.phase === "mid" ? "MID BAN" : h.phase === "first" ? "1ST PHASE" : "BAN"}</span>}
          {tone === "them" && h.player && <span className="ds-tag them">{names[h.player] ?? h.player}</span>}
        </div>
        <p>{humanize(h.why, names)}</p>
        <EvLine ev={h.evidence} unsupported={h.unsupported} names={names} map={map} />
      </div>
    </div>
  );
}

export function Lane({ title, kind, children, empty }: { title: string; kind: "ban" | "pick" | "them"; children: ReactNode[]; empty: string }) {
  return (
    <div className={`ds-lane ${kind}`}>
      <div className="ds-lane-hd">{title}</div>
      {children.length ? children : <div className="ds-lane-empty">{empty}</div>}
    </div>
  );
}

export function Consider({ points, names, map }: { points: Point[]; names: Names; map?: string }) {
  if (!points.length) return null;
  return (
    <div className="ds-consider-wrap">
      <div className="ds-lane-hd">Consider</div>
      <ul className="ds-consider">
        {points.map((p, i) => (
          <li key={i}><span>{humanize(p.point, names)}</span><EvLine ev={p.evidence} unsupported={p.unsupported} names={names} map={map} /></li>
        ))}
      </ul>
    </div>
  );
}

export function Chips({ list, tone = "plain", max = 6 }: { list: Count[]; tone?: Tone; max?: number }) {
  if (!list.length) return <span className="ds-none">—</span>;
  return (
    <>
      {list.slice(0, max).map((c) => (
        <span key={c.id} className="ds-chip" title={c.id}><Portrait hero={c.key} size={26} tone={tone} /><span>{c.key}</span><b>{c.count.k}×</b></span>
      ))}
    </>
  );
}

/** Rôles (pas de portrait). */
export function TextChips({ list }: { list: Count[] }) {
  if (!list.length) return <span className="ds-none">—</span>;
  return <>{list.map((c) => <span key={c.id} className="ds-chip text"><span>{c.key}</span><b>{c.count.k}×</b></span>)}</>;
}

export function PickChips({ list, names }: { list: HeroRow[]; names: Names }) {
  if (!list.length) return <span className="ds-none">—</span>;
  return (
    <>
      {list.map((h) => {
        const who = [...new Set((h.by ?? []).map((p) => names[p] ?? p))];
        return (
          <span key={h.id} className="ds-chip" title={h.id}>
            <Portrait hero={h.hero} size={26} />
            <span>{h.hero}{who.length > 0 && <em> {who.join(", ")}</em>}</span>
            <b>{h.picks.k}×</b>
          </span>
        );
      })}
    </>
  );
}

export function SecHead({ num, title, sub }: { num: number; title: string; sub?: ReactNode }) {
  return (
    <div className="ds-sec-hd">
      <span className="num">{String(num).padStart(2, "0")}</span>
      <h2>{title}</h2>
      {sub && <small>{sub}</small>}
    </div>
  );
}
