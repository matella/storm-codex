import { useQuery } from "@tanstack/react-query";
import { Link } from "react-router-dom";
import { fetchMatches, fetchHeroes, modeBadge, fmtTime, fmtDur } from "../api";
import { Avatar } from "../components/Avatar";

export function Dashboard() {
  const { data: matches } = useQuery({ queryKey: ["matches", { mode: undefined }], queryFn: () => fetchMatches({ limit: 100 }) });
  const { data: heroes } = useQuery({ queryKey: ["heroes"], queryFn: fetchHeroes });

  const last = matches?.[0];
  const total = matches?.length ?? 0;
  const topHero = heroes?.[0];
  const totalGames = heroes?.reduce((s, h) => s + h.games, 0) ?? 0;
  const totalWins = heroes?.reduce((s, h) => s + h.wins, 0) ?? 0;
  const wr = totalGames ? ((100 * totalWins) / totalGames).toFixed(1) : "—";

  return (
    <>
      <h1>Session</h1>
      <p className="note">Tableau de bord — dernière partie et stats agrégées (DB backfillée).</p>

      <div className="card">
        {last && (
          <div className="card-hd">
            <Avatar hero={last.players?.[0]?.hero ?? null} size={42} />
            <div>
              <div style={{ fontSize: 13, fontWeight: 500 }}>
                Dernière partie — {last.players?.[0]?.hero ?? "?"} · {last.map}{" "}
                <span className={`bdg ${modeBadge(last.mode).cls}`} style={{ marginLeft: 6 }}>
                  {modeBadge(last.mode).short}
                </span>
              </div>
              <div className="mono" style={{ fontSize: 11.5, color: "var(--text-2)", marginTop: 2 }}>
                {fmtTime(last.played_at)} · {fmtDur(last.length)} ·{" "}
                <Link to={`/match/${last.id}`} style={{ color: "var(--accent)" }}>détail ›</Link>
              </div>
            </div>
          </div>
        )}
        <div style={{ display: "grid", gridTemplateColumns: "repeat(4,1fr)" }}>
          {[
            ["Win rate", `${wr}%`],
            ["Parties (DB)", String(total >= 100 ? "100+" : total)],
            ["Héros distincts", String(heroes?.length ?? 0)],
            ["Main héros", topHero ? `${topHero.hero}` : "—"],
          ].map(([k, v], idx) => (
            <div key={k} style={{ padding: "12px 18px", borderRight: idx < 3 ? "1px solid var(--hairline)" : undefined }}>
              <p className="kick" style={{ margin: "0 0 3px" }}>{k}</p>
              <p className="mono" style={{ margin: 0, fontSize: 15 }}>{v}</p>
            </div>
          ))}
        </div>
      </div>

      <p className="cap">Derniers matchs</p>
      <div className="card">
        {matches?.slice(0, 6).map((m) => (
          <Link key={m.id} to={`/match/${m.id}`} className="row link">
            <span className="mono muted" style={{ minWidth: 92, fontSize: 11 }}>{fmtTime(m.played_at)}</span>
            <span className={`bdg ${modeBadge(m.mode).cls}`}>{modeBadge(m.mode).short}</span>
            <Avatar hero={m.players?.[0]?.hero ?? null} />
            <span style={{ fontSize: 12 }}>{m.map}</span>
            <span style={{ marginLeft: "auto", color: "var(--kicker)", fontSize: 10 }}>{fmtDur(m.length)} ›</span>
          </Link>
        ))}
      </div>
    </>
  );
}
