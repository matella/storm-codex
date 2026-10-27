// Post-game breakdown plein cadre (maquette docs/specs/2026-10-25-cast-mockups/postgame-breakdown.html).
// Toutes les valeurs viennent de `GameSummary` (résumé du replay) ; une valeur absente (`null` / vide)
// retire sa ligne — jamais de 0 inventé. Comparatifs dégâts/K/D/A = sommes des joueurs (ligne
// « Team total » de la fiche de match).
import type { Side } from "../api";
import type { CastSeries, CastTeam, GameRecord, GameSummary, PlayerSummary } from "../castApi";
import type { BreakdownTab } from "../castState";
import { displayName, seriesScore } from "../castState";
import { fmtClock as clock } from "../api";
import { HeroPortrait, initials } from "./portrait";

const TAB_LABEL: Record<BreakdownTab, string> = {
  hero_damage: "HERO DAMAGE",
  healing: "HEALING",
  siege_damage: "SIEGE",
  takedowns: "TAKEDOWNS",
};

/** Totaux d'équipe : 214k / 82.4k / 950. */
export function fmtStat(n: number): string {
  if (n >= 100_000) return `${Math.round(n / 1000)}k`;
  return fmtBar(n);
}

/** Valeurs par joueur (barres) : toujours une décimale au-delà de 1000 (161.6k). */
export function fmtBar(n: number): string {
  return n >= 1000 ? `${(n / 1000).toFixed(1)}k` : String(Math.round(n));
}


function Side_({ t, side, won, score }: { t: CastTeam; side: Side; won: boolean; score: number }) {
  const name = displayName(t, side);
  return (
    <div className={`pg-side ${side === "blue" ? "b" : "r"}`}>
      <div className="pg-logo">{t.logo ? <img src={t.logo} alt="" /> : initials(name)}</div>
      <div className="pg-id">
        <div className="pg-nm">{name}</div>
        <span className={`pg-tag ${won ? "w" : "l"}`}>{won ? "WIN" : "LOSS"}</span>
      </div>
      <b className="pg-sc">{score}</b>
    </div>
  );
}

function Stat({ label, v }: { label: string; v: [string | number, string | number] }) {
  return (
    <div className="pg-st"><b className="a">{v[0]}</b><span>{label}</span><b className="c">{v[1]}</b></div>
  );
}

/** Courbe d'avance d'XP de Blue (au-dessus = Blue devant), aire bleue / rouge de part et d'autre de 0. */
function XpCurve({ lead, length }: { lead: [number, number][]; length: number }) {
  const maxT = Math.max(1, length, ...lead.map(([t]) => t));
  const maxV = Math.max(1, ...lead.map(([, v]) => Math.abs(v)));
  const pts = lead.map(([t, v]) => `${((t / maxT) * 400).toFixed(1)},${(55 - (v / maxV) * 50).toFixed(1)}`);
  const line = `M0,55 L${pts.join(" L")}`;
  const area = `${line} L${((lead[lead.length - 1][0] / maxT) * 400).toFixed(1)},55 Z`;
  return (
    <svg viewBox="0 0 400 110" preserveAspectRatio="none" role="img" aria-label="Experience advantage over time">
      <defs>
        <clipPath id="pg-up"><rect x="0" y="0" width="400" height="55" /></clipPath>
        <clipPath id="pg-dn"><rect x="0" y="55" width="400" height="55" /></clipPath>
      </defs>
      <line x1="0" y1="55" x2="400" y2="55" className="pg-zero" />
      <path d={area} className="pg-area b" clipPath="url(#pg-up)" />
      <path d={area} className="pg-area r" clipPath="url(#pg-dn)" />
      <path d={line} className="pg-line" />
    </svg>
  );
}

