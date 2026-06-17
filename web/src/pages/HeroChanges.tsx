import { useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { Link } from "react-router-dom";
import DOMPurify from "dompurify";
import { fetchHeroChanges, fetchHeroChangeHeroes, fetchHeroPatches, fetchPatches, useDimHeroes, classBadge, fmtTime, type HeroPatchSection } from "../api";
import { Avatar } from "../components/Avatar";
import { SearchSelect } from "../components/SearchSelect";

const CLASSES = ["BUFF", "NERF", "REWORK", "MIXED"];
const inp = { background: "var(--surface-2)", border: "1px solid var(--hairline-strong)", color: "var(--text)", borderRadius: 6, padding: "4px 8px", fontSize: 12 } as const;

function Badge({ c }: { c: string | null }) {
  const b = classBadge(c);
  return b ? <span style={{ fontSize: 9, fontWeight: 700, padding: "0 5px", borderRadius: 4, background: b.bg, color: b.fg }}>{b.label}</span> : null;
}

/** Une section de patch concernant un héros : patch + date + badge + résumé, contenu dépliable +
 *  lien vers le patch complet. `hero` (optionnel) préfixe l'avatar+nom (vue timeline, héros mélangés). */
function PatchSection({ p, hero }: { p: HeroPatchSection; hero?: string }) {
  return (
    <details style={{ borderBottom: "1px solid var(--border, rgba(255,255,255,.08))", paddingBottom: 8 }}>
      <summary style={{ cursor: "pointer", display: "flex", alignItems: "center", gap: 8, flexWrap: "wrap" }}>
        {hero && (
          <Link to={`/hero/${encodeURIComponent(hero)}`} onClick={(e) => e.stopPropagation()} style={{ display: "flex", alignItems: "center", gap: 6, textDecoration: "none", color: "inherit", fontWeight: 600 }}>
            <Avatar hero={hero} size={20} /> {hero}
          </Link>
        )}
        <Link to={`/patch/${encodeURIComponent(p.patchInternalId)}#${p.anchor}`} onClick={(e) => e.stopPropagation()} style={{ color: "var(--accent)", textDecoration: "none", fontWeight: hero ? 400 : 600, fontSize: hero ? 11 : undefined }}>{p.patchName}</Link>
        <span className="mono muted" style={{ fontSize: 11 }}>{fmtTime(p.liveDate)}</span>
        <Badge c={p.classification} />
        {p.shortSummary && <span className="muted" style={{ fontSize: 12 }}>{p.shortSummary}</span>}
      </summary>
      {p.content && <div className="patch-content" style={{ marginTop: 6, lineHeight: 1.5 }} dangerouslySetInnerHTML={{ __html: DOMPurify.sanitize(p.content) }} />}
      <div style={{ marginTop: 6 }}>
        <Link to={`/patch/${encodeURIComponent(p.patchInternalId)}#${p.anchor}`} style={{ color: "var(--accent)", fontSize: 11, textDecoration: "none" }}>Open full patch notes ›</Link>
      </div>
    </details>
  );
}

/** Historique complet d'un héros (lazy : monté à la 1ʳᵉ ouverture). */
function HeroHistory({ hero }: { hero: string }) {
  const { data, isLoading } = useQuery({ queryKey: ["hero-patches", hero], queryFn: () => fetchHeroPatches(hero) });
  if (isLoading) return <div className="empty" style={{ padding: 8 }}>loading…</div>;
  if (!data || data.length === 0) return <div className="empty" style={{ padding: 8 }}>no changes</div>;
  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 8, marginTop: 8, paddingLeft: 28 }}>
      {data.map((p, i) => <PatchSection key={p.patchInternalId + p.anchor + i} p={p} />)}
    </div>
  );
}

/** Patch notes « par héros » : un héros = une ligne, déplier = tout son historique de patchs.
 *  Toggle « Timeline » pour la vue chronologique (tous héros mélangés). Filtres : héros / type / patch. */
