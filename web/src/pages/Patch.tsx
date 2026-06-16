import { useQuery } from "@tanstack/react-query";
import { useParams, Link } from "react-router-dom";
import ReactMarkdown from "react-markdown";
import rehypeRaw from "rehype-raw";
import rehypeSanitize from "rehype-sanitize";
import { fetchPatch, fmtTime } from "../api";

/** Détail d'un patch : entête + contenu (markdown + HTML embarqué, rendu sanitizé). */
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
          <ReactMarkdown rehypePlugins={[rehypeRaw, rehypeSanitize]}>
            {data.content.replace(/\s*\{#[^}]+\}/g, "")}
          </ReactMarkdown>
        ) : (
          <div className="empty">no content for this patch</div>
        )}
      </div>
    </>
  );
}
