import { LiveGame } from "../components/LiveGame";

/** Live — la partie en cours, lue au fil de l'eau depuis les fichiers que HotS écrit pendant le
 *  jeu (≈10 s de retard pour les stats, davantage pour les aptitudes). */
export function Live() {
  return (
    <div className="ds-page">
      <header className="ds-cover" style={{ marginTop: 18 }}>
        <div className="ds-kicker">Live game</div>
        <h1 className="ds-title" style={{ cursor: "default" }}>Live</h1>
        <div className="ds-subtitle" style={{ cursor: "default" }}>
          The game in progress, read from the replay files HotS writes while you play — stats about 10 s behind, ability
          presses up to a minute or more.
        </div>
      </header>
      <section className="ds-sec" style={{ marginTop: 24 }}>
        <LiveGame />
      </section>
    </div>
  );
}
