import { useEffect, useMemo, useState } from "react";
import {
  useDraft, useDimHeroes, sideOfStep,
  draftAction, draftUndo, draftReset, draftConfig, draftTeams, draftUnavailable,
  draftScore, draftSeriesNext, draftSeriesNew,
  type DraftState, type Side, type DraftFormat, type TeamInfo,
} from "../api";
import { Avatar } from "../components/Avatar";

const MAPS = [
  "Alterac Pass", "Battlefield of Eternity", "Braxis Holdout", "Blackheart's Bay",
  "Cursed Hollow", "Dragon Shire", "Garden of Terror", "Hanamura Temple",
  "Infernal Shrines", "Sky Temple", "Tomb of the Spider Queen", "Towers of Doom",
  "Volskaya Foundry", "Warhead Junction",
];

const FORMATS: { v: DraftFormat; label: string }[] = [
  { v: "standard", label: "Standard" }, { v: "normal", label: "Normal (sans ban)" }, { v: "fearless", label: "Fearless" },
];

/** Console opérateur du simulateur de draft. Pilote l'état serveur (REST) ; l'overlay /draft/overlay
 *  reflète tout en direct via WS. Cf. docs/draft-control-mockup.html. */
export function Draft() {
  const { data: d } = useDraft();
  const dim = useDimHeroes();
  const [role, setRole] = useState("Tous");
  const [search, setSearch] = useState("");

  if (!d) return <div className="empty">loading…</div>;
  return <DraftInner d={d} dim={dim} role={role} setRole={setRole} search={search} setSearch={setSearch} />;
}

type Dim = ReturnType<typeof useDimHeroes>;

