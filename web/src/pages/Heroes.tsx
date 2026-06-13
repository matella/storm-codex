import { useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { useNavigate } from "react-router-dom";
import { fetchHeroes, type HeroStat } from "../api";
import { Avatar } from "../components/Avatar";

type SortKey = "games" | "winrate";

export function Heroes() {
  const [sort, setSort] = useState<SortKey>("games");
  const nav = useNavigate();
  const { data, isLoading } = useQuery({ queryKey: ["heroes"], queryFn: fetchHeroes });

  const wr = (h: HeroStat) => (h.games ? h.wins / h.games : 0);
  const rows = [...(data ?? [])].sort((a, b) =>
    sort === "games" ? b.games - a.games : wr(b) - wr(a)
  );

  return (
    <>
      <h1>Heroes</h1>
      <p className="note">Statistiques agrégées sur la base backfillée — {data?.length ?? 0} héros joués.</p>
      <div className="card">
        {isLoading && <div className="empty">loading…</div>}
        {data && (
          <table>
            <thead>
              <tr>
                <th>Heroes</th>
                <th onClick={() => setSort("games")} style={{ color: sort === "games" ? "var(--accent)" : undefined }}>Games</th>
                <th>Wins</th>
                <th onClick={() => setSort("winrate")} style={{ color: sort === "winrate" ? "var(--accent)" : undefined }}>Win rate</th>
                <th></th>
              </tr>
            </thead>
            <tbody>
              {rows.map((h) => {
                const rate = (100 * wr(h)).toFixed(1);
                return (
                  <tr key={h.hero} className="link" onClick={() => nav(`/matches?hero=${encodeURIComponent(h.hero)}`)}>
                    <td>
                      <span style={{ display: "flex", alignItems: "center", gap: 8 }}>
                        <Avatar hero={h.hero} size={20} /> {h.hero}
                      </span>
                    </td>
                    <td className="mono">{h.games}</td>
                    <td className="mono">{h.wins}</td>
                    <td className="mono" style={{ color: wr(h) >= 0.5 ? "var(--win)" : "var(--tm-red)" }}>{rate}%</td>
                    <td style={{ width: 90 }}>
                      <div className="gauge"><div style={{ width: `${rate}%`, background: wr(h) >= 0.5 ? "var(--win)" : "var(--tm-red)" }} /></div>
                    </td>
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
