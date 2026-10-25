import { useQuery } from "@tanstack/react-query";
import { useParams, Link } from "react-router-dom";
import { fetchPatch, fmtTime } from "../api";
import { SafeHtml } from "../components/SafeHtml";
import { patchHeadings, ptagClass } from "../patchHtml";

/** Détail d'un patch : couverture, sommaire collant (depuis `tableOfContents`, ancres → scroll,
 *  liens vers les fiches héros), contenu converti (`patchHeadings`) puis assaini (`SafeHtml`). */
export function Patch() {
  const { id } = useParams();
  const { data, isLoading } = useQuery({ queryKey: ["patch", id], queryFn: () => fetchPatch(id!) });
  if (isLoading) return <div className="ds-page"><div className="ds-empty" style={{ marginTop: 24 }}>loading…</div></div>;
  if (!data) return <div className="ds-page"><div className="ds-empty" style={{ marginTop: 24 }}>Patch not found. <Link to="/patches">All patches</Link></div></div>;
  // classification (BUFF/NERF/MIXED) par ancre, depuis les sections de type Hero
  const heroClass: Record<string, string> = {};
  for (const s of data.sections ?? []) if (s.sectionType === "Hero") heroClass[s.anchor] = s.classification;
  // sommaire : on saute l'entrée "Quick Navigation" (on EST la navigation)
  const toc = (data.tableOfContents ?? []).filter((t) => t.sectionType !== "Section");

  return (
    <div className="ds-page">
      <header className="ds-cover" style={{ marginTop: 18 }}>
        <div className="ds-kicker">{data.patchType} · {fmtTime(data.liveDate)}</div>
        <h1 className="ds-title" style={{ cursor: "default", fontSize: "clamp(40px, 6vw, 80px)" }}>{data.patchName}</h1>
        <div className="ds-cover-foot">
          <Link to="/patches" className="ds-btn">‹ All patches</Link>
          <Link to="/hero-changes" className="ds-btn">Changes by hero</Link>
          {data.officialLink && <a href={data.officialLink} target="_blank" rel="noreferrer" className="ds-btn primary">Official notes ↗</a>}
        </div>
      </header>

      <div className={toc.length ? "ds-patchgrid" : ""} style={toc.length ? undefined : { marginTop: 26 }}>
        {toc.length > 0 && (
          <nav className="ds-toc" aria-label="On this page">
            <div className="ds-label">On this page</div>
            {toc.map((t) => {
              const isHero = t.sectionType === "Hero";
              const cls = heroClass[t.anchor];
              return (
                <div key={t.anchor} className={isHero ? "it" : "it sec"} style={{ paddingLeft: Math.max(0, t.headingLevel - 2) * 12 }}>
                  <a href={`#${t.anchor}`}>{t.title}</a>
                  {cls && <span className={`ds-ptag ${ptagClass(cls)}`} style={{ padding: "1px 5px" }}>{cls}</span>}
                  {isHero && <Link to={`/hero/${encodeURIComponent(t.title)}`} title="hero page" className="muted" style={{ marginLeft: "auto" }}>↗</Link>}
                </div>
              );
            })}
          </nav>
        )}
        {data.content ? <SafeHtml className="ds-prose patch-content" html={patchHeadings(data.content)} keepIds /> : <div className="ds-empty">No content for this patch.</div>}
      </div>
    </div>
  );
}
