// Contenu HotsPatchNotes : de l'HTML pré-rendu (listes/gras en balises), mais pandoc laisse les
// TITRES en markdown `## Section {#anchor}`. On convertit chaque titre en vrai `<hN id="anchor">`
// (l'`id` alimente le sommaire). Pas de parseur markdown : CommonMark casse les blocs HTML sur les
// lignes vides → les `<ul>/<li>` fuiraient en texte. Le résultat est TOUJOURS assaini ensuite
// (SafeHtml / DOMPurify, avec `keepIds`) avant d'être inséré.

export function patchHeadings(raw: string): string {
  return raw.replace(
    /^(#{1,6})\s+(.+?)(?:\s*\{#([^}]+)\})?\s*$/gm,
    (_m: string, h: string, t: string, id?: string) =>
      id ? `<h${h.length} id="${id}">${t.trim()}</h${h.length}>` : `<h${h.length}>${t.trim()}</h${h.length}>`,
  );
}

/** Classe de pastille (ds-ptag) d'une classification de patch (BUFF, NERF…). */
export function ptagClass(c: string | null | undefined): string {
  const k = (c ?? "").toUpperCase();
  return { BUFF: "buff", NERF: "nerf", MIXED: "mixed", BUGFIX: "fix" }[k] ?? "upd";
}
