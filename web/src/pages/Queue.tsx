import { useQuery } from "@tanstack/react-query";
import {
  fetchMatches, modeBadge, useLiveUpdates, useDimHeroes, useSettings,
  matchOperator, jarvisPhrase, type MatchSummary, type MatchPlayer,
} from "../api";
import { Avatar } from "../components/Avatar";

/**
 * Between-games / queue panel (OBS browser source). Shows the operator's session at a glance
 * while queuing: tonight's record, recent games, heroes played, a win-rate sparkline, best game,
 * and a Jarvis line. English UI. Transparent background, live via WS. Add at /queue.
 */
export function Queue() {
  useDimHeroes();
  useSettings();
  const { data, refetch } = useQuery({ queryKey: ["queue-matches"], queryFn: () => fetchMatches({ limit: 200 }) });
  useLiveUpdates(() => refetch());

  const matches = data ?? [];
  // "Session" = every game on the same calendar day as the most recent one (works on historical
  // data too). Each match reduced to the operator's perspective.
  type Game = { m: MatchSummary; me: MatchPlayer; won: boolean };
  const games: Game[] = [];
  if (matches.length) {
    const day = (matches[0].played_at ?? "").slice(0, 10);
    for (const m of matches) {
      if ((m.played_at ?? "").slice(0, 10) !== day) break;
      const me = matchOperator(m.players ?? []);
      if (me) games.push({ m, me, won: me.team != null && m.winner === me.team });
    }
  }

  const wins = games.filter((g) => g.won).length;
  const losses = games.length - wins;
  const wr = games.length ? Math.round((100 * wins) / games.length) : 0;
  const streak = currentStreak(games);

  // heroes played tonight, W-L
  const byHero = new Map<string, { w: number; l: number }>();
  for (const g of games) {
    const h = g.me.hero ?? "?";
    const e = byHero.get(h) ?? { w: 0, l: 0 };
    g.won ? e.w++ : e.l++;
    byHero.set(h, e);
  }
  const heroesTonight = [...byHero.entries()].sort((a, b) => b[1].w + b[1].l - (a[1].w + a[1].l)).slice(0, 4);

  // best game = highest takedowns
  const best = [...games].sort((a, b) => (b.me.takedowns ?? 0) - (a.me.takedowns ?? 0))[0];

  // win-rate sparkline (cumulative, oldest→newest)
  const chrono = [...games].reverse();
  const wrSeries: number[] = [];
  let w = 0;
  chrono.forEach((g, i) => { if (g.won) w++; wrSeries.push((100 * w) / (i + 1)); });

  return (
    <div style={{ padding: 16, maxWidth: 560 }}>
      <div style={{ background: "rgba(14,16,22,.92)", border: "1px solid var(--hairline-strong)", borderRadius: 14, padding: "16px 18px", boxShadow: "0 8px 30px rgba(0,0,0,.5)" }}>
        {/* header */}
        <div style={{ display: "flex", alignItems: "baseline", gap: 10 }}>
          <span style={{ fontSize: 16, fontWeight: 600, letterSpacing: ".04em" }}>TODAY'S SESSION</span>
          <span className="mono" style={{ marginLeft: "auto", fontSize: 18 }}>
            <span style={{ color: "var(--win)" }}>{wins}W</span> – <span style={{ color: "var(--loss)" }}>{losses}L</span>
          </span>
        </div>
        <div className="mono" style={{ fontSize: 11, color: "var(--text-2)", marginTop: 3 }}>
          {streakLabel(streak)} · {games.length} games · {wr}% win rate
        </div>

        {/* win-rate sparkline */}
        {wrSeries.length >= 2 && (
          <svg width="100%" height="34" viewBox={`0 0 100 34`} preserveAspectRatio="none" style={{ marginTop: 10 }}>
            <line x1="0" y1="17" x2="100" y2="17" stroke="var(--hairline)" strokeDasharray="2,2" />
            <polyline
              points={wrSeries.map((v, i) => `${(i / (wrSeries.length - 1)) * 100},${34 - (v / 100) * 34}`).join(" ")}
              fill="none" stroke="var(--win)" strokeWidth="1.5" vectorEffect="non-scaling-stroke"
            />
          </svg>
        )}

        {/* recent games */}
        <div className="kick" style={{ margin: "14px 0 5px" }}>Recent games</div>
        {games.slice(0, 6).map((g) => {
          const mb = modeBadge(g.m.mode);
          const td = g.me.takedowns ?? 0, k = g.me.kills ?? 0, d = g.me.deaths ?? 0;
          return (
            <div key={g.m.id} style={{ display: "flex", alignItems: "center", gap: 8, padding: "5px 0", borderBottom: "1px solid var(--hairline)", fontSize: 12.5 }}>
              <span className={`bdg ${mb.cls}`}>{mb.short}</span>
              <Avatar hero={g.me.hero} size={22} />
              <span style={{ flex: 1, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{g.m.map}</span>
              <span className={`bdg ${g.won ? "b-win" : "b-loss"}`}>{g.won ? "W" : "L"}</span>
              <span className="mono" style={{ color: "#cfd3e0" }}>{k}/{Math.max(0, td - k)}/{d}</span>
            </div>
          );
        })}
        {games.length === 0 && <div style={{ fontSize: 12, color: "var(--text-2)" }}>No games yet today.</div>}

        {/* heroes tonight + best game */}
        <div style={{ display: "flex", gap: 22, marginTop: 14 }}>
          <div style={{ flex: 1 }}>
            <div className="kick" style={{ margin: "0 0 6px" }}>Heroes tonight</div>
            {heroesTonight.map(([h, r]) => (
              <div key={h} style={{ display: "flex", alignItems: "center", gap: 7, marginBottom: 4, fontSize: 12 }}>
                <Avatar hero={h} size={20} />
                <span style={{ flex: 1 }}>{h}</span>
                <span className="mono" style={{ color: "#cfd3e0" }}>
                  <span style={{ color: "var(--win)" }}>{r.w}</span>-<span style={{ color: "var(--loss)" }}>{r.l}</span>
                </span>
              </div>
            ))}
            {heroesTonight.length === 0 && <span style={{ fontSize: 11, color: "var(--text-2)" }}>—</span>}
          </div>
          {best && (
            <div style={{ flex: 1 }}>
              <div className="kick" style={{ margin: "0 0 6px" }}>Best game</div>
              <div style={{ display: "flex", alignItems: "center", gap: 7 }}>
                <Avatar hero={best.me.hero} size={26} />
                <div className="mono" style={{ fontSize: 11.5, color: "#cfd3e0" }}>
                  {best.me.hero}<br />
                  {best.me.kills ?? 0}/{Math.max(0, (best.me.takedowns ?? 0) - (best.me.kills ?? 0))}/{best.me.deaths ?? 0}
                </div>
              </div>
            </div>
          )}
        </div>

        {/* Jarvis line */}
        {games[0] && (
          <div style={{ marginTop: 16, fontSize: 13, color: "var(--u-nexus)", fontStyle: "italic", borderTop: "1px solid var(--hairline)", paddingTop: 12 }}>
            « {jarvisPhrase({ won: games[0].won, hero: games[0].me.hero, deaths: games[0].me.deaths ?? 0, takedowns: games[0].me.takedowns ?? 0, streak })} » — Jarvis
          </div>
        )}
      </div>
    </div>
  );
}

function currentStreak(games: { won: boolean }[]): number {
  if (!games.length) return 0;
  const first = games[0].won;
  let n = 0;
  for (const g of games) { if (g.won === first) n++; else break; }
  return first ? n : -n;
}
function streakLabel(s: number): string {
  if (s === 0) return "no streak";
  return s > 0 ? `W${s} streak ▲` : `L${-s} streak ▼`;
}
