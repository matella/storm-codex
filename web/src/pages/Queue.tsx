import { useEffect } from "react";
import { useQuery } from "@tanstack/react-query";
import {
  fetchMatches, modeBadge, useLiveUpdates, useDimHeroes, useSettings,
  matchOperator, operatorNames, jarvisPhrase, parseTrack, fmtDur, awardLabel,
  type MatchSummary, type MatchPlayer, type NowPlayingResp,
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
  // source OBS : fond transparent (laisse passer les sources cam/game placées sous /queue).
  useEffect(() => {
    document.body.style.background = "transparent";
    document.body.style.margin = "0";
    return () => { document.body.style.background = ""; document.body.style.margin = ""; };
  }, []);

  const matches = data ?? [];
  // Parties de l'opérateur (tous comptes configurés via operator_names), les plus récentes
  // d'abord. matchOperator est strict : une partie sans aucun compte opérateur est ignorée.
  type Game = { m: MatchSummary; me: MatchPlayer; won: boolean; day: string };
  const opGames: Game[] = matches.flatMap((m) => {
    const me = matchOperator(m.players ?? []);
    return me && m.played_at
      ? [{ m, me, won: me.team != null && m.winner === me.team, day: localDay(m.played_at) }]
      : [];
  });
  // "Session" = les parties d'AUJOURD'HUI (date locale). Si aucune partie aujourd'hui, on retombe
  // sur la dernière session jouée (le jour le plus récent) plutôt qu'un panneau vide.
  const todayKey = localDay(new Date());
  const todays = opGames.filter((g) => g.day === todayKey);
  const isToday = todays.length > 0;
  const sessionDay = isToday ? todayKey : opGames[0]?.day;
  const games: Game[] = sessionDay ? opGames.filter((g) => g.day === sessionDay) : [];
  const sessionLabel = isToday || games.length === 0 ? "TODAY'S SESSION" : "LAST SESSION";
  // Quand plusieurs comptes opérateur sont configurés, on indique sur quel compte chaque partie a
  // été jouée (sinon l'info est redondante → on l'omet).
  const multiAccount = operatorNames().length > 1;

  const wins = games.filter((g) => g.won).length;
  const losses = games.length - wins;
  const wr = games.length ? Math.round((100 * wins) / games.length) : 0;
  const streak = currentStreak(games);

  // ventilation W-L par mode — affichée seulement si la session mélange ≥2 modes (sinon =global).
  const byMode = new Map<number, { w: number; l: number }>();
  for (const g of games) {
    const k = g.m.mode ?? -999;
    const e = byMode.get(k) ?? { w: 0, l: 0 };
    g.won ? e.w++ : e.l++;
    byMode.set(k, e);
  }
  const modeBreakdown = [...byMode.entries()].sort((a, b) => b[1].w + b[1].l - (a[1].w + a[1].l));

  // heroes played tonight, W-L
  const byHero = new Map<string, { w: number; l: number }>();
  for (const g of games) {
    const h = g.me.hero ?? "?";
    const e = byHero.get(h) ?? { w: 0, l: 0 };
    g.won ? e.w++ : e.l++;
    byHero.set(h, e);
  }
  const heroesTonight = [...byHero.entries()].sort((a, b) => b[1].w + b[1].l - (a[1].w + a[1].l)).slice(0, 10);

  // séquence chronologique (oldest→newest) pour la rangée de pastilles W/L
  const chrono = [...games].reverse();

  // agrégats de session (perspective opérateur) pour le bloc "Session"
  const n = games.length || 1;
  const sum = (f: (g: Game) => number) => games.reduce((s, g) => s + f(g), 0);
  const tK = sum((g) => g.me.kills ?? 0);
  const tD = sum((g) => g.me.deaths ?? 0);
  const tTd = sum((g) => g.me.takedowns ?? 0);
  const tA = Math.max(0, tTd - tK);
  const kda = tD ? (tK + tA) / tD : tK + tA;
  const lengths = games.map((g) => g.m.length).filter((x): x is number => x != null);
  const avgLen = lengths.length ? lengths.reduce((a, b) => a + b, 0) / lengths.length : null;

  return (
    <div style={{ width: 1920, height: 1080, display: "grid", gridTemplateColumns: "minmax(0,1.5fr) minmax(0,1fr)", gap: 18, padding: 24, boxSizing: "border-box" }}>
      <div style={{ background: "rgba(14,16,22,.92)", border: "1px solid var(--hairline-strong)", borderRadius: 16, padding: "22px 26px", boxShadow: "0 8px 30px rgba(0,0,0,.5)", height: "100%", boxSizing: "border-box", display: "flex", flexDirection: "column", overflow: "hidden" }}>
        {/* header */}
        <div style={{ display: "flex", alignItems: "baseline", gap: 10 }}>
          <span style={{ fontSize: 22, fontWeight: 600, letterSpacing: ".04em" }}>{sessionLabel}</span>
          {sessionLabel === "LAST SESSION" && sessionDay && (
            <span className="mono" style={{ fontSize: 13, color: "var(--text-2)" }}>{fmtDay(sessionDay)}</span>
          )}
          <span className="mono" style={{ marginLeft: "auto", fontSize: 24 }}>
            <span style={{ color: "var(--win)" }}>{wins}W</span> – <span style={{ color: "var(--loss)" }}>{losses}L</span>
          </span>
        </div>
        <div className="mono" style={{ fontSize: 13, color: "var(--text-2)", marginTop: 4 }}>
          {streakLabel(streak)} · {games.length} games · {wr}% win rate
        </div>
        {modeBreakdown.length >= 2 && (
          <div style={{ display: "flex", flexWrap: "wrap", gap: 12, marginTop: 8 }}>
            {modeBreakdown.map(([mode, r]) => {
              const mb = modeBadge(mode);
              return (
                <span key={mode} style={{ display: "inline-flex", alignItems: "center", gap: 6, fontSize: 14 }}>
                  <span className={`bdg ${mb.cls}`}>{mb.short}</span>
                  <span className="mono">
                    <span style={{ color: "var(--win)" }}>{r.w}</span>–<span style={{ color: "var(--loss)" }}>{r.l}</span>
                  </span>
                </span>
              );
            })}
          </div>
        )}

        {/* rangée de pastilles W/L (chronologique → on voit la série/le momentum d'un coup d'œil) */}
        {chrono.length > 0 && (
          <div style={{ display: "flex", flexWrap: "wrap", gap: 5, marginTop: 16 }}>
            {chrono.map((g, i) => {
              const last = i === chrono.length - 1;
              return (
                <span
                  key={g.m.id}
                  title={`${g.won ? "W" : "L"} · ${g.m.map}`}
                  style={{
                    width: 17,
                    height: 17,
                    borderRadius: 5,
                    background: g.won ? "var(--win)" : "var(--loss)",
                    opacity: last ? 1 : 0.82,
                    boxShadow: last ? "0 0 0 2px rgba(255,255,255,.35)" : "none",
                  }}
                />
              );
            })}
          </div>
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
              <span style={{ flex: 1, minWidth: 0, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>
                {g.m.map}
                {multiAccount && g.me.name && (
                  <span className="muted" style={{ fontSize: 11, marginLeft: 8 }}>· {g.me.name}</span>
                )}
              </span>
              {(() => {
                const aw = awardLabel(g.me.award);
                if (!aw) return null;
                return aw.mvp ? (
                  <span title="MVP" style={{ fontSize: 10, fontWeight: 700, padding: "1px 6px", borderRadius: 999, color: "#1a1500", background: "linear-gradient(90deg,#f5c542,#e0a818)" }}>👑 MVP</span>
                ) : (
                  <span title={aw.label} style={{ fontSize: 13 }}>🏅</span>
                );
              })()}
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
          {games.length > 0 && (
            <div style={{ flex: 1 }}>
              <div className="kick" style={{ margin: "0 0 8px", fontSize: 12 }}>Session</div>
              <div style={{ display: "flex", alignItems: "baseline", gap: 8 }}>
                <span className="mono" style={{ fontSize: 28, fontWeight: 700, color: kda >= 3 ? "var(--win)" : "#cfd3e0" }}>{kda.toFixed(1)}</span>
                <span className="muted" style={{ fontSize: 12 }}>KDA ratio</span>
              </div>
              <div className="mono" style={{ fontSize: 13, color: "var(--text-2)", marginTop: 8, lineHeight: 1.8 }}>
                <div>avg <span style={{ color: "#cfd3e0" }}>{(tK / n).toFixed(1)}/{(tA / n).toFixed(1)}/{(tD / n).toFixed(1)}</span> per game</div>
                <div>{tTd} takedowns{avgLen != null && <> · avg {fmtDur(avgLen)}</>}</div>
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
function MusicCard({ np }: { np: NowPlayingResp | undefined }) {
  const t = parseTrack(np);
  return (
    <div style={{ background: "rgba(14,16,22,.92)", border: "1px solid var(--hairline-strong)", borderRadius: 12, padding: "10px 14px", display: "flex", alignItems: "center", gap: 11 }}>
      {t.playing && t.art ? (
        <img src={t.art} alt="" style={{ width: 36, height: 36, borderRadius: 6, objectFit: "cover" }} />
      ) : (
        <span style={{ fontSize: 18 }}>{t.playing ? "♪" : "♫"}</span>
      )}
      <div style={{ flex: 1, minWidth: 0 }}>
        {t.playing ? (
          <>
            <div style={{ fontSize: 14, fontWeight: 600, whiteSpace: "nowrap", overflow: "hidden", textOverflow: "ellipsis" }}>{t.title}</div>
            {t.artist && <div style={{ fontSize: 12, color: "var(--text-2)", whiteSpace: "nowrap", overflow: "hidden", textOverflow: "ellipsis" }}>{t.artist}</div>}
          </>
        ) : (
          <div style={{ fontSize: 12.5, color: "var(--text-2)" }}>{np?.authenticated ? "Nothing playing" : "Music — off"}</div>
        )}
      </div>
      <span className="kick" style={{ fontSize: 9 }}>ORPHEUS</span>
    </div>
  );
}

/** Clé jour en heure LOCALE (YYYY-MM-DD) — pas l'UTC : une partie jouée le soir (Belgique) reste
 *  attribuée au bon jour calendaire local. Accepte une date ISO ou un objet Date. */
function localDay(d: string | Date): string {
  const x = typeof d === "string" ? new Date(d) : d;
  return `${x.getFullYear()}-${String(x.getMonth() + 1).padStart(2, "0")}-${String(x.getDate()).padStart(2, "0")}`;
}
/** Affiche une clé jour (YYYY-MM-DD) en libellé court lisible. */
function fmtDay(key: string): string {
  const [y, m, d] = key.split("-").map(Number);
  return new Date(y, m - 1, d).toLocaleDateString("en-GB", { weekday: "short", day: "2-digit", month: "short" });
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
