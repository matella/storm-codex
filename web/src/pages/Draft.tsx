import { useEffect, useMemo, useState } from "react";
import {
  useDraft, useDimHeroes, sideOfStep,
  draftAction, draftUndo, draftReset, draftConfig, draftTeams, draftUnavailable,
  draftScore, draftSeriesNext, draftSeriesNew,
  type DraftState, type Side, type DraftFormat, type TeamInfo,
} from "../api";
import { Portrait } from "../components/ds";

const MAPS = [
  "Alterac Pass", "Battlefield of Eternity", "Braxis Holdout", "Blackheart's Bay",
  "Cursed Hollow", "Dragon Shire", "Garden of Terror", "Hanamura Temple",
  "Infernal Shrines", "Sky Temple", "Tomb of the Spider Queen", "Towers of Doom",
  "Volskaya Foundry", "Warhead Junction",
];

const FORMATS: { v: DraftFormat; label: string }[] = [
  { v: "standard", label: "Standard" }, { v: "normal", label: "Normal (no bans)" }, { v: "fearless", label: "Fearless" },
];

const ALL = "All";

/** Console opérateur du simulateur de draft (design « dossier »). Pilote l'état serveur (REST) ;
 *  l'overlay /draft/overlay (OBS, inchangé) reflète tout en direct via WS. */
export function Draft() {
  const { data: d } = useDraft();
  const dim = useDimHeroes();
  const [role, setRole] = useState(ALL);
  const [search, setSearch] = useState("");

  if (!d) return <div className="ds-page"><div className="ds-empty" style={{ marginTop: 24 }}>loading…</div></div>;
  return <DraftInner d={d} dim={dim} role={role} setRole={setRole} search={search} setSearch={setSearch} />;
}

type Dim = ReturnType<typeof useDimHeroes>;

