import { useQuery } from "@tanstack/react-query";
import { Link } from "react-router-dom";
import { fetchMatches, modeBadge, fmtTime, fmtDur, pickOperator, matchOperator } from "../api";
import { Avatar } from "../components/Avatar";

export function Dashboard() {
  // assez de parties pour des stats opérateur représentatives (winrate récent, main héros)
  const { data: matches } = useQuery({
    queryKey: ["matches", "dashboard"],
    queryFn: () => fetchMatches({ limit: 500 }),
  });

  const last = matches?.[0];
  const lastMe = last ? pickOperator(last.players ?? []) : undefined;

  // stats DU POINT DE VUE OPÉRATEUR, calculées sur les parties où un nom configuré matche
  let games = 0;
  let wins = 0;
  const heroCount = new Map<string, number>();
  for (const m of matches ?? []) {
    const me = matchOperator(m.players ?? []);
    if (!me) continue;
    games += 1;
    if (m.winner != null && me.team === m.winner) wins += 1;
    if (me.hero) heroCount.set(me.hero, (heroCount.get(me.hero) ?? 0) + 1);
  }
  const wr = games ? ((100 * wins) / games).toFixed(1) : "—";
  const mainHero = [...heroCount.entries()].sort((a, b) => b[1] - a[1])[0]?.[0] ?? "—";
  const distinct = heroCount.size;

  return (
    <>
      <h1>Session</h1>
      <p className="note">Dashboard — your latest game and your stats (operator perspective).</p>

      <div className="card">
        {last && (
          <div className="card-hd">
            <Avatar hero={lastMe?.hero ?? null} size={42} />
            <div>
              <div style={{ fontSize: 13, fontWeight: 500 }}>
                Last game — {lastMe?.hero ?? "?"} · {last.map}{" "}
                <span className={`bdg ${modeBadge(last.mode).cls}`} style={{ marginLeft: 6 }}>
                  {modeBadge(last.mode).short}
                </span>
                {lastMe?.team != null && last.winner != null && (
                  <span
                    className={`bdg ${lastMe.team === last.winner ? "b-win" : "b-loss"}`}
                    style={{ marginLeft: 6 }}
                  >
                    {lastMe.team === last.winner ? "W" : "L"}
                  </span>
                )}
              </div>
              <div className="mono" style={{ fontSize: 11.5, color: "var(--text-2)", marginTop: 2 }}>
                {fmtTime(last.played_at)} · {fmtDur(last.length)} ·{" "}
                <Link to={`/match/${last.id}`} style={{ color: "var(--accent)" }}>details ›</Link>
              </div>
            </div>
          </div>
        )}
        <div style={{ display: "grid", gridTemplateColumns: "repeat(4,1fr)" }}>
          {[
            ["Win rate", `${wr}%`],
            ["My games", String(games)],
            ["Heroes played", String(distinct)],
            ["Main hero", mainHero],
          ].map(([k, v], idx) => (
            <div key={k} style={{ padding: "12px 18px", borderRight: idx < 3 ? "1px solid var(--hairline)" : undefined }}>
              <p className="kick" style={{ margin: "0 0 3px" }}>{k}</p>
              <p className="mono" style={{ margin: 0, fontSize: 15 }}>{v}</p>
            </div>
          ))}
        </div>
      </div>

      <p className="cap">Recent matches</p>
      <div className="card">
        {matches?.slice(0, 6).map((m) => {
          const me = pickOperator(m.players ?? []);
          return (
            <Link key={m.id} to={`/match/${m.id}`} className="row link">
              <span className="mono muted" style={{ minWidth: 92, fontSize: 11 }}>{fmtTime(m.played_at)}</span>
              <span className={`bdg ${modeBadge(m.mode).cls}`}>{modeBadge(m.mode).short}</span>
              <Avatar hero={me?.hero ?? null} />
              <span style={{ fontSize: 12 }}>{m.map}</span>
              {me?.team != null && m.winner != null && (
                <span className={`bdg ${me.team === m.winner ? "b-win" : "b-loss"}`}>
                  {me.team === m.winner ? "W" : "L"}
                </span>
              )}
              <span style={{ marginLeft: "auto", color: "var(--kicker)", fontSize: 10 }}>{fmtDur(m.length)} ›</span>
            </Link>
          );
        })}
      </div>
    </>
  );
}
