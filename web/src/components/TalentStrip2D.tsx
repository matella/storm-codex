// US-27 : bande de progression des talents (10 lignes héros × 7 colonnes tiers), rendue sous le
// scrub de la visionneuse 2D. Une cellule est "remplie" (couleur d'équipe) dès que le pick de ce
// tier a un instant `t` ≤ l'instant courant — pas d'animation, juste un état statique par frame.
// Résolution du NOM de talent (tooltip) est BEST-EFFORT : dépend de match_players.talents +
// dim_talents chargés à part par Replay2D ; dégrade gracieusement en "Tier N" sinon.
import { Avatar } from "./Avatar";
import type { HeroTrack, LevelTick, PlayerMeta } from "../replay2d";

// Paliers HotS réels des 7 tiers de talent (affichage uniquement — le crate garde `tier` = ordre
// de pick 1..7, PAS le niveau).
const TIER_LEVELS = [1, 4, 7, 10, 13, 16, 20];

export function TalentStrip2D({
  heroes,
  players,
  levels,
  t,
  talentNameFor,
}: {
  heroes: HeroTrack[];
  players: PlayerMeta[];
  levels: LevelTick[];
  t: number;
  /** Nom de talent résolu depuis le NOM de joueur (battletag, unique par match — évite la
   *  collision de héros miroir) ; null si non résolvable → le composant retombe sur "Tier N". */
  talentNameFor: (playerName: string | null, tier: number) => string | null;
}) {
  if (!heroes.length) return null;
  const byPlayer = new Map(players.map((p) => [p.playerId, p]));
  const rows = heroes
    .map((h) => ({ h, p: byPlayer.get(h.playerId) }))
    .sort((a, b) => (a.p?.team === 1 ? 1 : 0) - (b.p?.team === 1 ? 1 : 0));

  const teamLevel = (team: number): number =>
    levels
      .filter((l) => l.team === team && l.t <= t)
      .reduce((max, l) => Math.max(max, l.level), 0);

  return (
    <div style={{ marginTop: 10 }}>
      <div style={{ display: "flex", justifyContent: "space-between", alignItems: "baseline", marginBottom: 4 }}>
        <p className="cap" style={{ margin: 0 }}>Talents</p>
        <span style={{ fontSize: 11 }}>
          <span className="tm-blue">Blue lvl {teamLevel(0)}</span>
          <span className="muted"> · </span>
          <span className="tm-red">Red lvl {teamLevel(1)}</span>
        </span>
      </div>
      <div
        style={{
          display: "grid",
          gridTemplateColumns: "110px repeat(7, 1fr)",
          gap: 3,
          fontSize: 10,
          alignItems: "center",
        }}
      >
        {rows.map(({ h, p }) => {
          const team = p?.team === 1 ? 1 : 0;
          const teamClass = team === 0 ? "tm-blue" : "tm-red";
          const teamColorVar = team === 0 ? "var(--tm-blue)" : "var(--tm-red)";
          return (
            <div key={h.playerId} style={{ display: "contents" }}>
              <div style={{ display: "flex", alignItems: "center", gap: 4, overflow: "hidden", minWidth: 0 }}>
                <Avatar hero={p?.hero ?? null} size={14} />
                <span
                  className={teamClass}
                  style={{ whiteSpace: "nowrap", overflow: "hidden", textOverflow: "ellipsis" }}
                >
                  {p?.hero ?? "—"}
                </span>
              </div>
              {TIER_LEVELS.map((lvl, i) => {
                const tier = i + 1;
                const taken = h.talents.some((tp) => tp.tier === tier && tp.t <= t);
                const name = taken ? talentNameFor(p?.name ?? null, tier) : null;
                const label = taken ? (name ?? `Tier ${tier}`) : `Tier ${tier} not taken yet (level ${lvl})`;
                return (
                  <div
                    key={tier}
                    role="img"
                    aria-label={`${p?.hero ?? "hero"}, tier ${tier}: ${label}`}
                    title={label}
                    style={{
                      height: 14,
                      borderRadius: 3,
                      background: taken ? teamColorVar : "var(--hairline-strong)",
                      opacity: taken ? 1 : 0.35,
                    }}
                  />
                );
              })}
            </div>
          );
        })}
      </div>
    </div>
  );
}
