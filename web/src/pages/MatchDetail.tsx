import { useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { useParams, Link } from "react-router-dom";
import {
  fetchMatch, fmtTime, fmtDur, fmtClock, announceLabel, useDimTalents, useDimHeroAttributes, banHero, talentInfo,
  matchOperator, useSettings,
} from "../api";
import { Portrait, SecHead } from "../components/ds";
import { AwardTag, LevelAdvantage, ModeTag, hasMapArt, mapArt } from "../components/ds/match";
import { Replay2D } from "../components/Replay2D";

// Les objets `match`/`players` viennent tels quels de storm-stats (JSON riche, non typé) : `any`.
const num = (v: any): number => (typeof v === "number" ? v : 0);
const tierNum = (k: string): number => parseInt(k.match(/\d+/)?.[0] ?? "0", 10);
const decamel = (s: string): string => s.replace(/([a-z])([A-Z])/g, "$1 $2");
const fmtN = (n: number) => n.toLocaleString("fr-FR");
const fmtK = (n: number) => (n >= 1000 ? `${(n / 1000).toFixed(1)}k` : String(Math.round(n)));

/** Build de talents d'un joueur (`{TierNChoice: talentTreeId}`), en puces ordonnées par palier. */
function TalentStrip({ talents }: { talents: Record<string, string> | undefined }) {
  if (!talents) return null;
  const picks = Object.entries(talents).filter(([k]) => /^Tier\d/.test(k)).sort((a, b) => tierNum(a[0]) - tierNum(b[0])).map(([, id]) => id);
  if (!picks.length) return null;
  return (
    <div className="ds-talents">
      {picks.map((tid, i) => {
        const info = talentInfo(tid);
        return <span key={i} className="ds-talent" title={`Tier ${i + 1}${info ? ` · ${info.name}` : ""}`}><b>{info?.tier ?? i + 1}</b>{info?.name ?? decamel(tid)}</span>;
      })}
    </div>
  );
}

/** Colonnes du tableau de score. `adv` = colonnes étendues (masquées par défaut) ; `bar` = jauge
 *  relative au max de la partie ; `total` = sommée dans la ligne « Team total ». */
const COLS: { label: string; get: (g: any) => number; adv?: boolean; total?: boolean; bar?: boolean }[] = [
  { label: "Hero dmg", get: (g) => num(g.HeroDamage), total: true, bar: true },
  { label: "Siege", get: (g) => num(g.SiegeDamage), total: true, bar: true },
  { label: "Healing", get: (g) => num(g.Healing), total: true, bar: true },
  { label: "XP", get: (g) => num(g.ExperienceContribution), total: true, bar: true },
  { label: "Lvl", get: (g) => num(g.Level) },
  { label: "Spell", get: (g) => num(g.SpellDamage), adv: true, total: true, bar: true },
  { label: "Taken", get: (g) => num(g.DamageTaken), adv: true, total: true, bar: true },
  { label: "Self-heal", get: (g) => num(g.SelfHealing), adv: true, total: true },
  { label: "CC s", get: (g) => num(g.TimeCCdEnemyHeroes), adv: true, total: true },
  { label: "Mercs", get: (g) => num(g.MercCampCaptures), adv: true, total: true },
];

function ScoreTable({ players, team, label, adv, meToon, max }: {
  players: any[]; team: number; label: string; adv: boolean; meToon: string | null; max: Record<string, number>;
}) {
  const rows = players.filter((p) => p.team === team);
  const cols = COLS.filter((c) => !c.adv || adv);
  const red = team === 1;
  return (
    <div className="ds-panel flush scroll" style={{ marginTop: 14 }}>
      <table className="ds-table">
        <thead>
          <tr><th className={red ? "ds-them" : "ds-us"}>{label}</th><th>K/D/A</th>{cols.map((c) => <th key={c.label}>{c.label}</th>)}</tr>
        </thead>
        <tbody>
          {rows.map((p) => {
            const g = p.gameStats ?? {};
            const me = p.ToonHandle === meToon;
            return [
              <tr key={p.ToonHandle} className={me ? "me" : undefined}>
                <td>
                  <Link to={`/player/${encodeURIComponent(p.ToonHandle)}`} className="ds-who">
                    <Portrait hero={p.hero} size={36} tone={red ? "red" : "blue"} />
                    <div><strong>{p.hero}</strong><em>{p.name}</em></div>
                    <AwardTag raw={(g.awards ?? [])[0]} />
                  </Link>
                </td>
                <td className="ds-kda">{num(g.SoloKill)}<i>/</i>{num(g.Deaths)}<i>/</i>{num(g.Assists ?? g.Takedowns)}</td>
                {cols.map((c) => (
                  <td key={c.label}>
                    {c.bar ? (
                      <div className="ds-cell"><span>{fmtK(c.get(g))}</span>
                        <i className={red ? "ds-bar red" : "ds-bar"}><b style={{ width: `${Math.round((c.get(g) * 100) / (max[c.label] || 1))}%` }} /></i></div>
                    ) : <span className="ds-num">{fmtN(c.get(g))}</span>}
                  </td>
                ))}
              </tr>,
              p.talents ? <tr key={`${p.ToonHandle}-t`} className={me ? "me tal" : "tal"}><td colSpan={cols.length + 2}><TalentStrip talents={p.talents} /></td></tr> : null,
            ];
          })}
          <tr className="total">
            <td>Team total</td>
            <td>{rows.reduce((s, p) => s + num(p.gameStats?.SoloKill), 0)}/{rows.reduce((s, p) => s + num(p.gameStats?.Deaths), 0)}</td>
            {cols.map((c) => <td key={c.label}>{c.total ? fmtN(rows.reduce((s, p) => s + c.get(p.gameStats ?? {}), 0)) : ""}</td>)}
          </tr>
        </tbody>
      </table>
    </div>
  );
}

/** Nom d'objectif par type de carte (le champ `objective.type` = nom de la carte). */
const OBJ_NOUN: Record<string, string> = {
  "Dragon Shire": "Dragon Knight", "Garden of Terror": "Garden Terror", "Cursed Hollow": "Curse",
  "Battlefield of Eternity": "Immortal", "Tomb of the Spider Queen": "Webweaver", "Sky Temple": "Temple",
  "Towers of Doom": "Altar", "Braxis Holdout": "Zerg wave", "Volskaya Foundry": "Triglav", "Infernal Shrines": "Punisher",
  "Hanamura Temple": "Payload", "Blackheart's Bay": "Cannons", "Alterac Pass": "Cavalry",
};

/** Événements d'objectif (horodatés, par équipe) : `results` (Battlefield) ou buckets `0`/`1`.events. */
function objectiveEvents(m: any): { t: number; team: number; label: string }[] {
  const o = m.objective;
  if (!o || typeof o !== "object") return [];
  const noun = OBJ_NOUN[o.type] ?? "Objective";
  const out: { t: number; team: number; label: string }[] = [];
  for (const r of (o.results ?? []) as any[]) if (r?.time != null && (r.winner === 0 || r.winner === 1)) out.push({ t: r.time, team: r.winner, label: noun });
  for (const k of ["0", "1"]) for (const e of (o[k]?.events ?? []) as any[]) if (e?.time != null) out.push({ t: e.time, team: Number(k), label: noun });
  return out;
}

/** Timeline des événements (kills, structures, objectifs) : piste de repères + liste. Couleur =
 *  équipe qui MARQUE (opposée à la victime / au propriétaire de la structure). */
function MatchTimeline({ m, players, num: n }: { m: any; players: Record<string, any>; num: number }) {
  const teamOf = (toon: string | undefined): number | undefined => (toon ? players[toon]?.team : undefined);
  type Ev = { t: number; kind: "kill" | "struct" | "obj"; team: number; label: string; hero?: string | null };
  const evs: Ev[] = [];
  for (const td of (m.takedowns ?? []) as any[]) {
    const vt = teamOf(td?.victim?.player);
    const k = td?.killers?.length ?? 0;
    evs.push({ t: td.time ?? 0, kind: "kill", team: vt === 0 ? 1 : 0, hero: td?.victim?.hero ?? null, label: `${td?.victim?.hero ?? "?"} killed${k > 1 ? ` ×${k}` : ""}` });
  }
  for (const s of Object.values(m.structures ?? {}) as any[]) {
    if (s?.destroyed == null) continue;
    evs.push({ t: s.destroyed, kind: "struct", team: s.team === 0 ? 1 : 0, label: `${s.name} destroyed` });
  }
  for (const o of objectiveEvents(m)) evs.push({ t: o.t, kind: "obj", team: o.team, label: o.label });
  evs.sort((a, b) => a.t - b.t);
  if (!evs.length) return null;
  const maxT = Math.max(m.length || 0, ...evs.map((e) => e.t)) || 1;
  const col = (team: number) => (team === 0 ? "var(--tm-blue)" : "var(--tm-red)");
  return (
    <section className="ds-sec">
      <SecHead num={n} title="Timeline" sub={`${evs.length} events · kills below the line, 🏰 structures and 🎯 objectives above`} />
      <div className="ds-panel">
        <div className="ds-track">
          <div className="base" />
          {evs.map((e, i) => {
            const left = `${(e.t / maxT) * 100}%`;
            if (e.kind === "kill") return <span key={i} className="k" title={`${fmtDur(e.t)} · ${e.label}`} style={{ left, background: col(e.team) }} />;
            return <span key={i} className="pin" title={`${fmtDur(e.t)} · ${e.label}`} style={{ left, top: e.kind === "obj" ? 0 : 12, borderBottom: `2px solid ${col(e.team)}` }}>{e.kind === "obj" ? "🎯" : "🏰"}</span>;
          })}
        </div>
        <div className="ds-lvl-legend"><span>0:00</span><span>{fmtDur(maxT)}</span></div>
      </div>
      <div className="ds-panel flush ds-evlist" style={{ marginTop: 12 }}>
        {evs.map((e, i) => (
          <div key={i} className={`ds-ev-row ${e.team === 0 ? "blue" : "red"}`}>
            <span className="t">{fmtDur(e.t)}</span>
            <span>{e.kind === "kill" ? "⚔️" : e.kind === "struct" ? "🏰" : "🎯"}</span>
            {e.kind === "kill" && e.hero && <Portrait hero={e.hero} size={20} />}
            <span>{e.label}</span>
            <span className={e.team === 0 ? "ds-us" : "ds-them"} style={{ marginLeft: "auto", fontSize: 10, fontFamily: "var(--mono)" }}>{e.team === 0 ? "BLUE" : "RED"}</span>
          </div>
        ))}
      </div>
    </section>
  );
}

/** Courbe d'XP par équipe (somme du `breakdown` par échantillon, hors champs temps). */
function XPCurve({ data }: { data: any[] }) {
  if (!Array.isArray(data) || data.length < 2) return null;
  const sumXP = (b: any) => Object.entries(b || {}).reduce((s, [k, v]) => (typeof v === "number" && !/Time/i.test(k) ? s + v : s), 0);
  const series: Record<number, { t: number; xp: number }[]> = { 0: [], 1: [] };
  for (const d of data) series[d.team === 0 ? 0 : 1].push({ t: d.time ?? 0, xp: sumXP(d.breakdown) });
  [0, 1].forEach((t) => series[t].sort((a, b) => a.t - b.t));
  const maxXP = Math.max(1, ...data.map((d) => sumXP(d.breakdown)));
  const maxT = Math.max(1, ...data.map((d) => d.time ?? 0));
  const line = (pts: { t: number; xp: number }[]) => pts.map((p) => `${(p.t / maxT) * 100},${100 - (p.xp / maxXP) * 100}`).join(" ");
  return (
    <svg width="100%" height="130" viewBox="0 0 100 100" preserveAspectRatio="none" role="img" aria-label="Team XP over time">
      <polyline points={line(series[0])} fill="none" stroke="var(--tm-blue)" strokeWidth="2" vectorEffect="non-scaling-stroke" />
      <polyline points={line(series[1])} fill="none" stroke="var(--tm-red)" strokeWidth="2" vectorEffect="non-scaling-stroke" />
    </svg>
  );
}

// `match.messages` (storm-stats) — constants.json MessageType / MessageTarget.
const MSG_CHAT = 0, MSG_PING = 1, MSG_ANNOUNCE = 5;
const TARGET: Record<number, string> = { 0: "all", 1: "allies", 4: "obs" };

/** Table BM (taunts/dances/sprays/voiceLines) + pings (depuis `match.messages`) par joueur. */
function BMTable({ players, messages, num: n }: { players: any[]; messages: any[]; num: number }) {
  const pings = (toon: string) => (messages || []).filter((x) => x.player === toon && x.type === MSG_PING).length;
  const cnt = (a: any) => (Array.isArray(a) ? a.length : 0);
  if (!players.some((p) => cnt(p.taunts) + cnt(p.dances) + cnt(p.sprays) + cnt(p.voiceLines) + pings(p.ToonHandle) > 0)) return null;
  return (
    <section className="ds-sec">
      <SecHead num={n} title="Taunts & pings" />
      <div className="ds-panel flush scroll">
        <table className="ds-table">
          <thead><tr><th>Player</th><th>Taunts</th><th>Dances</th><th>Sprays</th><th>Voice</th><th>Pings</th></tr></thead>
          <tbody>
            {players.map((p) => (
              <tr key={p.ToonHandle}>
                <td><span className="ds-who" style={{ minWidth: 0 }}><Portrait hero={p.hero} size={24} tone={p.team === 1 ? "red" : "blue"} /><span>{p.hero}</span></span></td>
                <td className="ds-num">{cnt(p.taunts)}</td><td className="ds-num">{cnt(p.dances)}</td><td className="ds-num">{cnt(p.sprays)}</td>
                <td className="ds-num">{cnt(p.voiceLines)}</td><td className="ds-num">{pings(p.ToonHandle)}</td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    </section>
  );
}

function ChatLog({ messages, players, num: n }: { messages: any; players: Record<string, any>; num: number }) {
  const [pings, setPings] = useState(false);
  const [announces, setAnnounces] = useState(false);
  const all: any[] = Array.isArray(messages) ? messages : [];
  const counts = {
    chat: all.filter((m) => m.type === MSG_CHAT).length,
    ping: all.filter((m) => m.type === MSG_PING).length,
    announce: all.filter((m) => m.type === MSG_ANNOUNCE).length,
  };
  const shown = all
    .filter((m) => m.type === MSG_CHAT || (pings && m.type === MSG_PING) || (announces && m.type === MSG_ANNOUNCE))
    .sort((a, b) => num(a.loop) - num(b.loop));
  return (
    <section className="ds-sec">
      <SecHead num={n} title="Match chat" />
      <div className="ds-panel flush">
        <div className="ds-panel-hd ds-pills">
          <span className="ds-pill on">{counts.chat} messages</span>
          <span className={pings ? "ds-pill on" : "ds-pill"} onClick={() => setPings(!pings)}>pings ({counts.ping})</span>
          <span className={announces ? "ds-pill on" : "ds-pill"} onClick={() => setAnnounces(!announces)}>callouts ({counts.announce})</span>
        </div>
        {shown.length === 0 && <div className="ds-empty-row">no message</div>}
        {shown.map((msg, i) => {
          const p = players[msg.player] ?? {};
          return (
            <div key={i} className="ds-chat-row">
              <span className="t">{fmtClock(num(msg.time))}</span>
              {p.hero ? <Portrait hero={p.hero} size={22} tone={msg.team === 1 ? "red" : "blue"} /> : <span style={{ width: 22 }} />}
              <span className={`who ${msg.team === 0 ? "ds-us" : msg.team === 1 ? "ds-them" : "muted"}`} title={msg.player}>{p.name ?? msg.player}</span>
              {msg.type === MSG_CHAT ? (
                <><span className="ds-tag award">{TARGET[msg.recipient] ?? "?"}</span><span className="msg">{msg.text}</span></>
              ) : (
                <span className="muted" style={{ fontSize: 11, paddingTop: 3 }}>{msg.type === MSG_PING ? `ping ${TARGET[msg.recipient] ?? ""}`.trim() : announceLabel(msg.announcement)}</span>
              )}
            </div>
          );
        })}
      </div>
    </section>
  );
}

export function MatchDetail() {
  const { id } = useParams();
  const [adv, setAdv] = useState(false);
  const [tab, setTab] = useState<"score" | "replay2d">("score");
  useSettings(); // operator_names (perspective)
  useDimTalents(); // talentTreeId → nom
  const attrs = useDimHeroAttributes(); // codes attribut des bans → noms de héros
  const { data, isLoading } = useQuery({ queryKey: ["match", id], queryFn: () => fetchMatch(id!) });

  if (isLoading) return <div className="ds-page"><div className="ds-empty" style={{ marginTop: 24 }}>loading…</div></div>;
  if (!data) return <div className="ds-page"><div className="ds-empty" style={{ marginTop: 24 }}>Match not found. <Link to="/matches">Back to matches</Link></div></div>;
  const m = data.match;
  const byToon: Record<string, any> = data.players ?? {};
  const players = Object.values(byToon);
  const me: any = matchOperator(players as any[]);
  const myTeam: number | null = me?.team ?? null;
  const won = myTeam != null && m.winner === myTeam;
  const bans = m.bans ?? {};
  const lvl = (t: number) => Math.max(0, ...Object.keys(m.levelTimes?.[String(t)] ?? {}).map(Number));
  const tk = (t: number) => num(m[`team${t}Takedowns`]);
  // Perspective : « Us / Them » si l'opérateur a joué, sinon « Blue / Red » ; la couleur suit
  // toujours l'équipe réelle (bleue = 0, rouge = 1), l'équipe de l'opérateur est simplement listée en premier.
  const side = (t: number | null | undefined) =>
    t == null || t < 0 ? "—" : myTeam == null ? (t === 0 ? "BLUE" : "RED") : t === myTeam ? "US" : "THEM";
  const sideCls = (t: number | null | undefined) => (t == null || t < 0 ? "" : t === 0 ? "ds-us" : "ds-them");
  const teamLabel = (t: number) => (myTeam == null ? (t === 0 ? "Blue team" : "Red team") : t === myTeam ? "Your team" : "Enemy team");
  const maxes: Record<string, number> = {};
  for (const c of COLS) maxes[c.label] = Math.max(1, ...players.map((p: any) => c.get(p.gameStats ?? {})));
  const fort = (Object.values(m.structures ?? {}) as any[]).filter((s) => s?.destroyed != null && s?.name === "Fort").sort((a, b) => a.destroyed - b.destroyed)[0];
  const marks = fort ? [{ t: fort.destroyed, label: `first fort · ${side(fort.team === 0 ? 1 : 0).toLowerCase()} ${myTeam == null ? "team takes it" : "take it"}` }] : [];
  const order = myTeam == null ? [0, 1] : [myTeam, 1 - myTeam];
  const hasDraft = !!m.picks || bans[0]?.length > 0 || bans[1]?.length > 0;
  let sec = 0;
  const next = () => ++sec;

  return (
    <div className="ds-page">
      <header className="ds-mhead" style={hasMapArt(m.map) ? mapArt(m.map) : undefined}>
        <div className="ds-kicker"><ModeTag mode={m.mode} /> {fmtTime(m.date)} · {fmtDur(m.length)} · build {m.version?.m_build}</div>
        <h1 className="ds-title" style={{ cursor: "default" }}>{m.map}</h1>
        {myTeam != null
          ? <span className={`ds-verdict ${won ? "w" : "l"}`}>{won ? "VICTORY" : "DEFEAT"}</span>
          : <span className="ds-verdict n">{m.winner === 0 ? "BLUE" : "RED"} TEAM WINS</span>}
        <div className="ds-score">
          <div><span className="ds-label">Takedowns</span><strong className={sideCls(order[0])}>{tk(order[0])}</strong><span>–</span><strong className={sideCls(order[1])}>{tk(order[1])}</strong></div>
          <div><span className="ds-label">Team level</span><strong className={sideCls(order[0])}>{lvl(order[0])}</strong><span>–</span><strong className={sideCls(order[1])}>{lvl(order[1])}</strong></div>
          <div><span className="ds-label">First objective</span><strong className={sideCls(m.firstObjective)}>{side(m.firstObjective)}</strong></div>
          <div><span className="ds-label">First fort</span><strong className={sideCls(m.firstFort)}>{side(m.firstFort)}</strong></div>
          {m.firstKeep != null && m.firstKeep >= 0 && <div><span className="ds-label">First keep</span><strong className={sideCls(m.firstKeep)}>{side(m.firstKeep)}</strong></div>}
        </div>
      </header>

      <div className="ds-tabs">
        <span className={tab === "score" ? "ds-pill on" : "ds-pill"} onClick={() => setTab("score")}>Score</span>
        <span className={tab === "replay2d" ? "ds-pill on" : "ds-pill"} onClick={() => setTab("replay2d")}>Replay 2D</span>
        <a className="ds-pill" style={{ marginLeft: "auto" }} href={`/api/matches/${data.id}/raw?stream=tracker`} target="_blank" rel="noreferrer">raw dump ›</a>
      </div>

      {tab === "replay2d" && <div style={{ marginTop: 16 }}>{id ? <Replay2D id={id} /> : <div className="ds-empty">invalid match</div>}</div>}

      {tab === "score" && (
        <>
          {hasDraft && (
            <section className="ds-sec">
              <SecHead num={next()} title="Draft" sub={`bans struck through · picks in team order${m.firstPickWin != null ? ` · first pick ${m.firstPickWin ? "won" : "lost"}` : ""}`} />
              <div className="ds-draft">
                {order.map((t) => {
                  const ps = (m.picks?.[t] ?? []) as string[];
                  const heroesInTeam = players.filter((p: any) => p.team === t);
                  const picks = ps.length ? ps : heroesInTeam.map((p: any) => p.hero);
                  return (
                    <div key={t} className={`ds-dside ${t === 0 ? "blue" : "red"}`}>
                      <div className="ds-dside-hd">
                        <span className={`ds-label ${t === 0 ? "ds-us" : "ds-them"}`} style={{ margin: 0 }}>{teamLabel(t)} · {t === 0 ? "blue" : "red"}</span>
                        {m.picks?.first === t && <span className="ds-tag fp">FIRST PICK</span>}
                      </div>
                      {(bans[t] ?? []).length > 0 && (
                        <div className="ds-dbans">
                          {(bans[t] ?? []).map((b: any, i: number) => {
                            const h = banHero(b, attrs);
                            const phase = typeof b === "object" && b ? (b.order === 2 ? "MID" : "1ST") : "BAN";
                            return h
                              ? <span key={i} className="ds-dban"><Portrait hero={h} size={40} tone="ban" /><em>{phase}</em></span>
                              : <span key={i} className="ds-dban"><span className="ds-tag award" style={{ marginTop: 12 }}>no ban</span></span>;
                          })}
                        </div>
                      )}
                      <div className="ds-dpicks">
                        {picks.map((h: string, i: number) => {
                          const p: any = heroesInTeam.find((x: any) => x.hero === h);
                          return (
                            <Link key={i} to={p ? `/player/${encodeURIComponent(p.ToonHandle)}` : "#"} className="ds-dpick">
                              <Portrait hero={h} size={52} tone={t === 0 ? "blue" : "red"} />
                              <div><strong>{h}</strong><em>{p?.name ?? ""}</em></div>
                              {p && me && p.ToonHandle === me.ToonHandle && <span className="ds-tag you">YOU</span>}
                            </Link>
                          );
                        })}
                      </div>
                    </div>
                  );
                })}
              </div>
            </section>
          )}

          <section className="ds-sec">
            <SecHead num={next()} title="Scoreboard" sub={<span className={adv ? "ds-pill on" : "ds-pill"} onClick={() => setAdv(!adv)}>{adv ? "− basic stats" : "+ advanced stats"}</span>} />
            {order.map((t) => <ScoreTable key={t} players={players} team={t} label={teamLabel(t)} adv={adv} meToon={me?.ToonHandle ?? null} max={maxes} />)}
          </section>

          {Array.isArray(m.levelAdvTimeline) && m.levelAdvTimeline.length > 1 && (
            <section className="ds-sec">
              <SecHead num={next()} title="Level advantage" sub={myTeam == null ? "above the line = blue team ahead" : "above the line = you are ahead"} />
              <div className="ds-panel"><LevelAdvantage timeline={m.levelAdvTimeline} length={m.length} side={myTeam === 1 ? 1 : 0} marks={marks} /></div>
            </section>
          )}

          {Array.isArray(m.XPBreakdown) && m.XPBreakdown.length > 1 && (
            <section className="ds-sec">
              <SecHead num={next()} title="Team XP" sub={<><span className="ds-us">blue</span> / <span className="ds-them">red</span></>} />
              <div className="ds-panel"><XPCurve data={m.XPBreakdown} /></div>
            </section>
          )}

          <MatchTimeline m={m} players={byToon} num={next()} />
          <ChatLog messages={m.messages} players={byToon} num={next()} />
          <BMTable players={players} messages={m.messages ?? []} num={next()} />

          <section className="ds-sec">
            <SecHead num={next()} title="Full data" />
            <div className="ds-panel flush">
              <div className="ds-kv"><span>Winner</span><span className={m.winner === 0 ? "ds-us" : "ds-them"}>{m.winner === 0 ? "Blue team" : "Red team"}</span></div>
              <div className="ds-kv"><span>Takedowns</span><span className="ds-num">{(m.takedowns ?? []).length}</span></div>
              <div className="ds-kv"><span>Raw decoded dump</span><a style={{ color: "var(--accent-2)" }} href={`/api/matches/${data.id}/raw?stream=tracker`} target="_blank" rel="noreferrer">tracker events ›</a></div>
            </div>
          </section>
        </>
      )}
    </div>
  );
}