export function HeroChanges() {
  const dim = useDimHeroes();
  const [view, setView] = useState<"heroes" | "timeline">("heroes");
  const [hero, setHero] = useState("");
  const [klass, setKlass] = useState("");
  const [patch, setPatch] = useState("");
  const [limit, setLimit] = useState(60);
  const [opened, setOpened] = useState<Set<string>>(new Set());
  const open = (h: string) => setOpened((p) => new Set(p).add(h));

  const { data: patches } = useQuery({ queryKey: ["patches"], queryFn: fetchPatches });
  const heroOptions = dim ? Object.keys(dim).sort() : [];

  const { data: heroList, isLoading: loadingHeroes } = useQuery({
    queryKey: ["hc-heroes", klass, patch],
    queryFn: () => fetchHeroChangeHeroes({ klass: klass || undefined, patch: patch || undefined }),
    enabled: view === "heroes",
  });
  const filteredHeroes = (heroList ?? []).filter((h) => !hero || h.heroName.toLowerCase().includes(hero.toLowerCase()));

  const { data: feed, isLoading: loadingFeed } = useQuery({
    queryKey: ["hero-changes", hero, klass, patch, limit],
    queryFn: () => fetchHeroChanges({ hero: hero || undefined, klass: klass || undefined, patch: patch || undefined, limit }),
    enabled: view === "timeline",
  });

  return (
    <>
      <h1 style={{ display: "flex", alignItems: "center", gap: 10, flexWrap: "wrap" }}>
        Hero Changes
        <Link to="/patches" className="pill" style={{ fontSize: 11, marginLeft: "auto" }}>‹ patch list</Link>
      </h1>
      <p className="note">Patch changes per hero — open a hero for their full history. "Timeline" lists every change chronologically.</p>
      <div className="card">
        <div className="card-hd" style={{ flexWrap: "wrap", gap: 6, alignItems: "center" }}>
          <span className={view === "heroes" ? "pill on" : "pill"} onClick={() => setView("heroes")}>By hero</span>
          <span className={view === "timeline" ? "pill on" : "pill"} onClick={() => setView("timeline")}>Timeline</span>
          <span style={{ width: 1, alignSelf: "stretch", background: "var(--hairline)", margin: "0 4px" }} />
          <SearchSelect options={heroOptions} value={hero} onChange={(v) => { setHero(v); setLimit(60); }} placeholder="hero…" style={inp} />
          <span style={{ width: 1, alignSelf: "stretch", background: "var(--hairline)", margin: "0 4px" }} />
          <span className={klass === "" ? "pill on" : "pill"} onClick={() => { setKlass(""); setLimit(60); }}>All</span>
          {CLASSES.map((c) => (
            <span key={c} className={klass === c ? "pill on" : "pill"} onClick={() => { setKlass(c); setLimit(60); }}>{c}</span>
          ))}
          <select style={inp} value={patch} onChange={(e) => { setPatch(e.target.value); setLimit(60); }}>
            <option value="">All patches</option>
            {(patches?.items ?? []).map((p) => <option key={p.internalId} value={p.internalId}>{p.patchName}</option>)}
          </select>
        </div>

        {view === "heroes" ? (
          <>
            {loadingHeroes && <div className="empty">loading…</div>}
            {!loadingHeroes && filteredHeroes.length === 0 && <div className="empty">no heroes for this filter</div>}
            <div style={{ display: "flex", flexDirection: "column", gap: 4 }}>
              {filteredHeroes.map((h) => (
                <details key={h.heroName} onToggle={(e) => { if ((e.currentTarget as HTMLDetailsElement).open) open(h.heroName); }} style={{ borderBottom: "1px solid var(--border, rgba(255,255,255,.08))", paddingBottom: 6 }}>
                  <summary style={{ cursor: "pointer", display: "flex", alignItems: "center", gap: 8, flexWrap: "wrap" }}>
                    <Link to={`/hero/${encodeURIComponent(h.heroName)}`} onClick={(e) => e.stopPropagation()} style={{ display: "flex", alignItems: "center", gap: 6, textDecoration: "none", color: "inherit", fontWeight: 600 }}>
                      <Avatar hero={h.heroName} size={22} /> {h.heroName}
                    </Link>
                    <span className="bdg b-qm" style={{ fontSize: 10 }}>{h.count} change{h.count > 1 ? "s" : ""}</span>
                    <Badge c={h.latestClass} />
                    <span className="mono muted" style={{ fontSize: 11, marginLeft: "auto" }}>last {fmtTime(h.latestDate)}</span>
                  </summary>
                  {opened.has(h.heroName) && <HeroHistory hero={h.heroName} />}
                </details>
              ))}
            </div>
          </>
        ) : (
          <>
            {loadingFeed && <div className="empty">loading…</div>}
            {!loadingFeed && (feed ?? []).length === 0 && <div className="empty">no hero changes for this filter</div>}
            <div style={{ display: "flex", flexDirection: "column", gap: 8 }}>
              {(feed ?? []).map((p, i) => <PatchSection key={p.patchInternalId + p.anchor + i} p={p} hero={p.heroName} />)}
            </div>
            {(feed ?? []).length >= limit && (
              <div style={{ textAlign: "center", marginTop: 10 }}>
                <span className="pill" onClick={() => setLimit(limit + 60)}>Load more</span>
              </div>
            )}
          </>
        )}
      </div>
    </>
  );
}
