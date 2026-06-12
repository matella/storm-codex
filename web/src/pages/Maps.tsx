import { useQuery } from "@tanstack/react-query";
import { useNavigate } from "react-router-dom";
import { fmtDur } from "../api";

interface MapStat { map: string; games: number; blue_wins: number; avg_length: number }

export function Maps() {
  const nav = useNavigate();
  const { data, isLoading } = useQuery({
    queryKey: ["maps"],
    queryFn: async () => (await fetch("/api/maps")).json() as Promise<MapStat[]>,
  });
  return (
    <>
      <h1>Cartes</h1>
      <div className="card">
        {isLoading && <div className="empty">chargement…</div>}
        {data && (
          <table>
            <thead><tr><th>Carte</th><th>Parties</th><th>Win équipe bleue</th><th>Durée moy.</th></tr></thead>
            <tbody>
              {data.map((m) => {
                const wr = m.games ? (100 * m.blue_wins) / m.games : 0;
                return (
                  <tr key={m.map} className="link" onClick={() => nav(`/matches?map=${encodeURIComponent(m.map)}`)}>
                    <td>{m.map}</td>
                    <td className="mono">{m.games}</td>
                    <td className="mono tm-blue">{wr.toFixed(0)}%</td>
                    <td className="mono">{fmtDur(m.avg_length)}</td>
                  </tr>
                );
              })}
            </tbody>
          </table>
        )}
      </div>
    </>
  );
}
