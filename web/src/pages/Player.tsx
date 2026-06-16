import { useQuery } from "@tanstack/react-query";
import { useParams, useNavigate } from "react-router-dom";
import { fetchPlayer, modeBadge, fmtTime, awardLabel } from "../api";
import { Avatar } from "../components/Avatar";

export function Player() {
  const { toon } = useParams();
  const nav = useNavigate();
  const { data, isLoading } = useQuery({ queryKey: ["player", toon], queryFn: () => fetchPlayer(toon!) });
  if (isLoading) return <div className="empty">loading…</div>;
  if (!data) return <div className="empty">player not found</div>;
  const wr = data.matches ? ((100 * data.wins) / data.matches).toFixed(1) : "—";
  const kdaRatio = ((data.avg_takedowns ?? 0) / Math.max(1, data.avg_deaths ?? 0)).toFixed(1);

  return (
    <>
      <h1>{data.name ?? data.toon}</h1>
      <p className="note mono">{data.toon} · alias : {data.names.join(", ") || "—"}</p>
      <div className="card">
        <div style={{ display: "grid", gridTemplateColumns: "repeat(5,1fr)" }}>
          {[
            ["Games", String(data.matches)],
            ["Wins", String(data.wins)],
            ["Win rate", `${wr}%`],
            ["Avg KDA", `${data.avg_kills}/${data.avg_deaths}/${data.avg_takedowns}`],
            ["KDA ratio", kdaRatio],
          ].map(([k, v], i) => (
            <div key={k} style={{ padding: "12px 18px", borderRight: i < 4 ? "1px solid var(--hairline)" : undefined }}>
              <p className="kick" style={{ margin: "0 0 3px" }}>{k}</p>
              <p className="mono" style={{ margin: 0, fontSize: 15 }}>{v}</p>
            </div>
          ))}
        </div>
      </div>

      <p className="cap">Hero pool</p>
      <div className="card">
        <table>
          <thead><tr><th>Heroes</th><th>Games</th><th>Win rate</th><th>Avg KDA</th></tr></thead>
          <tbody>
            {data.heroes.map((h) => {
              const r = h.games ? (100 * h.wins) / h.games : 0;
              return (
                <tr key={h.hero}>
                  <td><span style={{ display: "flex", alignItems: "center", gap: 8 }}><Avatar hero={h.hero} size={20} /> {h.hero}</span></td>
                  <td className="mono">{h.games}</td>
                  <td className="mono" style={{ color: r >= 50 ? "var(--win)" : "var(--loss)" }}>{r.toFixed(0)}%</td>
                  <td className="mono muted">{h.avg_kills}/{h.avg_deaths}/{h.avg_takedowns}</td>
                </tr>
              );
            })}
          </tbody>
        </table>
      </div>

      <p className="cap">Recent games</p>
      <div className="card">
        <table>
          <thead><tr><th>When</th><th>Mode</th><th>Hero</th><th>Map</th><th>Result</th><th>KDA</th></tr></thead>
          <tbody>
            {data.recent.map((g) => {
              const mb = modeBadge(g.mode);
              const aw = awardLabel(g.award);
              return (
                <tr key={g.match_id} className="link" onClick={() => nav(`/match/${g.match_id}`)}>
                  <td className="mono muted" style={{ fontSize: 11 }}>{fmtTime(g.played_at)}</td>
                  <td><span className={`bdg ${mb.cls}`}>{mb.short}</span></td>
                  <td><span style={{ display: "flex", alignItems: "center", gap: 7 }}><Avatar hero={g.hero} size={18} /> {g.hero}{aw && <span title={aw.label} style={{ fontSize: 11 }}>{aw.mvp ? "👑" : aw.icon}</span>}</span></td>
                  <td style={{ fontSize: 12 }}>{g.map}</td>
                  <td><span className={`bdg ${g.win ? "b-win" : "b-loss"}`}>{g.win ? "W" : "L"}</span></td>
                  <td className="mono">{g.kills}/{Math.max(0, (g.takedowns ?? 0) - (g.kills ?? 0))}/{g.deaths}</td>
                </tr>
              );
            })}
          </tbody>
        </table>
      </div>
    </>
  );
}
