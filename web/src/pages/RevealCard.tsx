import type { Track } from "../api";
import type { RevealPhase } from "../revealState";
import "./now-playing-reveal.css";

/** Pochette, ou placeholder ♫ quand la source n'en fournit pas. */
function Art({ art }: { art?: string }) {
  if (!art) return <div className="npr-art npr-none">♫</div>;
  return <div className="npr-art" style={{ backgroundImage: `url(${art})` }} />;
}

/**
 * Carte du reveal musical : une seule boîte qui s'anime entre la grande annonce et le badge
 * compact, avec les deux couches de contenu en fondu croisé. Les deux couches sont **toujours
 * montées** — c'est ce qui permet le fondu ; c'est la classe d'état sur la racine qui décide.
 */
export function RevealCard({ track, phase }: { track: Track; phase: RevealPhase }) {
  return (
    <div className={`npr is-${phase}`}>
      <div className="npr-card">
        <div className="npr-layer npr-big">
          <div className="npr-krow">
            <span className="npr-kick">Now playing</span>
            <span className="npr-eq"><i /><i /><i /><i /></span>
          </div>
          <Art art={track.art} />
          <div className="npr-title">{track.title}</div>
          <div className="npr-artist">{track.artist}</div>
        </div>
        <div className="npr-layer npr-mini">
          <Art art={track.art} />
          <div className="npr-txt">
            <div className="npr-title">{track.title}</div>
            <div className="npr-artist">{track.artist}</div>
          </div>
        </div>
      </div>
    </div>
  );
}
