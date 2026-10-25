// Helpers purs de la recherche et de l'identité des joueurs (testés dans players.test.ts).
import type { NameSpan, PlayerIdentity } from "./api";

export interface Segment { text: string; hit: boolean }

/** Découpe `name` autour de la première occurrence littérale (sans regex) de `q`, sans tenir compte
 *  de la casse ; garde la casse d'origine. Aucun segment vide. */
export function highlightSegments(name: string, q: string): Segment[] {
  const needle = q.trim().toLowerCase();
  const hay = name.toLowerCase();
  // toLowerCase peut changer la longueur (rares caractères spéciaux) : pas de surlignage plutôt que décalé.
  const at = needle && hay.length === name.length ? hay.indexOf(needle) : -1;
  if (at < 0) return [{ text: name, hit: false }];
  const out: Segment[] = [];
  if (at > 0) out.push({ text: name.slice(0, at), hit: false });
  out.push({ text: name.slice(at, at + needle.length), hit: true });
  if (at + needle.length < name.length) out.push({ text: name.slice(at + needle.length), hit: false });
  return out;
}

/** Historique trié récent d'abord : les `n` premiers + le nombre de pseudos repliés. */
export function recentNames(h: NameSpan[], n = 3): { shown: NameSpan[]; hidden: number } {
  return { shown: h.slice(0, n), hidden: Math.max(0, h.length - n) };
}

const MONTHS = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];

/** « Jan 2025 », « Jan–Aug 2025 », « Nov 2024–Aug 2025 » ou « date unknown » (mois en UTC). */
export function seenRange(s: NameSpan): string {
  if (!s.first_seen || !s.last_seen) return "date unknown";
  const a = new Date(s.first_seen);
  const b = new Date(s.last_seen);
  const [ya, ma, yb, mb] = [a.getUTCFullYear(), a.getUTCMonth(), b.getUTCFullYear(), b.getUTCMonth()];
  if (ya === yb && ma === mb) return `${MONTHS[ma]} ${ya}`;
  if (ya === yb) return `${MONTHS[ma]}–${MONTHS[mb]} ${ya}`;
  return `${MONTHS[ma]} ${ya}–${MONTHS[mb]} ${yb}`;
}

/** Compte à signaler sur le profil principal : un lien confirmé d'abord, sinon la première
 *  suggestion de confiance haute. Jamais une suggestion de confiance basse. */
export function likelyAlias(i: PlayerIdentity): { toon: string; name: string | null } | null {
  const c = i.confirmed[0];
  if (c) return { toon: c.toon, name: c.name };
  const h = i.suggestions.find((s) => s.level === "high");
  return h ? { toon: h.toon, name: h.name } : null;
}
