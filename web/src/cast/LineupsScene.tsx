import type { Side } from "../api";
import { useDimHeroes } from "../api";
import { useCastLineup } from "../castApi";
import type { CastLineup, CastSeries, CastTeam, HeroCount } from "../castApi";
import { displayName, effectiveLineupMode, seriesPlayerHeroes, seriesScore } from "../castState";
import { HeroPortrait, initials } from "./portrait";

function Top({ t, side, sub }: { t: CastTeam; side: Side; sub: string }) {
  const name = displayName(t, side);
  return (
    <div className="lu-top">
      <div className="lu-lg">{t.logo ? <img src={t.logo} alt="" /> : initials(name)}</div>
      <div className="lu-id">
        <div className="lu-nm">{name}</div>
        {sub !== "" && <div className="lu-sub">{sub}</div>}
      </div>
    </div>
  );
}

const playerName = (name: string, i: number) => (name.trim() === "" ? `Player ${i + 1}` : name);

function ModeA({ s, side, heroes }: { s: CastSeries; side: Side; heroes: HeroCount[][] }) {
  const t = side === "blue" ? s.blue : s.red;
  return (
    <section className={`lu-card ${side === "blue" ? "b" : "r"}`}>
      <Top t={t} side={side} sub={t.record.trim().toUpperCase()} />
      {t.players.map((p, i) => {
        // joueur non lié (ou < 5 parties récentes : liste vide côté serveur) → pas de rangée de héros
        const hs = p.toon ? (heroes[i] ?? []) : [];
        return (
          <div key={i} className="lu-pl">
            <div className="lu-pn">{playerName(p.name, i)}
              {p.tagline.trim() !== "" && <span className="lu-st">{p.tagline}</span>}
            </div>
            {hs.length > 0 && (
              <div className="lu-hp">{hs.map((h) => <HeroPortrait key={h.hero} hero={h.hero} title={`${h.hero} · ${h.games}`} />)}</div>
            )}
            {t.captain === i && <span className="lu-cap">CAPTAIN</span>}
          </div>
        );
      })}
    </section>
  );
}

function ModeB({ s, side }: { s: CastSeries; side: Side }) {
  const t = side === "blue" ? s.blue : s.red;
  const played = seriesPlayerHeroes(s, side);
  const lastWith = Math.max(0, ...played.flat().map((h) => h.game));
  const results = s.games
    .filter((g) => g.winner !== null)
    .map((g) => `G${g.number} ${g.winner === side ? "WON" : "LOST"}`)
    .join(" · ");
  return (
    <section className={`lu-card ${side === "blue" ? "b" : "r"}`}>
      <Top t={t} side={side} sub={results} />
      {t.players.map((p, i) => {
        const hs = played[i] ?? [];
        return (
          <div key={i} className="lu-pl">
            <div className="lu-pn">{playerName(p.name, i)}
              <span className="lu-st">{hs.length > 0 ? hs.map((h) => `G${h.game} ${h.hero}`).join(" · ") : "awaiting game data"}</span>
            </div>
            {hs.length > 0 && (
              <div className="lu-hp">
                {hs.map((h) => {
                  // Fearless : un héros d'une partie précédente est verrouillé → atténué ; celui de la dernière reste en évidence
                  const locked = s.format === "fearless" && h.game < lastWith;
                  return (
                    <HeroPortrait key={h.game} hero={h.hero} className={locked ? "lk" : "cur"}
                      title={`G${h.game} ${h.hero}${locked ? " (locked)" : ""}`} />
                  );
                })}
              </div>
            )}
            {t.captain === i && <span className="lu-cap">CAPTAIN</span>}
          </div>
        );
      })}
    </section>
  );
}

/** Scène « Lineups » : mode A (avant la série) = jusqu'à 3 héros récents par joueur lié, jamais de
 *  pourcentage ; mode B (entre les parties) = héros réellement joués dans la série (`games[].summary`). */
export function LineupsScene({ s, heroes }: { s: CastSeries; heroes?: CastLineup | null }) {
  const mode = effectiveLineupMode(s);
  const [sb, sr] = seriesScore(s);
  const done = s.games.filter((g) => g.winner !== null).length;
  const title =
    mode === "A"
      ? `BEST OF ${s.bo} · ${s.format.toUpperCase()} · LINEUPS`
      : `AFTER GAME ${done} · SERIES ${sb} – ${sr}${s.format === "fearless" ? " · FEARLESS HEROES LOCKED" : ""}`;
  return (
    <div className={`lu lu-${mode.toLowerCase()}`} data-mode={mode}>
      <div className="lu-title">{title}</div>
      <div className="lu-vs">{mode === "A" ? "VS" : `${sb} – ${sr}`}</div>
      {mode === "A" ? (
        <>
          <ModeA s={s} side="blue" heroes={heroes?.blue ?? []} />
          <ModeA s={s} side="red" heroes={heroes?.red ?? []} />
        </>
      ) : (
        <>
          <ModeB s={s} side="blue" />
          <ModeB s={s} side="red" />
        </>
      )}
    </div>
  );
}

/** Overlay / aperçu : charge les héros récents (serveur) et le référentiel de portraits. */
export function LineupsLive({ s }: { s: CastSeries }) {
  useDimHeroes(); // peuple le cache des portraits (heroIcon)
  const { data } = useCastLineup(s);
  return <LineupsScene s={s} heroes={data} />;
}
