import { useQuery } from "@tanstack/react-query";
import { useParams, Link } from "react-router-dom";
import DOMPurify from "dompurify";
import { fetchPatch, fmtTime, classBadge } from "../api";

/** Le contenu HotsPatchNotes est de l'HTML pré-rendu (listes/gras en balises), mais pandoc laisse
 *  les TITRES en markdown `## Section {#anchor}`. On convertit chaque titre en vrai `<hN id="anchor">`
 *  (l'`id` alimente le sommaire), puis on sanitize et on injecte. Surtout PAS via un parseur markdown :
 *  CommonMark casse les blocs HTML sur les lignes vides → les balises `<ul>/<li>` fuient en texte. */
function renderPatch(raw: string): string {
  const html = raw.replace(
    /^(#{1,6})\s+(.+?)(?:\s*\{#([^}]+)\})?\s*$/gm,
    (_m: string, h: string, t: string, id?: string) =>
      id ? `<h${h.length} id="${id}">${t.trim()}</h${h.length}>` : `<h${h.length}>${t.trim()}</h${h.length}>`,
  );
  return DOMPurify.sanitize(html, { ADD_ATTR: ["id"] });
}

/** Détail d'un patch : entête + sommaire collant (depuis `tableOfContents`, ancres → scroll, liens
 *  vers les fiches héros) + contenu HTML sanitizé. */
export function Patch() {
  const { id } = useParams();
  const { data, isLoading } = useQuery({ queryKey: ["patch", id], queryFn: () => fetchPatch(id!) });

  if (isLoading) return <div className="empty">loading…</div>;
  if (!data) return <div className="empty">patch not found</div>;

  // classification (BUFF/NERF/MIXED) par ancre, depuis les sections de type Hero
  const heroClass: Record<string, string> = {};
  for (const s of data.sections ?? []) if (s.sectionType === "Hero") heroClass[s.anchor] = s.classification;
  // sommaire : on saute l'entrée "Quick Navigation" (on EST la navigation)
  const toc = (data.tableOfContents ?? []).filter((t) => t.sectionType !== "Section");

  return (
    <>
      <style>{`.patch-content h1,.patch-content h2,.patch-content h3,.patch-content h4,.patch-content h5{scroll-margin-top:80px}`}</style>
      <h1 style={{ display: "flex", alignItems: "center", gap: 10, flexWrap: "wrap" }}>
        {data.patchName}
        <span className="bdg b-qm">{data.patchType}</span>
        <span className="mono muted" style={{ fontSize: 11, fontWeight: 400 }}>{fmtTime(data.liveDate)}</span>
        <Link to="/patches" className="pill" style={{ fontSize: 11, marginLeft: "auto" }}>‹ all patches</Link>
      </h1>
      {data.officialLink && (
        <p className="note"><a href={data.officialLink} target="_blank" rel="noreferrer" style={{ color: "var(--accent)" }}>Official notes ↗</a></p>
      )}
      <div style={{ display: "flex", gap: 24, alignItems: "flex-start" }}>
        {toc.length > 0 && (
          <nav className="card" style={{ position: "sticky", top: 16, flex: "0 0 218px", padding: "12px 14px", maxHeight: "calc(100vh - 96px)", overflow: "auto", fontSize: 12 }}>
            <div className="muted" style={{ fontSize: 10, letterSpacing: ".08em", marginBottom: 8 }}>ON THIS PAGE</div>
            {toc.map((t) => {
              const isHero = t.sectionType === "Hero";
              const badge = classBadge(heroClass[t.anchor]);
              return (
                <div key={t.anchor} style={{ paddingLeft: Math.max(0, t.headingLevel - 2) * 12, margin: "3px 0", display: "flex", alignItems: "center", gap: 6, lineHeight: 1.3 }}>
                  <a href={`#${t.anchor}`} style={{ color: isHero ? "inherit" : "var(--accent)", textDecoration: "none" }}>{t.title}</a>
                  {badge && <span style={{ fontSize: 9, fontWeight: 700, padding: "0 5px", borderRadius: 4, background: badge.bg, color: badge.fg }}>{badge.label}</span>}
                  {isHero && <Link to={`/hero/${encodeURIComponent(t.title)}`} title="fiche du héros" className="muted" style={{ textDecoration: "none", marginLeft: "auto" }}>↗</Link>}
                </div>
              );
            })}
          </nav>
        )}
        <div className="card patch-content" style={{ flex: 1, minWidth: 0, padding: "18px 22px", lineHeight: 1.55 }}>
          {data.content ? (
            <div dangerouslySetInnerHTML={{ __html: renderPatch(data.content) }} />
          ) : (
            <div className="empty">no content for this patch</div>
          )}
        </div>
      </div>
    </>
  );
}
