import { useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { Link } from "react-router-dom";
import { fetchHeroChanges, fetchHeroChangeHeroes, fetchHeroPatches, fetchPatches, useDimHeroes, fmtTime } from "../api";
import { SearchSelect } from "../components/SearchSelect";
import { Portrait } from "../components/ds";
import { PatchEntry } from "../components/ds/patch";
import { ptagClass } from "../patchHtml";

const CLASSES = ["BUFF", "NERF", "REWORK", "MIXED"];

/** Historique complet d'un héros (monté à la 1ʳᵉ ouverture). */
function HeroHistory({ hero }: { hero: string }) {
  const { data, isLoading } = useQuery({ queryKey: ["hero-patches", hero], queryFn: () => fetchHeroPatches(hero) });
  if (isLoading) return <div className="ds-empty-row">loading…</div>;
  if (!data || data.length === 0) return <div className="ds-empty-row">no changes</div>;
  return <div className="ds-patches">{data.map((p, i) => <PatchEntry key={p.patchInternalId + p.anchor + i} p={p} />)}</div>;
}

/** Hero changes — les patch notes « par héros » : un héros = une ligne, la déplier = tout son
 *  historique. « Timeline » : vue chronologique (tous héros mélangés). Filtres héros / type / patch. */
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
    <div className="ds-page">
      <header className="ds-cover" style={{ marginTop: 18 }}>
        <div className="ds-kicker">Patch notes · by hero</div>
        <h1 className="ds-title" style={{ cursor: "default" }}>Hero changes</h1>
        <div className="ds-subtitle" style={{ cursor: "default" }}>Open a hero for their full patch history — “Timeline” lists every change chronologically.</div>
        <div className="ds-filterbar">
          <span className={view === "heroes" ? "ds-pill on" : "ds-pill"} onClick={() => setView("heroes")}>By hero</span>
          <span className={view === "timeline" ? "ds-pill on" : "ds-pill"} onClick={() => setView("timeline")}>Timeline</span>
          <span className="ds-sep" />
          <SearchSelect className="ds-input" style={{ width: 160 }} options={heroOptions} value={hero} onChange={(v) => { setHero(v); setLimit(60); }} placeholder="hero…" />
          <span className="ds-sep" />
          <span className={klass === "" ? "ds-pill on" : "ds-pill"} onClick={() => { setKlass(""); setLimit(60); }}>All</span>
          {CLASSES.map((c) => <span key={c} className={klass === c ? "ds-pill on" : "ds-pill"} onClick={() => { setKlass(c); setLimit(60); }}>{c}</span>)}
          <select className="ds-input" value={patch} onChange={(e) => { setPatch(e.target.value); setLimit(60); }}>
            <option value="">All patches</option>
            {(patches?.items ?? []).map((p) => <option key={p.internalId} value={p.internalId}>{p.patchName}</option>)}
          </select>
          <Link to="/patches" className="ds-btn" style={{ marginLeft: "auto" }}>‹ Patch list</Link>
        </div>
      </header>

      <section className="ds-sec" style={{ marginTop: 26 }}>
        {view === "heroes" ? (
          <>
            {loadingHeroes && <div className="ds-empty">loading…</div>}
            {!loadingHeroes && filteredHeroes.length === 0 && <div className="ds-empty">No hero for this filter.</div>}
            <div className="ds-hclist">
              {filteredHeroes.map((h) => (
                <details key={h.heroName} className="ds-hc" onToggle={(e) => { if ((e.currentTarget as HTMLDetailsElement).open) open(h.heroName); }}>
                  <summary>
                    <Portrait hero={h.heroName} size={34} />
                    <strong>{h.heroName}</strong>
                    <span className="ds-tag award">{h.count} change{h.count > 1 ? "s" : ""}</span>
                    {h.latestClass && <span className={`ds-ptag ${ptagClass(h.latestClass)}`} style={{ padding: "3px 8px" }}>{h.latestClass.toUpperCase()}</span>}
                    <Link to={`/hero/${encodeURIComponent(h.heroName)}`} onClick={(e) => e.stopPropagation()} className="ds-pill" style={{ padding: "3px 10px" }}>hero page ›</Link>
                    <span className="last">last {fmtTime(h.latestDate)}</span>
                  </summary>
                  {opened.has(h.heroName) && <HeroHistory hero={h.heroName} />}
                </details>
              ))}
            </div>
          </>
        ) : (
          <>
            {loadingFeed && <div className="ds-empty">loading…</div>}
            {!loadingFeed && (feed ?? []).length === 0 && <div className="ds-empty">No hero change for this filter.</div>}
            <div className="ds-patches">{(feed ?? []).map((p, i) => <PatchEntry key={p.patchInternalId + p.anchor + i} p={p} hero={p.heroName} />)}</div>
            {(feed ?? []).length >= limit && <div style={{ textAlign: "center", marginTop: 14 }}><span className="ds-pill" onClick={() => setLimit(limit + 60)}>Load more</span></div>}
          </>
        )}
      </section>
    </div>
  );
}