function DraftInner({ d, dim, role, setRole, search, setSearch }: {
  d: DraftState; dim: Dim; role: string; setRole: (s: string) => void; search: string; setSearch: (s: string) => void;
}) {
  const heroes = useMemo(() => Object.entries(dim ?? {}).map(([name, h]) => ({ name, ...h })), [dim]);
  const roles = useMemo(() => [ALL, ...Array.from(new Set(heroes.map((h) => h.role).filter(Boolean))) as string[]], [heroes]);

  const used = new Set([...d.assignments.filter(Boolean) as string[], ...d.manual_unavailable, ...d.series_bans]);
  const cur = d.steps[d.cursor];
  const curSide = cur ? sideOfStep(d, cur) : null;

  const filtered = heroes
    .filter((h) => role === ALL || h.role === role)
    .filter((h) => h.name.toLowerCase().includes(search.toLowerCase()))
    .sort((a, b) => a.name.localeCompare(b.name));

  // mode Pick (assigne à l'étape courante) vs Pre-ban (toggle dispo manuelle — compétition / partie manquée)
  const [mode, setMode] = useState<"pick" | "preban">("pick");
  const onHero = (name: string) => {
    if (mode === "preban") { draftUnavailable(name, !d.manual_unavailable.includes(name)); return; }
    if (!used.has(name) && d.cursor < d.steps.length) draftAction(name);
  };
  const overlayUrl = `${window.location.origin}/draft/overlay?skin=nexus`;
  const cfg = { format: d.format, map: d.map, first_pick: d.first_pick, blue: d.blue, red: d.red, bo: d.bo };

  return (
    <div className="draftc ds-page">
      <style>{CSS}</style>
      <header className="ds-cover dcover">
        <div className="ds-kicker">Draft simulator · operator console</div>
        <h1 className="ds-title" style={{ cursor: "default", fontSize: "clamp(40px, 6vw, 76px)" }}>
          <span className="tb">{d.blue.name || "Blue"}</span> <i>vs</i> <span className="tr">{d.red.name || "Red"}</span>
        </h1>
        <div className="ds-subtitle" style={{ cursor: "default" }}>
          {d.map} · {FORMATS.find((f) => f.v === d.format)?.label} · Bo{d.bo} · series {d.score[0]}–{d.score[1]}
        </div>
        <div className="ds-cover-foot">
          <span className="ds-label" style={{ margin: 0 }}>OBS overlay</span>
          <a className="olink" href={overlayUrl} target="_blank" rel="noreferrer">{overlayUrl} ↗</a>
          <span className="muted-mono">browser source 1920×1080, transparent · skins <code>nexus · glass · tactical · mono</code></span>
        </div>
      </header>

      <div className="bar">
        <Field label="Format">
          <select value={d.format} onChange={(e) => draftConfig({ ...cfg, format: e.target.value as DraftFormat })}>
            {FORMATS.map((f) => <option key={f.v} value={f.v}>{f.label}</option>)}
          </select>
        </Field>
        <Field label="Map">
          <select value={d.map} onChange={(e) => draftConfig({ ...cfg, map: e.target.value })}>
            {MAPS.map((m) => <option key={m}>{m}</option>)}
          </select>
        </Field>
        <Field label="First pick">
          <div className="seg">
            {(["blue", "red"] as Side[]).map((s) => (
              <button key={s} className={`${d.first_pick === s ? "on" : ""} ${s}`} onClick={() => draftConfig({ ...cfg, first_pick: s })}>
                {s === "blue" ? "Blue" : "Red"}
              </button>
            ))}
          </div>
        </Field>
        <Field label="Series">
          <select value={d.bo} onChange={(e) => draftConfig({ ...cfg, bo: Number(e.target.value) })}>
            {[1, 3, 5, 7].map((b) => <option key={b} value={b}>Bo{b}</option>)}
          </select>
        </Field>
        <Field label="Score">
          <div className="score">
            <Stepper v={d.score[0]} side="blue" on={(n) => draftScore(n, d.score[1])} />
            <span className="dash">–</span>
            <Stepper v={d.score[1]} side="red" on={(n) => draftScore(d.score[0], n)} />
          </div>
        </Field>
        <div className="spacer" />
        <button className="ds-btn" onClick={() => draftSeriesNext()}>Next game ›</button>
        <button className="ds-btn primary" onClick={() => { if (confirm("Start a new series? (clears the fearless history)")) draftSeriesNew(); }}>New series</button>
      </div>

      <div className={`phase ${curSide ?? "done"}`}>
        {curSide && cur ? <>
          <span className={`dot ${curSide}`} />
          <span className="who">{curSide === "blue" ? d.blue.name : d.red.name}</span>
          <span className={`ds-tag ${cur.action === "ban" ? "loss" : "win"}`}>{cur.action === "ban" ? "Ban" : "Pick"}</span>
          <span className="step">step {d.cursor + 1} / {d.steps.length}</span>
          <span className="prog"><span style={{ width: `${(d.cursor / Math.max(1, d.steps.length)) * 100}%` }} /></span>
        </> : <><span className="dot done" /><span className="who">Draft complete</span><span className="step">{d.steps.length} steps</span></>}
        <span className="acts">
          <button className="ds-btn" onClick={() => draftUndo()}>↶ Undo</button>
          <button className="ds-btn" onClick={() => draftReset()}>⟲ Reset</button>
        </span>
      </div>

      <div className="series">
        <div className="lab">Manual pre-bans (competition / missed game) · {d.manual_unavailable.length}
          <span> — switch the picker to <b>Pre-ban</b> and click heroes, or click a portrait here to remove it</span></div>
        <div className="pool">
          {d.manual_unavailable.length === 0 && <span className="none">none</span>}
          {d.manual_unavailable.map((h) => (
            <button key={h} className="si rm" title={`remove ${h}`} onClick={() => draftUnavailable(h, false)}>
              <Portrait hero={h} size={30} tone="ban" />
            </button>
          ))}
        </div>
      </div>

      {d.format === "fearless" && d.series_bans.length > 0 && (
        <div className="series">
          <div className="lab">Series bans — fearless (automatic, previous games) · {d.series_bans.length}</div>
          <div className="pool">{d.series_bans.map((h) => <span key={h} className="si" title={h}><Portrait hero={h} size={30} tone="ban" /></span>)}</div>
        </div>
      )}

      <div className="body">
        <TeamColumn d={d} side="blue" />
        <div className="picker">
          <div className="ptop">
            <input type="text" placeholder="Search a hero…" value={search} onChange={(e) => setSearch(e.target.value)} />
            <div className="seg" title="Pick = assign to the current step · Pre-ban = manually (un)ban">
              <button className={mode === "pick" ? "on" : ""} onClick={() => setMode("pick")}>Pick</button>
              <button className={mode === "preban" ? "on red" : ""} onClick={() => setMode("preban")}>Pre-ban</button>
            </div>
          </div>
          <div className="tabs">
            {roles.map((r) => <button key={r} className={role === r ? "on" : ""} onClick={() => setRole(r)}>{r}</button>)}
          </div>
          <div className="grid">
            {filtered.map((h) => {
              const preBanned = mode === "preban" && d.manual_unavailable.includes(h.name);
              return (
                <button key={h.name}
                  className={`hx ${used.has(h.name) ? "out" : ""} ${preBanned ? "banned" : ""}`}
                  disabled={mode === "pick" && used.has(h.name)} title={h.name} onClick={() => onHero(h.name)}
                  onContextMenu={(e) => { e.preventDefault(); draftUnavailable(h.name, !d.manual_unavailable.includes(h.name)); }}>
                  <Portrait hero={h.name} size={46} tone={preBanned ? "ban" : "plain"} />
                  <span className="nm">{h.name}</span>
                </button>
              );
            })}
          </div>
          <p className="hint"><b>Pick</b> mode: click = assign to the current step. <b>Pre-ban</b> mode: click = manually (un)ban. Right-click = quick toggle in both modes.</p>
        </div>
        <TeamColumn d={d} side="red" />
      </div>
    </div>
  );
}

