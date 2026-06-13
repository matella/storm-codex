import { useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { useNavigate } from "react-router-dom";
import { fmtDur, aggParams, type AggFilter } from "../api";
import { AggFilterBar } from "../components/AggFilterBar";

interface MapStat { map: string; games: number; blue_wins: number; avg_length: number }

export function Maps() {
  const nav = useNavigate();
  const [filter, setFilter] = useState<AggFilter>({});
  const { data, isLoading } = useQuery({
    queryKey: ["maps", filter],
    queryFn: async () => (await fetch(`/api/maps?${aggParams(filter)}`)).json() as Promise<MapStat[]>,
  });
  return (
    <>
      <h1>Maps</h1>
      <p className="note">Map stats over the filtered set — {data?.length ?? 0} maps. "My games" restricts to games you played.</p>
      <div className="card">
        <AggFilterBar value={filter} onChange={setFilter} mineLabel="My games" />
        {isLoading && <div className="empty">loading…</div>}
        {data && (
          <table>
            <thead><tr><th>Map</th><th>Games</th><th>Blue team win</th><th>Avg. duration</th></tr></thead>
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
