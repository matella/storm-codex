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
            <th>Joueur</th><th>K</th><th>D</th><th>A</th><th>Héros dmg</th><th>Soin</th><th>XP</th><th>Niv</th>
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

  if (isLoading) return <div className="empty">chargement…</div>;
  if (!data) return <div className="empty">match introuvable</div>;
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
        </div>
      )}

      <ScoreTable players={players} team={0} label="Équipe bleue" cls="tm-blue" />
      <ScoreTable players={players} team={1} label="Équipe rouge" cls="tm-red" />

      {Array.isArray(m.levelAdvTimeline) && m.levelAdvTimeline.length > 1 && (
        <>
          <p className="cap">Avantage de niveau (bleue +, rouge −)</p>
          <div className="card"><LevelChart timeline={m.levelAdvTimeline} /></div>
        </>
      )}

      <p className="cap">Données complètes</p>
      <div className="card">
        <div className="row">
          <span className="muted">Vainqueur</span>
          <span style={{ marginLeft: "auto" }} className={m.winner === 0 ? "tm-blue" : "tm-red"}>
            équipe {m.winner === 0 ? "bleue" : "rouge"}
          </span>
        </div>
        <div className="row">
          <span className="muted">Takedowns</span>
          <span style={{ marginLeft: "auto" }} className="mono">{(m.takedowns ?? []).length}</span>
        </div>
        <div className="row">
          <span className="muted">Dump décodé brut</span>
          <a style={{ marginLeft: "auto", color: "var(--accent)" }} href={`/api/matches/${data.id}/raw?stream=tracker`} target="_blank" rel="noreferrer">
            tracker events ›
          </a>
        </div>
      </div>
    </>
  );
}