function TeamColumn({ d, side }: { d: DraftState; side: Side }) {
  const team = side === "blue" ? d.blue : d.red;
  const [name, setName] = useState(team.name);
  const [players, setPlayers] = useState<string[]>(team.players);
  useEffect(() => { setName(team.name); setPlayers(team.players); }, [team.name, team.players]);

  const push = (n: string, pl: string[]) => {
    const info: TeamInfo = { name: n, players: pl };
    side === "blue" ? draftTeams(info, d.red) : draftTeams(d.blue, info);
  };

  const idx = d.steps.map((s, i) => ({ s, i })).filter(({ s }) => sideOfStep(d, s) === side);
  const bans = idx.filter(({ s }) => s.action === "ban");
  const picks = idx.filter(({ s }) => s.action === "pick");

  return (
    <div className={`team ${side}`}>
      <div className="ds-label">{side === "blue" ? "Blue side" : "Red side"}</div>
      <input className="tn" value={name} placeholder="Team name" title="Team name (editable)"
        onChange={(e) => setName(e.target.value)} onBlur={() => push(name, players)} />
      {bans.length > 0 && (
        <div className="bans">
          {bans.map(({ i }, k) => {
            const hero = d.assignments[i];
            return <span key={k} className={`bn ${hero ? "filled" : ""} ${i === d.cursor ? "cur" : ""}`} title={hero ?? undefined}>{hero ? <Portrait hero={hero} size={36} tone="ban" /> : "ban"}</span>;
          })}
        </div>
      )}
      {picks.map(({ i }, k) => {
        const hero = d.assignments[i];
        return (
          <div key={k} className={`slot ${i === d.cursor ? "cur" : ""}`}>
            {hero ? <Portrait hero={hero} size={46} tone={side} /> : <span className="av none" />}
            <div className="info">
              <div className={`hero ${hero ? "" : "none"}`}>{hero ?? (i === d.cursor ? "picking…" : "—")}</div>
              <input className="pl" value={players[k] ?? ""} placeholder={`player ${k + 1}`}
                onChange={(e) => { const p = [...players]; p[k] = e.target.value; setPlayers(p); }}
                onBlur={() => push(name, players)} />
            </div>
          </div>
        );
      })}
    </div>
  );
}

function Field({ label, children }: { label: string; children: React.ReactNode }) {
  return <div className="fld"><label>{label}</label>{children}</div>;
}
function Stepper({ v, side, on }: { v: number; side: Side; on: (n: number) => void }) {
  return (
    <div className="score">
      <span className={`v ${side}`}>{v}</span>
      <div className="stp">
        <button onClick={() => on(v + 1)} aria-label="increase">▲</button>
        <button onClick={() => on(Math.max(0, v - 1))} aria-label="decrease">▼</button>
      </div>
    </div>
  );
}

