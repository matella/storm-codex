import { useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { useParams, Link } from "react-router-dom";
import DOMPurify from "dompurify";
import { fetchHeroDetail, fetchHeroPatches, useDimTalents, talentInfo, classBadge, fmtTime } from "../api";
import { Avatar } from "../components/Avatar";

const tierNum = (k: string) => parseInt(k.match(/\d+/)?.[0] ?? "0", 10);
const MODES: [string, number | undefined][] = [
  ["All", undefined], ["Storm League", 50091], ["ARAM", 50101], ["Custom", -1], ["Hero League", 50061], ["QM", 50001],
];

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
  const [mode, setMode] = useState<number | undefined>(undefined);
  useDimTalents();
  const { data, isLoading } = useQuery({ queryKey: ["hero", name, mode], queryFn: () => fetchHeroDetail(name!, mode) });

  if (isLoading) return <div className="empty">loading…</div>;
  const wr = data && data.games ? (100 * data.wins) / data.games : 0;
  const kda = (data?.avg_takedowns ?? 0) / Math.max(1, data?.avg_deaths ?? 0);

  return (
    <>
      <h1 style={{ display: "flex", alignItems: "center", gap: 12 }}>
        <Avatar hero={name ?? null} size={40} /> {name}
        <Link to={`/matches?hero=${encodeURIComponent(name ?? "")}`} className="pill" style={{ fontSize: 11, marginLeft: "auto" }}>see games ›</Link>
      </h1>
      <p className="note">Your stats on this hero across all your accounts.</p>

      <div className="card">
        <div className="card-hd" style={{ flexWrap: "wrap", gap: 6 }}>
          {MODES.map(([label, m]) => (
            <span key={label} className={mode === m ? "pill on" : "pill"} onClick={() => setMode(m)}>{label}</span>
          ))}
        </div>
      </div>

      {!data || !data.games ? (
        <div className="empty">no games on {name} for this filter</div>
      ) : (
      <>
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
      )}

      {name && <HeroPatches hero={name} />}
    </>
  );
}

/** Sens héros → patch : ajustements de ce héros à travers les patch notes (récent d'abord).
 *  Visible même sans parties jouées. Chaque entrée lie vers la section du patch concerné. */
function HeroPatches({ hero }: { hero: string }) {
  const { data } = useQuery({ queryKey: ["hero-patches", hero], queryFn: () => fetchHeroPatches(hero) });
  if (!data || data.length === 0) return null;
  return (
    <>
      <p className="cap">Patch changes</p>
      <div className="card" style={{ display: "flex", flexDirection: "column", gap: 8 }}>
        {data.map((p) => {
          const badge = classBadge(p.classification);
          return (
            <details key={p.patchInternalId + p.anchor} style={{ borderBottom: "1px solid var(--border, rgba(255,255,255,.08))", paddingBottom: 8 }}>
              <summary style={{ cursor: "pointer", display: "flex", alignItems: "center", gap: 8, flexWrap: "wrap" }}>
                <Link to={`/patch/${encodeURIComponent(p.patchInternalId)}#${p.anchor}`} onClick={(e) => e.stopPropagation()} style={{ color: "var(--accent)", textDecoration: "none", fontWeight: 600 }}>{p.patchName}</Link>
                <span className="mono muted" style={{ fontSize: 11 }}>{fmtTime(p.liveDate)}</span>
                {badge && <span style={{ fontSize: 9, fontWeight: 700, padding: "0 5px", borderRadius: 4, background: badge.bg, color: badge.fg }}>{badge.label}</span>}
                {p.shortSummary && <span className="muted" style={{ fontSize: 12 }}>{p.shortSummary}</span>}
              </summary>
              {p.content && (
                <div className="patch-content" style={{ marginTop: 6, lineHeight: 1.5 }} dangerouslySetInnerHTML={{ __html: DOMPurify.sanitize(p.content) }} />
              )}
            </details>
          );
        })}
      </div>
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
