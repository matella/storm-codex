import { useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { Link } from "react-router-dom";
import DOMPurify from "dompurify";
import { fetchHeroChanges, fetchPatches, useDimHeroes, classBadge, fmtTime } from "../api";
import { Avatar } from "../components/Avatar";
import { SearchSelect } from "../components/SearchSelect";

const CLASSES = ["BUFF", "NERF", "REWORK", "MIXED"];
const inp = { background: "var(--surface-2)", border: "1px solid var(--hairline-strong)", color: "var(--text)", borderRadius: 6, padding: "4px 8px", fontSize: 12 } as const;

/** Fil global des ajustements héros à travers les patchs (récents d'abord), filtrable par héros /
 *  classification / patch. Vue « par héros » des patch notes. */
export function HeroChanges() {
  const dim = useDimHeroes();
  const heroOptions = dim ? Object.keys(dim).sort() : [];
  const [hero, setHero] = useState("");
  const [klass, setKlass] = useState("");
  const [patch, setPatch] = useState("");
  const [limit, setLimit] = useState(60);
  const reset = () => setLimit(60);

  const { data: patches } = useQuery({ queryKey: ["patches"], queryFn: fetchPatches });
  const { data, isLoading } = useQuery({
    queryKey: ["hero-changes", hero, klass, patch, limit],
    queryFn: () => fetchHeroChanges({ hero: hero || undefined, klass: klass || undefined, patch: patch || undefined, limit }),
  });
  const rows = data ?? [];

  return (
    <>
      <h1 style={{ display: "flex", alignItems: "center", gap: 10, flexWrap: "wrap" }}>
        Hero Changes
        <Link to="/patches" className="pill" style={{ fontSize: 11, marginLeft: "auto" }}>‹ patch list</Link>
      </h1>
      <p className="note">Every hero balance change across patches, newest first. Filter by hero, type or patch.</p>
      <div className="card">
        <div className="card-hd" style={{ flexWrap: "wrap", gap: 6, alignItems: "center" }}>
          <SearchSelect options={heroOptions} value={hero} onChange={(v) => { setHero(v); reset(); }} placeholder="hero…" style={inp} />
          <span style={{ width: 1, alignSelf: "stretch", background: "var(--hairline)", margin: "0 4px" }} />
          <span className={klass === "" ? "pill on" : "pill"} onClick={() => { setKlass(""); reset(); }}>All</span>
          {CLASSES.map((c) => (
            <span key={c} className={klass === c ? "pill on" : "pill"} onClick={() => { setKlass(c); reset(); }}>{c}</span>
          ))}
          <select style={inp} value={patch} onChange={(e) => { setPatch(e.target.value); reset(); }}>
            <option value="">All patches</option>
            {(patches?.items ?? []).map((p) => <option key={p.internalId} value={p.internalId}>{p.patchName}</option>)}
          </select>
        </div>
        {isLoading && <div className="empty">loading…</div>}
        {!isLoading && rows.length === 0 && <div className="empty">no hero changes for this filter</div>}
        <div style={{ display: "flex", flexDirection: "column", gap: 8 }}>
          {rows.map((p, i) => {
            const badge = classBadge(p.classification);
            return (
              <details key={p.patchInternalId + p.anchor + i} style={{ borderBottom: "1px solid var(--border, rgba(255,255,255,.08))", paddingBottom: 8 }}>
                <summary style={{ cursor: "pointer", display: "flex", alignItems: "center", gap: 8, flexWrap: "wrap" }}>
                  <Link to={`/hero/${encodeURIComponent(p.heroName)}`} onClick={(e) => e.stopPropagation()} style={{ display: "flex", alignItems: "center", gap: 6, textDecoration: "none", color: "inherit", fontWeight: 600 }}>
                    <Avatar hero={p.heroName} size={20} /> {p.heroName}
                  </Link>
                  {badge && <span style={{ fontSize: 9, fontWeight: 700, padding: "0 5px", borderRadius: 4, background: badge.bg, color: badge.fg }}>{badge.label}</span>}
                  <Link to={`/patch/${encodeURIComponent(p.patchInternalId)}#${p.anchor}`} onClick={(e) => e.stopPropagation()} style={{ fontSize: 11, textDecoration: "none", color: "var(--accent)" }}>{p.patchName}</Link>
                  <span className="mono muted" style={{ fontSize: 11 }}>{fmtTime(p.liveDate)}</span>
                  {p.shortSummary && <span className="muted" style={{ fontSize: 12 }}>{p.shortSummary}</span>}
                </summary>
                {p.content && <div className="patch-content" style={{ marginTop: 6, lineHeight: 1.5 }} dangerouslySetInnerHTML={{ __html: DOMPurify.sanitize(p.content) }} />}
              </details>
            );
          })}
        </div>
        {rows.length >= limit && (
          <div style={{ textAlign: "center", marginTop: 10 }}>
            <span className="pill" onClick={() => setLimit(limit + 60)}>Load more</span>
          </div>
        )}
      </div>
    </>
  );
}
