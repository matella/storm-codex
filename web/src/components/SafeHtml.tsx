import { useEffect, useRef } from "react";
import DOMPurify from "dompurify";

/** HTML externe (patch notes HotsPatchNotes) : DOMPurify en construit un fragment DOM assaini,
 *  inséré par `replaceChildren` — aucune chaîne HTML n'est jamais injectée telle quelle. Point
 *  unique d'insertion de HTML externe dans l'app. `keepIds` conserve les attributs `id` (ancres
 *  des sections de patch notes). */
export function SafeHtml({ html, className, keepIds = false }: { html: string; className?: string; keepIds?: boolean }) {
  const ref = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (!ref.current) return;
    const fragment = DOMPurify.sanitize(html, { RETURN_DOM_FRAGMENT: true, ...(keepIds ? { ADD_ATTR: ["id"] } : {}) });
    ref.current.replaceChildren(fragment);
  }, [html, keepIds]);
  return <div ref={ref} className={className} />;
}
