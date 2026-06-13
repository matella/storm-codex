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
  const { data: np } = useQuery({
    queryKey: ["now-playing"],
    queryFn: () => fetch("/api/now-playing").then((r) => r.json()),
    refetchInterval: 5000,
  });

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
    <div style={{ height: "100vh", display: "grid", gridTemplateColumns: "minmax(0,1.5fr) minmax(0,1fr)", gap: 18, padding: 24, boxSizing: "border-box" }}>
      <div style={{ background: "rgba(14,16,22,.92)", border: "1px solid var(--hairline-strong)", borderRadius: 16, padding: "22px 26px", boxShadow: "0 8px 30px rgba(0,0,0,.5)", height: "100%", boxSizing: "border-box", display: "flex", flexDirection: "column", overflow: "hidden" }}>
        {/* header */}
        <div style={{ display: "flex", alignItems: "baseline", gap: 10 }}>
          <span style={{ fontSize: 22, fontWeight: 600, letterSpacing: ".04em" }}>TODAY'S SESSION</span>
          <span className="mono" style={{ marginLeft: "auto", fontSize: 24 }}>
            <span style={{ color: "var(--win)" }}>{wins}W</span> – <span style={{ color: "var(--loss)" }}>{losses}L</span>
          </span>
        </div>
        <div className="mono" style={{ fontSize: 13, color: "var(--text-2)", marginTop: 4 }}>
          {streakLabel(streak)} · {games.length} games · {wr}% win rate
        </div>

        {/* win-rate sparkline */}
        {wrSeries.length >= 2 && (
          <svg width="100%" height="46" viewBox={`0 0 100 46`} preserveAspectRatio="none" style={{ marginTop: 14 }}>
            <line x1="0" y1="23" x2="100" y2="23" stroke="var(--hairline)" strokeDasharray="2,2" />
            <polyline
              points={wrSeries.map((v, i) => `${(i / (wrSeries.length - 1)) * 100},${46 - (v / 100) * 46}`).join(" ")}
              fill="none" stroke="var(--win)" strokeWidth="1.5" vectorEffect="non-scaling-stroke"
            />
          </svg>
        )}

        {/* recent games */}
        <div className="kick" style={{ margin: "18px 0 6px", fontSize: 12 }}>Recent games</div>
        {games.slice(0, 8).map((g) => {
          const mb = modeBadge(g.m.mode);
          const td = g.me.takedowns ?? 0, k = g.me.kills ?? 0, d = g.me.deaths ?? 0;
          return (
            <div key={g.m.id} style={{ display: "flex", alignItems: "center", gap: 10, padding: "7px 0", borderBottom: "1px solid var(--hairline)", fontSize: 15 }}>
              <span className={`bdg ${mb.cls}`}>{mb.short}</span>
              <Avatar hero={g.me.hero} size={28} />
              <span style={{ flex: 1, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{g.m.map}</span>
              <span className={`bdg ${g.won ? "b-win" : "b-loss"}`}>{g.won ? "W" : "L"}</span>
              <span className="mono" style={{ color: "#cfd3e0" }}>{k}/{Math.max(0, td - k)}/{d}</span>
            </div>
          );
        })}
        {games.length === 0 && <div style={{ fontSize: 14, color: "var(--text-2)" }}>No games yet today.</div>}

        {/* heroes today + best game */}
        <div style={{ display: "flex", gap: 28, marginTop: 20 }}>
          <div style={{ flex: 1 }}>
            <div className="kick" style={{ margin: "0 0 8px", fontSize: 12 }}>Heroes today</div>
            {heroesTonight.map(([h, r]) => (
              <div key={h} style={{ display: "flex", alignItems: "center", gap: 9, marginBottom: 6, fontSize: 14 }}>
                <Avatar hero={h} size={24} />
                <span style={{ flex: 1 }}>{h}</span>
                <span className="mono" style={{ color: "#cfd3e0" }}>
                  <span style={{ color: "var(--win)" }}>{r.w}</span>-<span style={{ color: "var(--loss)" }}>{r.l}</span>
                </span>
              </div>
            ))}
            {heroesTonight.length === 0 && <span style={{ fontSize: 13, color: "var(--text-2)" }}>—</span>}
          </div>
          {best && (
            <div style={{ flex: 1 }}>
              <div className="kick" style={{ margin: "0 0 8px", fontSize: 12 }}>Best game</div>
              <div style={{ display: "flex", alignItems: "center", gap: 9 }}>
                <Avatar hero={best.me.hero} size={32} />
                <div className="mono" style={{ fontSize: 14, color: "#cfd3e0" }}>
                  {best.me.hero}<br />
                  {best.me.kills ?? 0}/{Math.max(0, (best.me.takedowns ?? 0) - (best.me.kills ?? 0))}/{best.me.deaths ?? 0}
                </div>
              </div>
            </div>
          )}
        </div>

        {/* spacer pushes the Jarvis line to the bottom of the full-height panel */}
        <div style={{ flex: 1, minHeight: 16 }} />

        {/* Jarvis line */}
        {games[0] && (
          <div style={{ fontSize: 16, color: "var(--u-nexus)", fontStyle: "italic", borderTop: "1px solid var(--hairline)", paddingTop: 14 }}>
            « {jarvisPhrase({ won: games[0].won, hero: games[0].me.hero, deaths: games[0].me.deaths ?? 0, takedowns: games[0].me.takedowns ?? 0, streak })} » — Jarvis
          </div>
        )}
      </div>

      {/* Right column = the rest of the scene. Camera + Game are framed slots: drop your OBS
          sources into them (put this /queue source UNDER them, or align on top). Music is built in. */}
      <div style={{ display: "grid", gridTemplateRows: "1fr 1fr auto", gap: 18, height: "100%", minHeight: 0, boxSizing: "border-box" }}>
        <Frame label="CAMERA" />
        <Frame label="GAME · IN QUEUE" />
        <MusicCard np={np} />
      </div>
    </div>
  );
}

