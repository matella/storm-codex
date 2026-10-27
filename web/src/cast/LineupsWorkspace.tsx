// Espace de travail de la scène Lineups : mode Auto / A / B (forçage serveur) + aperçu de ce que le
// mode effectif montre pour chaque joueur (héros récents en A, héros joués dans la série en B).
import { useDimHeroes } from "../api";
import type { Side } from "../api";
import { castLineupMode, useCastLineup } from "../castApi";
import type { CastSeries, HeroCount, LineupMode } from "../castApi";
import { displayName, effectiveLineupMode, seriesPlayerHeroes } from "../castState";
import { HeroPortrait } from "./portrait";

type Run = (fn: () => Promise<unknown>) => Promise<void>;

const MODES: { id: LineupMode; label: string; hint: string }[] = [
  { id: "auto", label: "Auto", hint: "A until a game has a winner, then B" },
  { id: "a", label: "A · recent heroes", hint: "before the series: top 3 recent heroes per linked player" },
  { id: "b", label: "B · series heroes", hint: "between games: heroes played so far, Fearless-locked dimmed" },
];

function SideRows({ s, side, heroes }: { s: CastSeries; side: Side; heroes: HeroCount[][] }) {
  const t = side === "blue" ? s.blue : s.red;
  const mode = effectiveLineupMode(s);
  const played = seriesPlayerHeroes(s, side);
  return (
    <div className="cr-lu-side">
      <div className={`cr-lu-h tone-${side}`}>{displayName(t, side)}</div>
      {t.players.map((p, i) => {
        const hs = heroes[i] ?? [];
        const pl = played[i] ?? [];
        return (
          <div key={i} className="cr-lu-row">
            <span className="cr-lu-n">{p.name.trim() === "" ? `Player ${i + 1}` : p.name}</span>
            <span className={`cr-pk-st${p.toon ? " on" : ""}`}>{p.toon ? "linked" : "unlinked"}</span>
            <span className="cr-lu-h3">
              {mode === "A" && (p.toon && hs.length > 0
                ? hs.map((h) => <HeroPortrait key={h.hero} hero={h.hero} title={`${h.hero} · ${h.games} games`} />)
                : <em>{p.toon ? "hidden (under 5 recent games)" : "hidden (unlinked)"}</em>)}
              {mode === "B" && (pl.length > 0
                ? pl.map((h) => <HeroPortrait key={h.game} hero={h.hero} title={`G${h.game} ${h.hero}`} />)
                : <em>awaiting game data</em>)}
            </span>
          </div>
        );
      })}
    </div>
  );
}

export function LineupsWorkspace({ series, run }: { series: CastSeries; run: Run }) {
  useDimHeroes();
  const { data } = useCastLineup(series);
  const eff = effectiveLineupMode(series);
  return (
    <section className="cr-box">
      <div className="cr-k">Lineups · mode</div>
      <div className="cr-steppers" role="group" aria-label="Lineup mode">
        {MODES.map((m) => (
          <button key={m.id} type="button" className={`ds-btn small${series.lineup_mode === m.id ? " on" : ""}`}
            aria-pressed={series.lineup_mode === m.id} title={m.hint} onClick={() => run(() => castLineupMode(m.id))}>{m.label}</button>
        ))}
      </div>
      <div className="cr-note">
        Showing mode <b>{eff}</b>{series.lineup_mode === "auto" ? " (auto)" : " (forced)"} —{" "}
        {MODES.find((m) => m.id === (eff === "A" ? "a" : "b"))?.hint}. Link players in the rail (Find) to get their recent heroes.
      </div>
      <div className="cr-lu-prev" aria-label="Lineup preview">
        <SideRows s={series} side="blue" heroes={data?.blue ?? []} />
        <SideRows s={series} side="red" heroes={data?.red ?? []} />
      </div>
    </section>
  );
}
