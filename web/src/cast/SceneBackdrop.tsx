/** Fond opaque Nexus (dégradé radial + étoiles) des scènes plein écran (`series`, `lineups`) ; `ingame` et `hide` restent transparentes. */
export function SceneBackdrop() {
  return (
    <div className="scene-backdrop" data-backdrop>
      <div className="stars" />
    </div>
  );
}