/** Bordered placeholder zone — align the matching OBS source (camera / game capture) inside it. */
function Frame({ label }: { label: string }) {
  return (
    <div style={{ position: "relative", border: "1.5px dashed var(--hairline-strong)", borderRadius: 12, minHeight: 0 }}>
      <span className="kick" style={{ position: "absolute", top: 10, left: 14, fontSize: 11, opacity: 0.55 }}>{label}</span>
    </div>
  );
}

/** Built-in "now playing" (Orpheus) so this scene is a single self-contained source. */
function MusicCard({ np }: { np: { authenticated?: boolean; current?: Record<string, unknown> } | undefined }) {
  const cur = (np?.current ?? {}) as Record<string, string | undefined>;
  const title = cur.title ?? cur.name ?? cur.track ?? cur.song;
  const artist = cur.artist ?? cur.artists ?? cur.author;
  const playing = np?.authenticated && !!title;
  return (
    <div style={{ background: "rgba(14,16,22,.92)", border: "1px solid var(--hairline-strong)", borderRadius: 12, padding: "10px 14px", display: "flex", alignItems: "center", gap: 11 }}>
      <span style={{ fontSize: 18 }}>{playing ? "♪" : "♫"}</span>
      <div style={{ flex: 1, minWidth: 0 }}>
        {playing ? (
          <>
            <div style={{ fontSize: 14, fontWeight: 600, whiteSpace: "nowrap", overflow: "hidden", textOverflow: "ellipsis" }}>{title}</div>
            {artist && <div style={{ fontSize: 12, color: "var(--text-2)", whiteSpace: "nowrap", overflow: "hidden", textOverflow: "ellipsis" }}>{artist}</div>}
          </>
        ) : (
          <div style={{ fontSize: 12.5, color: "var(--text-2)" }}>{np?.authenticated ? "Nothing playing" : "Music — off"}</div>
        )}
      </div>
      <span className="kick" style={{ fontSize: 9 }}>ORPHEUS</span>
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