function DraftInner({ d, dim, role, setRole, search, setSearch }: {
  d: DraftState; dim: Dim; role: string; setRole: (s: string) => void; search: string; setSearch: (s: string) => void;
}) {
  const heroes = useMemo(() => Object.entries(dim ?? {}).map(([name, h]) => ({ name, ...h })), [dim]);
  const roles = useMemo(() => ["Tous", ...Array.from(new Set(heroes.map((h) => h.role).filter(Boolean))) as string[]], [heroes]);

  const used = new Set([...d.assignments.filter(Boolean) as string[], ...d.manual_unavailable, ...d.series_bans]);
  const cur = d.steps[d.cursor];
  const curSide = cur ? sideOfStep(d, cur) : null;

  const filtered = heroes
    .filter((h) => role === "Tous" || h.role === role)
    .filter((h) => h.name.toLowerCase().includes(search.toLowerCase()))
    .sort((a, b) => a.name.localeCompare(b.name));

  // mode Pick (assigne à l'étape courante) vs Préban (toggle dispo manuelle — compétition / game manquée)
  const [mode, setMode] = useState<"pick" | "preban">("pick");
  const onHero = (name: string) => {
    if (mode === "preban") { draftUnavailable(name, !d.manual_unavailable.includes(name)); return; }
    if (!used.has(name) && d.cursor < d.steps.length) draftAction(name);
  };
  const overlayUrl = `${window.location.origin}/draft/overlay?skin=nexus`;

  return (
    <div className="draftc">
      <style>{CSS}</style>
      <div className="bar">
        <Field label="Format">
          <select value={d.format} onChange={(e) => draftConfig({ format: e.target.value as DraftFormat, map: d.map, first_pick: d.first_pick, blue: d.blue, red: d.red, bo: d.bo })}>
            {FORMATS.map((f) => <option key={f.v} value={f.v}>{f.label}</option>)}
          </select>
        </Field>
        <Field label="Map">
          <select value={d.map} onChange={(e) => draftConfig({ format: d.format, map: e.target.value, first_pick: d.first_pick, blue: d.blue, red: d.red, bo: d.bo })}>
            {MAPS.map((m) => <option key={m}>{m}</option>)}
          </select>
        </Field>
        <Field label="First pick">
          <div className="seg">
            {(["blue", "red"] as Side[]).map((s) => (
              <button key={s} className={`${d.first_pick === s ? "on" : ""} ${s}`}
                onClick={() => draftConfig({ format: d.format, map: d.map, first_pick: s, blue: d.blue, red: d.red, bo: d.bo })}>
                {s === "blue" ? "Blue" : "Red"}
              </button>
            ))}
          </div>
        </Field>
        <Field label="Série">
          <select value={d.bo} onChange={(e) => draftConfig({ format: d.format, map: d.map, first_pick: d.first_pick, blue: d.blue, red: d.red, bo: Number(e.target.value) })}>
            {[1, 3, 5, 7].map((b) => <option key={b} value={b}>Bo{b}</option>)}
          </select>
        </Field>
        <Field label="Score">
          <div className="score">
            <Stepper v={d.score[0]} on={(n) => draftScore(n, d.score[1])} />
            <span className="dim">—</span>
            <Stepper v={d.score[1]} on={(n) => draftScore(d.score[0], n)} />
          </div>
        </Field>
        <div className="spacer" />
        <button className="btn" onClick={() => draftUndo()}>↶ Undo</button>
        <button className="btn" onClick={() => draftReset()}>⟲ Reset</button>
        <button className="btn" onClick={() => draftSeriesNext()}>Partie suivante</button>
        <button className="btn primary" onClick={() => { if (confirm("Nouvelle série ? (vide l'historique fearless)")) draftSeriesNew(); }}>Nouvelle série</button>
      </div>

      <div className="olink">
        <b>Overlay OBS</b> (browser source 1920×1080, fond transparent) :
        <a href={overlayUrl} target="_blank" rel="noreferrer"> {overlayUrl} </a>
        · skins : <code>nexus · glass · tactical · mono</code>
      </div>

      <div className="phase">
        {curSide ? <>
          <span className={`dot ${curSide}`} />
          <span><b>Au tour de {curSide === "blue" ? d.blue.name : d.red.name}</b>
            <span className="step"> — {cur.action === "ban" ? "Ban" : "Pick"} ({d.cursor + 1}/{d.steps.length})</span></span>
        </> : <b>Draft terminé</b>}
      </div>

      <div className="series">
        <div className="lab">Prébans manuels (compétition / partie manquée) · {d.manual_unavailable.length}
          &nbsp;— passe le picker en mode <b>Préban</b> et clique des héros, ou clique une vignette ici pour la retirer</div>
        <div className="pool">
          {d.manual_unavailable.length === 0 && <span className="dim" style={{ fontSize: 12 }}>aucun</span>}
          {d.manual_unavailable.map((h) => (
            <button key={h} className="si rm" title={`retirer ${h}`} onClick={() => draftUnavailable(h, false)}>
              <Avatar hero={h} size={26} />
            </button>
          ))}
        </div>
      </div>

      {d.format === "fearless" && d.series_bans.length > 0 && (
        <div className="series">
          <div className="lab">Series bans — fearless (auto, parties précédentes) · {d.series_bans.length}</div>
          <div className="pool">{d.series_bans.map((h) => <span key={h} className="si"><Avatar hero={h} size={26} /></span>)}</div>
        </div>
      )}

      <div className="body">
        <TeamColumn d={d} side="blue" />
        <div className="picker">
          <div className="ptop">
            <input type="text" placeholder="Rechercher un héros…" value={search} onChange={(e) => setSearch(e.target.value)} />
            <div className="seg" title="Pick = assigner à l'étape · Préban = (dé)bannir manuellement">
              <button className={mode === "pick" ? "on" : ""} onClick={() => setMode("pick")}>Pick</button>
              <button className={mode === "preban" ? "on red" : ""} onClick={() => setMode("preban")}>Préban</button>
            </div>
          </div>
          <div className="tabs">
            {roles.map((r) => <button key={r} className={role === r ? "on" : ""} onClick={() => setRole(r)}>{r}</button>)}
          </div>
          <div className="grid">
            {filtered.map((h) => (
              <button key={h.name}
                className={`hx ${used.has(h.name) ? "out" : ""} ${mode === "preban" && d.manual_unavailable.includes(h.name) ? "banned" : ""}`}
                disabled={mode === "pick" && used.has(h.name)} title={h.name} onClick={() => onHero(h.name)}
                onContextMenu={(e) => { e.preventDefault(); draftUnavailable(h.name, !d.manual_unavailable.includes(h.name)); }}>
                <Avatar hero={h.name} size={40} />
                <span className="nm">{h.name}</span>
              </button>
            ))}
          </div>
          <p className="hint dim">Mode <b>Pick</b> : clic = assigner à l'étape courante. Mode <b>Préban</b> : clic = (dé)bannir manuellement. (clic droit = toggle rapide dans les deux modes)</p>
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
      <input className="tn" value={name} placeholder="Nom de l'équipe" title="Nom d'équipe (éditable)"
        onChange={(e) => setName(e.target.value)} onBlur={() => push(name, players)} />
      <div className="bans">
        {bans.map(({ i }, k) => {
          const hero = d.assignments[i];
          return <span key={k} className={`bn ${hero ? "filled" : ""} ${i === d.cursor ? "cur" : ""}`}>{hero ? <Avatar hero={hero} size={32} /> : "ban"}</span>;
        })}
      </div>
      {picks.map(({ i }, k) => {
        const hero = d.assignments[i];
        return (
          <div key={k} className={`slot ${i === d.cursor ? "cur" : ""}`}>
            {hero ? <Avatar hero={hero} size={42} /> : <span className="av none" />}
            <div className="info">
              <div className={`hero ${hero ? "" : "none"}`}>{hero ?? (i === d.cursor ? "à choisir…" : "—")}</div>
              <input className="pl" value={players[k] ?? ""} placeholder={`joueur ${k + 1}`}
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
function Stepper({ v, on }: { v: number; on: (n: number) => void }) {
  return (
    <div className="score">
      <span className="v">{v}</span>
      <div className="stp">
        <button onClick={() => on(v + 1)}>▲</button>
        <button onClick={() => on(Math.max(0, v - 1))}>▼</button>
      </div>
    </div>
  );
}

const CSS = `
.draftc .bar{display:flex;flex-wrap:wrap;gap:10px 14px;align-items:flex-end;background:var(--surface,#12151d);border:1px solid var(--hairline,#222838);border-radius:12px;padding:12px 14px;margin-bottom:12px}
.draftc .fld{display:flex;flex-direction:column;gap:4px}
.draftc .fld>label{font-size:10.5px;letter-spacing:.1em;text-transform:uppercase;color:#7c8398}
.draftc select,.draftc input[type=text]{background:#10131c;border:1px solid #2a3142;border-radius:8px;padding:7px 10px;color:#e7ebf5}
.draftc .seg{display:flex;border:1px solid #2a3142;border-radius:8px;overflow:hidden}
.draftc .seg button{background:#10131c;border:none;padding:7px 13px;color:#9aa3bd;cursor:pointer}
.draftc .seg button.on{background:#2f6df6;color:#fff}.draftc .seg button.on.red{background:#e0414b}
.draftc .spacer{flex:1}
.draftc .score{display:flex;align-items:center;gap:6px}
.draftc .score .v{width:32px;height:32px;display:flex;align-items:center;justify-content:center;background:#10131c;border:1px solid #2a3142;border-radius:8px;font-weight:800}
.draftc .stp button{background:#10131c;border:1px solid #2a3142;color:#9aa3bd;width:22px;height:16px;line-height:1;cursor:pointer;border-radius:5px;display:block}
.draftc .btn{background:#1b2030;border:1px solid #2f3850;border-radius:8px;padding:8px 13px;color:#cdd4e6;cursor:pointer;font-weight:600}
.draftc .btn.primary{background:#2f6df6;border-color:#2f6df6;color:#fff}
.draftc .phase{display:flex;align-items:center;gap:12px;margin-bottom:12px;padding:10px 14px;border-radius:10px;background:#12151d;border:1px solid #284089}
.draftc .phase .dot{width:10px;height:10px;border-radius:50%}.draftc .phase .dot.blue{background:#2f6df6}.draftc .phase .dot.red{background:#e0414b}
.draftc .phase .step{color:#9aa3bd;font-size:12.5px}
.draftc .series{background:#10131c;border:1px solid #222838;border-radius:10px;padding:8px 12px;margin-bottom:12px}
.draftc .series .lab{font-size:10px;letter-spacing:.12em;text-transform:uppercase;color:#7c8398;margin-bottom:6px}
.draftc .series .pool{display:flex;flex-wrap:wrap;gap:4px}.draftc .series .si{filter:grayscale(.6) brightness(.8)}
.draftc .body{display:grid;grid-template-columns:260px 1fr 260px;gap:14px;align-items:start}
.draftc .team{background:#12151d;border:1px solid #222838;border-radius:12px;padding:12px}
.draftc .team.blue{border-top:3px solid #2f6df6}.draftc .team.red{border-top:3px solid #e0414b}
.draftc .team .tn{width:100%;font-weight:700;font-size:15px;margin-bottom:8px;background:#10131c;border:1px solid #2a3142;border-radius:8px;padding:7px 10px}
.draftc .bans{display:flex;gap:6px;margin-bottom:10px}
.draftc .bn{width:34px;height:34px;border-radius:7px;background:#0c0e13;border:1px solid #2a3142;display:flex;align-items:center;justify-content:center;font-size:9px;color:#5a6275}
.draftc .bn.cur{border-color:#e8c66a;box-shadow:0 0 0 1px #e8c66a}
.draftc .slot{display:flex;align-items:center;gap:9px;padding:6px;border-radius:9px;margin-bottom:6px;background:#0f121a;border:1px solid #1e2433}
.draftc .team.blue .slot.cur{border-color:#2f6df6;box-shadow:0 0 0 1px #2f6df6}
.draftc .team.red .slot.cur{border-color:#e0414b;box-shadow:0 0 0 1px #e0414b}
.draftc .slot .av.none{width:42px;height:42px;border-radius:8px;background:#0c0e13;border:1px dashed #2a3142;display:block}
.draftc .slot .info{min-width:0;flex:1}
.draftc .slot .hero{font-weight:700;font-size:13px;white-space:nowrap;overflow:hidden;text-overflow:ellipsis}
.draftc .slot .hero.none{color:#69728c;font-style:italic;font-weight:500}
.draftc .slot .pl{width:100%;background:transparent;border:none;border-bottom:1px solid #232a3a;border-radius:0;padding:2px 0;font-size:11.5px;color:#aab2ca}
.draftc .picker{background:#12151d;border:1px solid #222838;border-radius:12px;padding:12px}
.draftc .ptop{display:flex;gap:8px;margin-bottom:10px}
.draftc .ptop>input{flex:1}
.draftc .olink{font-size:12.5px;color:#9aa3bd;margin:-2px 0 12px;padding:0 2px}
.draftc .olink a{color:#7e97ff} .draftc .olink code{color:#cdd4e6}
.draftc .si.rm{cursor:pointer;background:none;border:1px solid #2a3142;border-radius:6px;padding:0;line-height:0;display:inline-flex}
.draftc .si.rm:hover{border-color:#e0414b}
.draftc .tabs{display:flex;flex-wrap:wrap;gap:6px;margin-bottom:12px}
.draftc .tabs button{background:#10131c;border:1px solid #2a3142;border-radius:999px;padding:6px 12px;color:#9aa3bd;cursor:pointer;font-size:12.5px}
.draftc .tabs button.on{background:#7e97ff;border-color:#7e97ff;color:#0c0e13;font-weight:700}
.draftc .grid{display:grid;grid-template-columns:repeat(auto-fill,minmax(76px,1fr));gap:8px}
.draftc .hx{display:flex;flex-direction:column;align-items:center;gap:4px;padding:7px 3px;border-radius:9px;background:#0f121a;border:1px solid #1e2433;color:#cdd4e6;cursor:pointer}
.draftc .hx:hover:not(:disabled){border-color:#7e97ff}
.draftc .hx .nm{font-size:10px;font-weight:600;white-space:nowrap;overflow:hidden;text-overflow:ellipsis;max-width:100%}
.draftc .hx.out{opacity:.35;cursor:not-allowed;filter:grayscale(.8)}
.draftc .hx.banned{opacity:1;cursor:pointer;filter:none;outline:2px solid #e0414b;outline-offset:-2px}
.draftc .hint{font-size:11px;margin:10px 0 0}
`;
