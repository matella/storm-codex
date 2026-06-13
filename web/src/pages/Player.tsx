import { useQuery } from "@tanstack/react-query";
import { useParams } from "react-router-dom";
import { fetchPlayer } from "../api";
import { Avatar } from "../components/Avatar";

export function Player() {
  const { toon } = useParams();
  const { data, isLoading } = useQuery({ queryKey: ["player", toon], queryFn: () => fetchPlayer(toon!) });
  if (isLoading) return <div className="empty">loading…</div>;
  if (!data) return <div className="empty">player not found</div>;
  const wr = data.matches ? ((100 * data.wins) / data.matches).toFixed(1) : "—";
  return (
    <>
      <h1>{data.name ?? data.toon}</h1>
      <p className="note mono">{data.toon} · alias : {data.names.join(", ") || "—"}</p>
      <div className="card">
        <div style={{ display: "grid", gridTemplateColumns: "repeat(3,1fr)" }}>
          {[["Games", String(data.matches)], ["Wins", String(data.wins)], ["Win rate", `${wr}%`]].map(([k, v], i) => (
            <div key={k} style={{ padding: "12px 18px", borderRight: i < 2 ? "1px solid var(--hairline)" : undefined }}>
              <p className="kick" style={{ margin: "0 0 3px" }}>{k}</p>
              <p className="mono" style={{ margin: 0, fontSize: 15 }}>{v}</p>
            </div>
          ))}
        </div>
      </div>
      <p className="cap">Hero pool</p>
      <div className="card">
        <table>
          <thead><tr><th>Heroes</th><th>Games</th><th>Wins</th><th>Win rate</th></tr></thead>
          <tbody>
            {data.heroes.map((h) => {
              const r = h.games ? (100 * h.wins) / h.games : 0;
              return (
                <tr key={h.hero}>
                  <td><span style={{ display: "flex", alignItems: "center", gap: 8 }}><Avatar hero={h.hero} size={20} /> {h.hero}</span></td>
                  <td className="mono">{h.games}</td>
                  <td className="mono">{h.wins}</td>
                  <td className="mono" style={{ color: r >= 50 ? "var(--win)" : "var(--tm-red)" }}>{r.toFixed(0)}%</td>
                </tr>
              );
            })}
          </tbody>
        </table>
      </div>
    </>
  );
}
