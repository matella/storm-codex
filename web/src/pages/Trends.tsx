import { useQuery } from "@tanstack/react-query";
import { fmtDur } from "../api";

interface Trend { build: number; games: number; blue_wins: number; avg_length: number }

export function Trends() {
  const { data, isLoading } = useQuery({
    queryKey: ["trends"],
    queryFn: async () => (await fetch("/api/trends")).json() as Promise<Trend[]>,
  });
  return (
    <>
      <h1>Trends par patch</h1>
      <p className="note">Évolution par build (proxy de patch) — parties, winrate équipe bleue, durée moyenne.</p>
      <div className="card">
        {isLoading && <div className="empty">chargement…</div>}
        {data && (
          <table>
            <thead><tr><th>Build</th><th>Parties</th><th>Win bleue</th><th>Durée moy.</th><th></th></tr></thead>
            <tbody>
              {data.map((t) => {
                const wr = t.games ? (100 * t.blue_wins) / t.games : 0;
                return (
                  <tr key={t.build}>
                    <td className="mono">{t.build}</td>
                    <td className="mono">{t.games}</td>
                    <td className="mono tm-blue">{wr.toFixed(0)}%</td>
                    <td className="mono">{fmtDur(t.avg_length)}</td>
                    <td style={{ width: 120 }}><div className="gauge"><div style={{ width: `${wr}%` }} /></div></td>
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
