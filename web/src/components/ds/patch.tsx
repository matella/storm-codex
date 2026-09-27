// Pièces du design system pour les patch notes : entrée de patch dépliable (héros ↔ patch),
// bloc date. HTML externe toujours rendu par SafeHtml.

import { Link } from "react-router-dom";
import { fmtTime, type HeroPatchSection } from "../../api";
import { ptagClass } from "../../patchHtml";
import { SafeHtml } from "../SafeHtml";
import { Portrait } from "./index";

/** Bloc date « 20 / APR 2026 » (liste de patchs). */
export function DateBlock({ iso }: { iso: string | null }) {
  if (!iso) return <span className="ds-pdate"><b>?</b></span>;
  const d = new Date(iso);
  return (
    <span className="ds-pdate">
      <b>{String(d.getDate()).padStart(2, "0")}</b>
      <span>{d.toLocaleDateString("en-GB", { month: "short" })} {d.getFullYear()}</span>
    </span>
  );
}

/** Une section de patch concernant un héros : date, classification, patch et résumé ; contenu
 *  dépliable, lien vers le patch complet. `hero` affiche le héros en tête (vue chronologique). */
export function PatchEntry({ p, hero }: { p: HeroPatchSection; hero?: string }) {
  const c = (p.classification ?? "").toUpperCase();
  const d = p.liveDate ? `${fmtTime(p.liveDate).slice(0, 5)}/${p.liveDate.slice(0, 4)}` : "?";
  return (
    <details className="ds-patch">
      <summary style={hero ? { gridTemplateColumns: "34px 150px 110px 84px 1fr auto" } : undefined}>
        {hero && <Portrait hero={hero} size={30} />}
        {hero && <Link to={`/hero/${encodeURIComponent(hero)}`} onClick={(e) => e.stopPropagation()} style={{ font: "800 17px var(--display)", textTransform: "uppercase", textDecoration: "none", color: "var(--text)" }}>{hero}</Link>}
        <span className="pdate">{d}</span>
        <span className={`ds-ptag ${ptagClass(c)}`}>{c || "UPDATE"}</span>
        <span className="pname">{p.patchName}{p.shortSummary ? ` — ${p.shortSummary}` : ""}</span>
        <Link className="popen" to={`/patch/${encodeURIComponent(p.patchInternalId)}#${p.anchor}`} onClick={(e) => e.stopPropagation()}>open ›</Link>
      </summary>
      {p.content && <SafeHtml className="pbody" html={p.content} />}
    </details>
  );
}
