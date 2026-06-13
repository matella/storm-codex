import { useQuery } from "@tanstack/react-query";
import { fetchMatches, matchOperator, useLiveUpdates, useDimHeroes, useSettings } from "../api";
import { Avatar } from "../components/Avatar";
import { OverlayFrame } from "../components/OverlayFrame";

/**
 * Overlay IN-GAME (browser source OBS) : pastille de coin ultra-compacte avec ton récap de session
 * — record du jour (W–L), streak, et le dernier héros joué. Fond transparent, mise à jour live via
 * WS après chaque partie. HotS n'expose aucune donnée pendant la partie (le replay n'est écrit qu'à
 * la fin) : ce ticker montre donc la session en cours, glançable sans gêner le jeu. À `/ticker`.
 */
export function Ticker() {
  useDimHeroes();
  useSettings();
  const { data, refetch } = useQuery({ queryKey: ["ticker"], queryFn: () => fetchMatches({ limit: 200 }) });
  useLiveUpdates(() => refetch());

  const matches = data ?? [];
  // parties de l'opérateur (tous comptes), récentes d'abord
  const opGames = matches.flatMap((m) => {
    const me = matchOperator(m.players ?? []);
    return me && m.played_at
      ? [{ me, won: me.team != null && m.winner === me.team, day: localDay(m.played_at) }]
      : [];
  });
  // session = aujourd'hui (heure locale) ; sinon la dernière session jouée
  const today = localDay(new Date());
  const todays = opGames.filter((g) => g.day === today);
  const games = todays.length ? todays : opGames[0] ? opGames.filter((g) => g.day === opGames[0].day) : [];

  if (!games.length) return <OverlayFrame anchor="top-left"><div /></OverlayFrame>;

  const wins = games.filter((g) => g.won).length;
  const losses = games.length - wins;
  const streak = currentStreak(games);
  const last = games[0];

  return (
    <OverlayFrame anchor="top-left">
      <div
        style={{
          display: "inline-flex",
          alignItems: "center",
          gap: 9,
          background: "rgba(14,16,22,.92)",
          border: "1px solid var(--hairline-strong)",
          borderRadius: 999,
          padding: "6px 13px 6px 7px",
          boxShadow: "0 6px 20px rgba(0,0,0,.5)",
          fontSize: 15,
        }}
      >
        <Avatar hero={last.me.hero} size={24} />
        <span className="mono" style={{ fontWeight: 600 }}>
          <span style={{ color: "var(--win)" }}>{wins}W</span>
          <span style={{ color: "var(--text-2)" }}>–</span>
          <span style={{ color: "var(--loss)" }}>{losses}L</span>
        </span>
        {streak !== 0 && (
          <span className="mono" style={{ fontSize: 12.5, color: streak > 0 ? "var(--win)" : "var(--loss)" }}>
            {streak > 0 ? `W${streak} ▲` : `L${-streak} ▼`}
          </span>
        )}
      </div>
    </OverlayFrame>
  );
}

/** Clé jour en heure LOCALE (YYYY-MM-DD) — pas l'UTC (cf. Queue). */
function localDay(d: string | Date): string {
  const x = typeof d === "string" ? new Date(d) : d;
  return `${x.getFullYear()}-${String(x.getMonth() + 1).padStart(2, "0")}-${String(x.getDate()).padStart(2, "0")}`;
}
/** Série en cours : +n victoires d'affilée, −n défaites, depuis la partie la plus récente. */
function currentStreak(games: { won: boolean }[]): number {
  if (!games.length) return 0;
  const first = games[0].won;
  let n = 0;
  for (const g of games) { if (g.won === first) n++; else break; }
  return first ? n : -n;
}
