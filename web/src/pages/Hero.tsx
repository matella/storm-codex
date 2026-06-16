import { useQuery } from "@tanstack/react-query";
import { useParams, Link } from "react-router-dom";
import { fetchHeroDetail, useDimTalents, talentInfo } from "../api";
import { Avatar } from "../components/Avatar";

const tierNum = (k: string) => parseInt(k.match(/\d+/)?.[0] ?? "0", 10);

/** Build de talents (TierNChoice → treeId) → chips ordonnés par tier, nom résolu via dim_talents. */
function Build({ talents }: { talents: Record<string, string> }) {
  const picks = Object.entries(talents)
    .filter(([k]) => /^Tier\d/.test(k))
    .sort((a, b) => tierNum(a[0]) - tierNum(b[0]))
    .map(([, tid]) => tid);
  return (
    <div style={{ display: "flex", flexWrap: "wrap", gap: 4 }}>
      {picks.map((tid, i) => (
        <span key={i} className="bdg b-qm" style={{ fontSize: 10 }} title={tid}>
          {talentInfo(tid)?.name ?? tid.replace(/([a-z])([A-Z])/g, "$1 $2")}
        </span>
      ))}
    </div>
  );
}

export function Hero() {
  const { name } = useParams();
  useDimTalents();
  const { data, isLoading } = useQuery({ queryKey: ["hero", name], queryFn: () => fetchHeroDetail(name!) });

  if (isLoading) return <div className="empty">loading…</div>;
  if (!data || !data.games) return <div className="empty">no games on {name} (as operator)</div>;
  const wr = data.games ? (100 * data.wins) / data.games : 0;
  const kda = (data.avg_takedowns ?? 0) / Math.max(1, data.avg_deaths ?? 0);

  return (
    <>
      <h1 style={{ display: "flex", alignItems: "center", gap: 12 }}>
        <Avatar hero={data.hero} size={40} /> {data.hero}
        <Link to={`/matches?hero=${encodeURIComponent(data.hero)}`} className="pill" style={{ fontSize: 11, marginLeft: "auto" }}>see games ›</Link>
      </h1>
      <p className="note">Your stats on this hero across all your accounts.</p>

      <div className="card">
        <div className="row" style={{ gap: 28, flexWrap: "wrap" }}>
          <Stat label="games" value={String(data.games)} />
          <Stat label="W–L" value={`${data.wins}–${data.games - data.wins}`} />
          <Stat label="win rate" value={`${wr.toFixed(0)}%`} color={wr >= 50 ? "var(--win)" : "var(--loss)"} />
          <Stat label="avg KDA" value={`${data.avg_kills}/${data.avg_deaths}/${data.avg_takedowns}`} />
          <Stat label="KDA ratio" value={kda.toFixed(1)} />
        </div>
      </div>

      <p className="cap">By map</p>
      <div className="card">
        <table>
          <thead><tr><th>Map</th><th>Games</th><th>W–L</th><th>Win rate</th></tr></thead>
          <tbody>
            {data.by_map.map((x) => {
              const w = x.games ? (100 * x.wins) / x.games : 0;
              return (
                <tr key={x.map}>
                  <td>{x.map}</td>
                  <td className="mono">{x.games}</td>
                  <td className="mono"><span style={{ color: "var(--win)" }}>{x.wins}</span>-<span style={{ color: "var(--loss)" }}>{x.games - x.wins}</span></td>
                  <td className="mono" style={{ color: w >= 50 ? "var(--win)" : "var(--loss)" }}>{w.toFixed(0)}%</td>
                </tr>
              );
            })}
          </tbody>
        </table>
      </div>

      {data.builds.length > 0 && (
        <>
          <p className="cap">Talent builds (most played)</p>
          <div className="card">
            {data.builds.map((b, i) => {
              const w = b.games ? (100 * b.wins) / b.games : 0;
              return (
                <div key={i} className="row" style={{ alignItems: "center", gap: 12 }}>
                  <Build talents={b.talents} />
                  <span className="mono" style={{ marginLeft: "auto", fontSize: 12 }}>
                    <span style={{ color: "var(--win)" }}>{b.wins}</span>-<span style={{ color: "var(--loss)" }}>{b.games - b.wins}</span>
                    <span style={{ color: w >= 50 ? "var(--win)" : "var(--loss)", marginLeft: 8 }}>{w.toFixed(0)}%</span>
                  </span>
                </div>
              );
            })}
          </div>
        </>
      )}
    </>
  );
}

function Stat({ label, value, color }: { label: string; value: string; color?: string }) {
  return (
    <div>
      <div className="mono" style={{ fontSize: 22, fontWeight: 700, color }}>{value}</div>
      <div className="kick" style={{ fontSize: 10 }}>{label}</div>
    </div>
  );
}
