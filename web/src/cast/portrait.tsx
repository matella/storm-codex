// Petits éléments partagés des scènes de cast (lineups, post-game, aperçu de la régie).
import { heroIcon } from "../api";

/** Initiales d'un nom d'équipe ou de héros : 2 premiers caractères alphanumériques, « ? » sinon. */
export const initials = (name: string) => name.replace(/[^A-Za-z0-9]/g, "").slice(0, 2).toUpperCase() || "?";

/** Portrait de héros (repli : initiales si le référentiel ne le connaît pas). */
export function HeroPortrait({ hero, className = "", title }: { hero: string; className?: string; title?: string }) {
  const icon = heroIcon(hero);
  return icon
    ? <img className={`hp-i ${className}`} src={icon} alt={hero} title={title ?? hero} />
    : <i className={`hp-i ph ${className}`} title={title ?? hero}>{initials(hero)}</i>;
}
