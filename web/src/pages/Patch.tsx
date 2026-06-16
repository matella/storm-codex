import { useQuery } from "@tanstack/react-query";
import { useParams, Link } from "react-router-dom";
import DOMPurify from "dompurify";
import { fetchPatch, fmtTime } from "../api";

/** Le contenu HotsPatchNotes est de l'HTML pré-rendu (listes/gras en balises), mais pandoc laisse
 *  les TITRES en markdown (`## Section {#anchor}`). On retire les ancres, on convertit ces titres
 *  résiduels en vrais `<hN>`, puis on sanitize et on injecte. Surtout PAS via un parseur markdown :
 *  CommonMark casse les blocs HTML sur les lignes vides → les balises `<ul>/<li>` fuient en texte. */
function renderPatch(raw: string): string {
  const html = raw
    .replace(/\s*\{#[^}]+\}/g, "") // ancres pandoc `{#slug}`
    .replace(/^(#{1,6})\s+(.+)$/gm, (_, h: string, t: string) => `<h${h.length}>${t.trim()}</h${h.length}>`);
  return DOMPurify.sanitize(html);
}

/** Détail d'un patch : entête + contenu HTML sanitizé. */
export function Patch() {
  const { id } = useParams();
  const { data, isLoading } = useQuery({ queryKey: ["patch", id], queryFn: () => fetchPatch(id!) });

  if (isLoading) return <div className="empty">loading…</div>;
  if (!data) return <div className="empty">patch not found</div>;

  return (
    <>
      <h1 style={{ display: "flex", alignItems: "center", gap: 10, flexWrap: "wrap" }}>
        {data.patchName}
        <span className="bdg b-qm">{data.patchType}</span>
        <span className="mono muted" style={{ fontSize: 11, fontWeight: 400 }}>{fmtTime(data.liveDate)}</span>
        <Link to="/patches" className="pill" style={{ fontSize: 11, marginLeft: "auto" }}>‹ all patches</Link>
      </h1>
      {data.officialLink && (
        <p className="note"><a href={data.officialLink} target="_blank" rel="noreferrer" style={{ color: "var(--accent)" }}>Official notes ↗</a></p>
      )}
      <div className="card patch-content" style={{ padding: "18px 22px", lineHeight: 1.55 }}>
        {data.content ? (
          <div dangerouslySetInnerHTML={{ __html: renderPatch(data.content) }} />
        ) : (
          <div className="empty">no content for this patch</div>
        )}
      </div>
    </>
  );
}