function Bars({ players, tab }: { players: PlayerSummary[]; tab: BreakdownTab }) {
  const val = (p: PlayerSummary) => p[tab];
  const side = (s: Side) => players.filter((p) => p.side === s).sort((a, b) => val(b) - val(a));
  const [blue, red] = [side("blue"), side("red")];
  const max = Math.max(1, ...players.map(val));
  const fmt = (p: PlayerSummary) => (tab === "takedowns" ? String(val(p)) : fmtBar(val(p)));
  const w = (p: PlayerSummary) => `${Math.round((val(p) * 100) / max)}%`;
  return (
    <>
      {Array.from({ length: Math.max(blue.length, red.length) }, (_, i) => {
        const b = blue[i];
        const r = red[i];
        return (
          <div key={i} className="pg-dm">
            <div className="bl">
              {b && <><em>{fmt(b)}</em><span className="tr"><i style={{ width: w(b) }} /></span><HeroPortrait hero={b.hero} title={`${b.name} · ${b.hero}`} /></>}
            </div>
            <div className="rd">
              {r && <><HeroPortrait hero={r.hero} title={`${r.name} · ${r.hero}`} /><span className="tr"><i style={{ width: w(r) }} /></span><em>{fmt(r)}</em></>}
            </div>
          </div>
        );
      })}
    </>
  );
}

export function PostGameSlide({ s, game, summary: sm, tab }: { s: CastSeries; game: GameRecord; summary: GameSummary; tab: BreakdownTab }) {
  const [sb, sr] = seriesScore(s);
  const sum = (side: Side, f: (p: PlayerSummary) => number) => sm.players.filter((p) => p.side === side).reduce((n, p) => n + f(p), 0);
  const pair = (f: (p: PlayerSummary) => number): [number, number] => [sum("blue", f), sum("red", f)];
  const kda = (side: Side) => `${sum(side, (p) => p.kills)} / ${sum(side, (p) => p.deaths)} / ${sum(side, (p) => p.assists)}`;
  const [siegeB, siegeR] = pair((p) => p.siege_damage);
  const [heroB, heroR] = pair((p) => p.hero_damage);
  const hasBans = sm.bans[0].length + sm.bans[1].length > 0;
  return (
    <div className="pg" data-tab={tab}>
      <div className="pg-hd">
        <Side_ t={s.blue} side="blue" won={sm.winner === "blue"} score={sm.takedowns[0]} />
        <div className="pg-ctr">
          <div className="t">{clock(sm.duration_secs)}</div>
          <div className="m">GAME {game.number} · {game.map.toUpperCase()}</div>
        </div>
        <Side_ t={s.red} side="red" won={sm.winner === "red"} score={sm.takedowns[1]} />
      </div>
      <div className="pg-bar1">BEST OF {s.bo} · SERIES {sb} – {sr} · TAKEDOWNS</div>

      <div className="pg-left">
        {sm.players.length > 0 && <Stat label="K / D / A" v={[kda("blue"), kda("red")]} />}
        {sm.team_level && <Stat label="TEAM LEVEL" v={sm.team_level} />}
        {sm.structures && <Stat label="FORTS · KEEPS" v={sm.structures} />}
        {sm.merc_camps && <Stat label="MERC CAMPS" v={sm.merc_camps} />}
        {sm.objectives && <Stat label="MAP OBJECTIVES" v={sm.objectives} />}
        {sm.players.length > 0 && <Stat label="SIEGE DAMAGE" v={[fmtStat(siegeB), fmtStat(siegeR)]} />}
        {sm.players.length > 0 && <Stat label="HERO DAMAGE" v={[fmtStat(heroB), fmtStat(heroR)]} />}
        {hasBans && (
          <div className="pg-bans">
            <div>{sm.bans[0].map((h) => <HeroPortrait key={h} hero={h} className="bn" />)}</div>
            <span>BANS</span>
            <div className="c">{sm.bans[1].map((h) => <HeroPortrait key={h} hero={h} className="bn" />)}</div>
          </div>
        )}
      </div>

      <div className="pg-right">
        <div className="pg-panel">
          <div className="pg-tabs">
            {(Object.keys(TAB_LABEL) as BreakdownTab[]).map((k) => <span key={k} className={k === tab ? "on" : undefined}>{TAB_LABEL[k]}</span>)}
          </div>
          <Bars players={sm.players} tab={tab} />
        </div>
        {sm.xp_lead.length > 1 && (
          <div className="pg-panel pg-gr">
            <h4>EXPERIENCE ADVANTAGE OVER TIME</h4>
            <div className="pg-svg"><XpCurve lead={sm.xp_lead} length={sm.duration_secs} /></div>
            <div className="pg-cap">above the line = {displayName(s.blue, "blue")} ahead · 0:00 → {clock(sm.duration_secs)}</div>
          </div>
        )}
      </div>
    </div>
  );
}