// Styles propres à la console, aux tokens du design system (theme.css / ds.css).
const CSS = `
.draftc .dcover{margin-top:18px;background:radial-gradient(700px 320px at 0% 0%,rgba(133,183,235,.16),transparent 60%),radial-gradient(700px 320px at 100% 100%,rgba(240,149,149,.14),transparent 60%),var(--panel)}
.draftc .dcover .ds-title i{font-style:normal;color:var(--muted-2);font-size:.45em;vertical-align:middle;margin:0 8px}
.draftc .dcover .tb{color:var(--tm-blue)} .draftc .dcover .tr{color:var(--tm-red)}
.draftc .olink{font:600 12px var(--mono);color:var(--accent-2);text-decoration:none;word-break:break-all}
.draftc .muted-mono{font:11px var(--mono);color:var(--muted-2)} .draftc .muted-mono code{color:var(--text-2)}
.draftc .bar{display:flex;flex-wrap:wrap;gap:12px 16px;align-items:flex-end;background:var(--panel);border:1px solid var(--line-2);border-radius:14px;padding:14px 16px;margin:16px 0 12px}
.draftc .fld{display:flex;flex-direction:column;gap:5px}
.draftc .fld>label{font:600 10px var(--mono);letter-spacing:.14em;text-transform:uppercase;color:var(--kicker)}
.draftc select,.draftc input[type=text]{background:var(--panel-2);border:1px solid var(--line-2);border-radius:8px;padding:7px 10px;color:var(--text);font:500 13px var(--body)}
.draftc select:focus,.draftc input[type=text]:focus{outline:none;border-color:var(--accent)}
.draftc .seg{display:flex;border:1px solid var(--line-2);border-radius:8px;overflow:hidden}
.draftc .seg button{background:var(--panel-2);border:none;padding:7px 13px;color:var(--muted-2);cursor:pointer;font:600 11px var(--mono);letter-spacing:.06em}
.draftc .seg button:hover{color:var(--text)}
.draftc .seg button.on{background:var(--accent);color:#0b0c12}
.draftc .seg button.on.blue{background:var(--tm-blue)} .draftc .seg button.on.red{background:var(--tm-red)}
.draftc .spacer{flex:1}
.draftc .score{display:flex;align-items:center;gap:6px}
.draftc .score .dash{color:var(--muted-2);font:800 18px var(--display)}
.draftc .score .v{width:38px;height:36px;display:flex;align-items:center;justify-content:center;background:var(--panel-2);border:1px solid var(--line-2);border-radius:8px;font:900 24px var(--display)}
.draftc .score .v.blue{color:var(--tm-blue)} .draftc .score .v.red{color:var(--tm-red)}
.draftc .stp button{background:var(--panel-2);border:1px solid var(--line-2);color:var(--muted-2);width:22px;height:17px;line-height:1;font-size:9px;cursor:pointer;border-radius:5px;display:block}
.draftc .stp button:hover{border-color:var(--accent);color:var(--text)}
.draftc .phase{display:flex;align-items:center;gap:12px;margin-bottom:12px;padding:12px 16px;border-radius:12px;background:var(--panel);border:1px solid var(--line-2)}
.draftc .phase.blue{box-shadow:inset 3px 0 0 var(--tm-blue)} .draftc .phase.red{box-shadow:inset 3px 0 0 var(--tm-red)}
.draftc .phase .who{font:800 22px var(--display);text-transform:uppercase;letter-spacing:.03em}
.draftc .phase .dot{width:12px;height:12px;border-radius:50%;flex-shrink:0}
.draftc .phase .dot.blue{background:var(--tm-blue);box-shadow:0 0 14px var(--tm-blue)} .draftc .phase .dot.red{background:var(--tm-red);box-shadow:0 0 14px var(--tm-red)}
.draftc .phase .dot.done{background:var(--win)}
.draftc .phase .step{font:12px var(--mono);color:var(--muted-2)}
.draftc .phase .prog{margin-left:auto;width:160px;height:4px;border-radius:2px;background:var(--line);overflow:hidden}
.draftc .phase .acts{display:flex;gap:8px;margin-left:auto}.draftc .phase .prog+.acts{margin-left:14px}
.draftc .phase .prog span{display:block;height:100%;background:var(--gold);transition:width .3s}
.draftc .series{background:var(--panel);border:1px solid var(--line);border-radius:12px;padding:10px 14px;margin-bottom:12px}
.draftc .series .lab{font:600 10px var(--mono);letter-spacing:.12em;text-transform:uppercase;color:var(--kicker);margin-bottom:8px}
.draftc .series .lab span{text-transform:none;letter-spacing:.02em;color:var(--muted-2);font-weight:400}
.draftc .series .lab b{color:var(--text-2)}
.draftc .series .pool{display:flex;flex-wrap:wrap;gap:6px;min-height:30px;align-items:center}
.draftc .series .none{font:12px var(--mono);color:var(--kicker)}
.draftc .si{display:inline-flex;line-height:0}
.draftc .si.rm{cursor:pointer;background:none;border:none;border-radius:50%;padding:0}
.draftc .si.rm:hover{transform:scale(1.1)}
.draftc .body{display:grid;grid-template-columns:270px 1fr 270px;gap:14px;align-items:start}
.draftc .team{background:var(--panel);border:1px solid var(--line-2);border-radius:16px;padding:14px}
.draftc .team.blue{box-shadow:inset 0 3px 0 var(--tm-blue)} .draftc .team.red{box-shadow:inset 0 3px 0 var(--tm-red)}
.draftc .team .tn{width:100%;font:800 22px var(--display);text-transform:uppercase;letter-spacing:.03em;margin:2px 0 12px;background:var(--panel-2);border:1px solid var(--line-2);border-radius:8px;padding:6px 10px}
.draftc .team.blue .tn{color:var(--tm-blue)} .draftc .team.red .tn{color:var(--tm-red)}
.draftc .bans{display:flex;gap:8px;margin-bottom:12px}
.draftc .bn{width:36px;height:36px;border-radius:50%;background:var(--panel-2);border:1px dashed var(--line-2);display:flex;align-items:center;justify-content:center;font:600 8px var(--mono);letter-spacing:.1em;text-transform:uppercase;color:var(--kicker)}
.draftc .bn.filled{border:none;background:none}
.draftc .bn.cur{border:1px solid var(--gold);box-shadow:0 0 0 2px var(--gold),0 0 16px rgba(250,199,117,.45)}
.draftc .slot{display:flex;align-items:center;gap:10px;padding:7px;border-radius:12px;margin-bottom:6px;background:var(--panel-2);border:1px solid var(--line)}
.draftc .slot.cur{border-color:var(--gold);box-shadow:0 0 0 1px var(--gold),0 0 18px rgba(250,199,117,.22)}
.draftc .slot .av.none{width:46px;height:46px;border-radius:50%;background:var(--panel);border:1px dashed var(--line-2);display:block;flex-shrink:0}
.draftc .slot .info{min-width:0;flex:1}
.draftc .slot .hero{font:800 17px/1.1 var(--display);text-transform:uppercase;letter-spacing:.02em;white-space:nowrap;overflow:hidden;text-overflow:ellipsis}
.draftc .slot .hero.none{color:var(--kicker);font:500 12px var(--mono);text-transform:none}
.draftc .slot.cur .hero.none{color:var(--gold)}
.draftc .slot .pl{width:100%;background:transparent;border:none;border-bottom:1px solid var(--line);border-radius:0;padding:2px 0;font:12px var(--body);color:var(--text-2)}
.draftc .slot .pl:focus{outline:none;border-bottom-color:var(--accent)}
.draftc .picker{background:var(--panel);border:1px solid var(--line-2);border-radius:16px;padding:14px}
.draftc .ptop{display:flex;gap:8px;margin-bottom:10px}
.draftc .ptop>input{flex:1}
.draftc .tabs{display:flex;flex-wrap:wrap;gap:6px;margin-bottom:12px}
.draftc .tabs button{background:var(--panel-2);border:1px solid var(--line-2);border-radius:20px;padding:5px 12px;color:var(--muted-2);cursor:pointer;font:600 11px var(--mono)}
.draftc .tabs button:hover{color:var(--text);border-color:var(--accent)}
.draftc .tabs button.on{background:var(--accent);border-color:var(--accent);color:#0b0c12}
.draftc .grid{display:grid;grid-template-columns:repeat(auto-fill,minmax(84px,1fr));gap:8px}
.draftc .hx{display:flex;flex-direction:column;align-items:center;gap:6px;padding:10px 3px 8px;border-radius:12px;background:var(--panel-2);border:1px solid var(--line);color:var(--text-2);cursor:pointer;transition:border-color .15s,transform .15s}
.draftc .hx:hover:not(:disabled){border-color:var(--accent);transform:translateY(-1px);color:var(--text)}
.draftc .hx .nm{font:800 12px var(--display);text-transform:uppercase;letter-spacing:.02em;white-space:nowrap;overflow:hidden;text-overflow:ellipsis;max-width:100%}
.draftc .hx.out{opacity:.3;cursor:not-allowed;filter:grayscale(.9)}
.draftc .hx.banned{opacity:1;cursor:pointer;filter:none;border-color:var(--loss);box-shadow:inset 0 0 0 1px var(--loss)}
.draftc .hint{font:11px var(--mono);color:var(--kicker);margin:12px 0 0}
.draftc .hint b{color:var(--text-2)}
@media (max-width:1000px){.draftc .body{grid-template-columns:1fr}.draftc .phase .prog{display:none}}
`;
