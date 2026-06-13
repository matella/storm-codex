import { useQuery } from "@tanstack/react-query";
import { useParams, Link } from "react-router-dom";
import { fetchMatch, modeBadge, fmtTime, fmtDur } from "../api";
import { Avatar } from "../components/Avatar";
import { LevelChart } from "../components/LevelChart";

const num = (v: any): number => (typeof v === "number" ? v : 0);

function ScoreTable({ players, team, label, cls }: { players: any[]; team: number; label: string; cls: string }) {
  const rows = players.filter((p) => p.team === team);
  return (
    <div className="card">
      <div className="card-hd">
        <h2 className={cls}>{label}</h2>
        <span style={{ marginLeft: "auto" }} className="muted mono">{rows.length} joueurs</span>
      </div>
      <table>
        <thead>
          <tr>
            <th>Player</th><th>K</th><th>D</th><th>A</th><th>Hero dmg</th><th>Healing</th><th>XP</th><th>Lvl</th>
          </tr>
        </thead>
        <tbody>
          {rows.map((p) => {
            const g = p.gameStats ?? {};
            return (
              <tr key={p.ToonHandle} className="link">
                <td>
                  <Link to={`/player/${encodeURIComponent(p.ToonHandle)}`} style={{ display: "flex", alignItems: "center", gap: 7 }}>
                    <Avatar hero={p.hero} size={20} />
                    <span>{p.hero}</span>
                    <span className="muted" style={{ fontSize: 10 }}>{p.name}</span>
                  </Link>
                </td>
                <td className="mono">{num(g.SoloKill)}</td>
                <td className="mono">{num(g.Deaths)}</td>
                <td className="mono">{num(g.Assists ?? g.Takedowns)}</td>
                <td className="mono">{num(g.HeroDamage).toLocaleString("fr-FR")}</td>
                <td className="mono">{num(g.Healing).toLocaleString("fr-FR")}</td>
                <td className="mono">{num(g.ExperienceContribution).toLocaleString("fr-FR")}</td>
                <td className="mono">{num(g.Level)}</td>
              </tr>
            );
          })}
        </tbody>
      </table>
    </div>
  );
}

export function MatchDetail() {
  const { id } = useParams();
  const { data, isLoading } = useQuery({ queryKey: ["match", id], queryFn: () => fetchMatch(id!) });

  if (isLoading) return <div className="empty">loading…</div>;
  if (!data) return <div className="empty">match not found</div>;
  const m = data.match;
  const players = Object.values(data.players ?? {});
  const mb = modeBadge(m.mode);
  const bans = m.bans ?? {};

  return (
    <>
      <h1 style={{ display: "flex", alignItems: "center", gap: 10 }}>
        {m.map} <span className={`bdg ${mb.cls}`}>{mb.short}</span>
        <span className="mono muted" style={{ fontSize: 11, fontWeight: 400 }}>
          {fmtTime(m.date)} · {fmtDur(m.length)} · build {m.version?.m_build}
        </span>
      </h1>

      {(m.picks || bans[0] || bans[1]) && (
        <div className="card">
          <div className="card-hd"><span className="kick" style={{ margin: 0 }}>Draft</span>
            {[0, 1].flatMap((t) => (bans[t] ?? []).map((b: any, i: number) => (
              <span key={`${t}-${i}`} className="bdg b-loss">ban {typeof b === "string" ? b : b.hero}</span>
            )))}
            {m.firstPickWin != null && (
              <span className="muted mono" style={{ marginLeft: "auto", fontSize: 10 }}>
                first pick {m.firstPickWin ? "gagne" : "perd"}
              </span>
            )}
          </div>
          {m.picks && [0, 1].map((t) => (
            <div key={t} className="row">
              <span className={t === 0 ? "tm-blue" : "tm-red"} style={{ minWidth: 70, fontSize: 11 }}>
                équipe {t === 0 ? "bleue" : "rouge"}{m.picks.first === t && <span className="bdg b-mvp" style={{ marginLeft: 5 }}>1st pick</span>}
              </span>
              <div style={{ display: "flex", gap: 6, flexWrap: "wrap" }}>
                {(m.picks[t] ?? []).map((h: string, i: number) => (
                  <span key={i} style={{ display: "flex", alignItems: "center", gap: 4 }}>
                    <Avatar hero={h} size={20} /><span style={{ fontSize: 11 }}>{h}</span>
                  </span>
                ))}
              </div>
            </div>
          ))}
        </div>
      )}

      {(m.firstObjective != null || m.firstFort != null) && (
        <div className="card">
          <div className="card-hd"><span className="kick" style={{ margin: 0 }}>Highlights</span></div>
          {[
            ["Premier objectif", m.firstObjective],
            ["Premier fort", m.firstFort],
            ["Premier keep", m.firstKeep],
          ].map(([label, v]) => (
            <div key={label as string} className="row">
              <span className="muted">{label}</span>
              <span style={{ marginLeft: "auto" }} className={v === 0 ? "tm-blue" : v === 1 ? "tm-red" : "muted"}>
                {v === 0 ? "blue team" : v === 1 ? "red team" : "—"}
              </span>
            </div>
          ))}
        </div>
      )}

      <ScoreTable players={players} team={0} label="Blue team" cls="tm-blue" />
      <ScoreTable players={players} team={1} label="Red team" cls="tm-red" />

      {Array.isArray(m.levelAdvTimeline) && m.levelAdvTimeline.length > 1 && (
        <>
          <p className="cap">Level advantage (blue +, red −)</p>
          <div className="card"><LevelChart timeline={m.levelAdvTimeline} /></div>
        </>
      )}

      <p className="cap">Full data</p>
      <div className="card">
        <div className="row">
          <span className="muted">Winner</span>
          <span style={{ marginLeft: "auto" }} className={m.winner === 0 ? "tm-blue" : "tm-red"}>
            équipe {m.winner === 0 ? "bleue" : "rouge"}
          </span>
        </div>
        <div className="row">
          <span className="muted">Takedowns</span>
          <span style={{ marginLeft: "auto" }} className="mono">{(m.takedowns ?? []).length}</span>
        </div>
        <div className="row">
          <span className="muted">Raw decoded dump</span>
          <a style={{ marginLeft: "auto", color: "var(--accent)" }} href={`/api/matches/${data.id}/raw?stream=tracker`} target="_blank" rel="noreferrer">
            tracker events ›
          </a>
        </div>
      </div>
    </>
  );
}
