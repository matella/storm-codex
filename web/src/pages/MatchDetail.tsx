import { Fragment } from "react";
import { useQuery } from "@tanstack/react-query";
import { useParams, Link } from "react-router-dom";
import { fetchMatch, modeBadge, fmtTime, fmtDur, useDimTalents, talentInfo, awardLabel } from "../api";
import { Avatar } from "../components/Avatar";
import { LevelChart } from "../components/LevelChart";

const num = (v: any): number => (typeof v === "number" ? v : 0);
const tierNum = (k: string): number => parseInt(k.match(/\d+/)?.[0] ?? "0", 10);
const decamel = (s: string): string => s.replace(/([a-z])([A-Z])/g, "$1 $2");

/** Build de talents d'un joueur : `talents` = `{TierNChoice: talentTreeId}` (écrit par le parser).
 *  Affiché en chips, dans l'ordre des tiers. Nom résolu via dim_talents, sinon id décamelisé. */
function TalentStrip({ talents }: { talents: Record<string, string> | undefined }) {
  if (!talents) return null;
  const picks = Object.entries(talents)
    .filter(([k]) => /^Tier\d/.test(k))
    .sort((a, b) => tierNum(a[0]) - tierNum(b[0]))
    .map(([, treeId]) => treeId);
  if (!picks.length) return null;
  return (
    <div style={{ display: "flex", flexWrap: "wrap", gap: 4, padding: "2px 0 4px" }}>
      {picks.map((tid, i) => {
        const info = talentInfo(tid);
        return (
          <span key={i} className="bdg b-qm" title={`Tier ${i + 1}${info ? ` · ${info.name}` : ""}`} style={{ fontSize: 10 }}>
            {info?.name ?? decamel(tid)}
          </span>
        );
      })}
    </div>
  );
}

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
              <Fragment key={p.ToonHandle}>
                <tr className="link">
                  <td>
                    <Link to={`/player/${encodeURIComponent(p.ToonHandle)}`} style={{ display: "flex", alignItems: "center", gap: 7 }}>
                      <Avatar hero={p.hero} size={20} />
                      <span>{p.hero}</span>
                      <span className="muted" style={{ fontSize: 10 }}>{p.name}</span>
                      {(() => {
                        const aw = awardLabel((g.awards ?? [])[0]);
                        if (!aw) return null;
                        return aw.mvp ? (
                          <span title="MVP" style={{ fontSize: 9, fontWeight: 700, padding: "1px 5px", borderRadius: 999, color: "#1a1500", background: "linear-gradient(90deg,#f5c542,#e0a818)" }}>MVP</span>
                        ) : (
                          <span className="muted" title={aw.label} style={{ fontSize: 9 }}>{aw.icon} {aw.label}</span>
                        );
                      })()}
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
                {p.talents && (
                  <tr>
                    <td colSpan={8} style={{ paddingTop: 0 }}><TalentStrip talents={p.talents} /></td>
                  </tr>
                )}
              </Fragment>
            );
          })}
        </tbody>
      </table>
    </div>
  );
}

/** Nom d'objectif par type de carte (le champ `objective.type` = nom de la carte). */
const OBJ_NOUN: Record<string, string> = {
  "Dragon Shire": "Dragon Knight",
  "Garden of Terror": "Garden Terror",
  "Cursed Hollow": "Curse",
  "Battlefield of Eternity": "Immortal",
  "Tomb of the Spider Queen": "Webweaver",
  "Sky Temple": "Temple",
  "Towers of Doom": "Altar",
  "Braxis Holdout": "Zerg wave",
  "Volskaya Foundry": "Triglav",
  "Infernal Shrines": "Punisher",
  "Hanamura Temple": "Payload",
  "Blackheart's Bay": "Cannons",
  "Alterac Pass": "Cavalry",
};

/** Événements d'objectif (horodatés, par équipe) extraits du champ `objective` map-spécifique :
 *  `results` (Battlefield) ou buckets `0`/`1`.events (Dragon Shire, Garden…). Générique. */
function objectiveEvents(m: any): { t: number; team: number; label: string }[] {
  const o = m.objective;
  if (!o || typeof o !== "object") return [];
  const noun = OBJ_NOUN[o.type] ?? "Objective";
  const out: { t: number; team: number; label: string }[] = [];
  for (const r of (o.results ?? []) as any[]) {
    if (r?.time != null && (r.winner === 0 || r.winner === 1)) out.push({ t: r.time, team: r.winner, label: noun });
  }
  for (const k of ["0", "1"]) {
    for (const e of (o[k]?.events ?? []) as any[]) {
      if (e?.time != null) out.push({ t: e.time, team: Number(k), label: noun });
    }
  }
  return out;
}

/** Timeline chronologique des événements du match (kills + structures détruites + objectifs),
 *  reconstruite des données déjà stockées (horodatées). Couleur = équipe qui MARQUE (opposée à la
 *  victime / au propriétaire de la structure ; équipe qui prend l'objectif). */
function MatchTimeline({ m, players }: { m: any; players: Record<string, any> }) {
  const teamOf = (toon: string | undefined): number | undefined => (toon ? players[toon]?.team : undefined);
  type Ev = { t: number; kind: "kill" | "struct" | "obj"; team: number; label: string; hero?: string | null };
  const evs: Ev[] = [];
  for (const td of (m.takedowns ?? []) as any[]) {
    const vt = teamOf(td?.victim?.player);
    const n = td?.killers?.length ?? 0;
    evs.push({
      t: td.time ?? 0, kind: "kill", team: vt === 0 ? 1 : 0, hero: td?.victim?.hero ?? null,
      label: `${td?.victim?.hero ?? "?"} killed${n > 1 ? ` ×${n}` : ""}`,
    });
  }
  for (const s of Object.values(m.structures ?? {}) as any[]) {
    if (s?.destroyed == null) continue;
    evs.push({ t: s.destroyed, kind: "struct", team: s.team === 0 ? 1 : 0, label: `${s.name} destroyed` });
  }
  for (const o of objectiveEvents(m)) evs.push({ t: o.t, kind: "obj", team: o.team, label: o.label });
  evs.sort((a, b) => a.t - b.t);
  if (!evs.length) return null;
  return (
    <>
      <p className="cap">Timeline — {evs.length} events</p>
      <div className="card" style={{ maxHeight: 440, overflowY: "auto" }}>
        {evs.map((e, i) => (
          <div key={i} className="row" style={{ gap: 9, borderLeft: `3px solid ${e.team === 0 ? "var(--tm-blue)" : "var(--tm-red)"}`, paddingLeft: 10 }}>
            <span className="mono muted" style={{ minWidth: 42, fontSize: 11 }}>{fmtDur(e.t)}</span>
            <span style={{ fontSize: 13 }}>{e.kind === "kill" ? "⚔️" : e.kind === "struct" ? "🏰" : "🎯"}</span>
            {e.kind === "kill" && <Avatar hero={e.hero ?? null} size={18} />}
            <span style={{ fontSize: 12 }}>{e.label}</span>
            <span className={e.team === 0 ? "tm-blue" : "tm-red"} style={{ marginLeft: "auto", fontSize: 10 }}>{e.team === 0 ? "blue" : "red"}</span>
          </div>
        ))}
      </div>
    </>
  );
}

export function MatchDetail() {
  const { id } = useParams();
  useDimTalents(); // peuple le référentiel talents (talentTreeId → nom)
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

      <MatchTimeline m={m} players={data.players ?? {}} />

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
