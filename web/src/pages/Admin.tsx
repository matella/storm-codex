import { useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";

// Le token admin est saisi par l'opérateur et conservé en localStorage (UI LAN/Tailscale).
function useAdminToken(): [string, (v: string) => void] {
  const [t, setT] = useState(() => localStorage.getItem("admin_token") ?? "");
  return [t, (v: string) => { localStorage.setItem("admin_token", v); setT(v); }];
}

async function adminFetch(url: string, token: string, opts: RequestInit = {}) {
  const r = await fetch(url, {
    ...opts,
    headers: { ...(opts.headers ?? {}), Authorization: `Bearer ${token}`, "Content-Type": "application/json" },
  });
  return r;
}

export function Admin() {
  const [token, setToken] = useAdminToken();
  const qc = useQueryClient();
  const { data: settings } = useQuery({ queryKey: ["settings"], queryFn: async () => (await fetch("/api/settings")).json() });
  // mode ouvert : aucun ADMIN_TOKEN configuré côté serveur (auto-hébergement local) → pas d'auth.
  const adminOpen: boolean = (settings as { admin_open?: boolean } | undefined)?.admin_open === true;
  const { data: health } = useQuery({
    queryKey: ["admin-uploads", token, adminOpen],
    queryFn: async () => (await adminFetch("/api/admin/uploads", token)).json(),
    enabled: !!token || adminOpen,
  });
  const { data: teams } = useQuery({ queryKey: ["teams"], queryFn: async () => (await fetch("/api/teams")).json() });
  const { data: collections } = useQuery({ queryKey: ["collections"], queryFn: async () => (await fetch("/api/collections")).json() });

  const [tokenName, setTokenName] = useState("");
  const [newToken, setNewToken] = useState<string | null>(null);
  const [opNames, setOpNames] = useState<string | null>(null);
  const [opMsg, setOpMsg] = useState<string | null>(null);
  // valeur éditable : la saisie locale si touchée, sinon les réglages chargés
  const opValue: string =
    opNames ?? ((settings?.operator_names as string[] | undefined) ?? []).join(", ");
  const saveOperator = async () => {
    // l'échec le plus courant : pas de token admin (stocké par navigateur) → on le dit clairement
    // plutôt que d'échouer en silence.
    if (!token && !adminOpen) { setOpMsg("⚠ enter the admin token above first"); return; }
    const names = opValue.split(",").map((s) => s.trim()).filter(Boolean);
    try {
      const r = await adminFetch("/api/admin/settings", token, { method: "PUT", body: JSON.stringify({ operator_names: names }) });
      if (r.ok) { setOpMsg("✓ saved"); qc.invalidateQueries({ queryKey: ["settings"] }); }
      else if (r.status === 401 || r.status === 403) setOpMsg("✗ unauthorized — check the admin token");
      else setOpMsg(`✗ save failed (HTTP ${r.status})`);
    } catch { setOpMsg("✗ network error — server unreachable"); }
    setTimeout(() => setOpMsg(null), 5000);
  };
  const [teamName, setTeamName] = useState("");
  const [teamLeague, setTeamLeague] = useState("");
  const [collName, setCollName] = useState("");

  const createToken = async () => {
    const r = await adminFetch("/api/admin/tokens", token, { method: "POST", body: JSON.stringify({ name: tokenName }) });
    if (r.ok) { setNewToken((await r.json()).token); setTokenName(""); }
  };
  const createTeam = async () => {
    await adminFetch("/api/teams", token, { method: "POST", body: JSON.stringify({ name: teamName, roster: [], league: teamLeague || null }) });
    setTeamName(""); setTeamLeague(""); qc.invalidateQueries({ queryKey: ["teams"] });
  };
  const setLeague = async (id: number, league: string) => {
    await adminFetch(`/api/teams/${id}`, token, { method: "PUT", body: JSON.stringify({ league: league || null }) });
    qc.invalidateQueries({ queryKey: ["teams"] });
  };
  const createColl = async () => {
    await adminFetch("/api/collections", token, { method: "POST", body: JSON.stringify({ name: collName, match_ids: [] }) });
    setCollName(""); qc.invalidateQueries({ queryKey: ["collections"] });
  };
  const reprocess = async () => { await adminFetch("/api/admin/reprocess", token, { method: "POST" }); };

  const byStatus = Object.entries((health?.by_status ?? {}) as Record<string, number>);
  const byClass = Object.entries((health?.by_error_class ?? {}) as Record<string, number>);
  const del = async (url: string, key: string) => { await adminFetch(url, token, { method: "DELETE" }); qc.invalidateQueries({ queryKey: [key] }); };

  return (
    <div className="ds-page admin">
      <style>{CSS}</style>
      <header className="ds-cover" style={{ marginTop: 18 }}>
        <div className="ds-kicker">Operator · settings</div>
        <h1 className="ds-title" style={{ cursor: "default", fontSize: "clamp(44px, 7vw, 88px)" }}>Admin</h1>
        <div className="ds-subtitle" style={{ cursor: "default" }}>Identity, upload pipeline, tokens, teams and collections.</div>
        <div className="ds-cover-foot">
          {adminOpen ? (
            <span className="mode open">🔓 Local mode — no admin token required (server has no ADMIN_TOKEN set). Set ADMIN_TOKEN in the server env to re-enable auth (recommended if exposed publicly).</span>
          ) : (
            <>
              <span className="ds-label" style={{ margin: 0 }}>Admin token</span>
              <input className="ds-input" style={{ flex: 1, minWidth: 220 }} type="password" placeholder="ADMIN_TOKEN" value={token} onChange={(e) => setToken(e.target.value)} />
              <span className={token ? "ds-tag win" : "ds-tag loss"}>{token ? "configured" : "required for actions"}</span>
            </>
          )}
        </div>
      </header>

      <Sec num="01" title="My identity" note="operator perspective">
        <div className="ds-panel">
          <div className="arow">
            <input className="ds-input" style={{ flex: 1 }} placeholder="my in-game names, comma-separated (e.g. matella, MatellaSmurf)"
              value={opValue} onChange={(e) => setOpNames(e.target.value)} />
            <button className="ds-btn primary" onClick={saveOperator}>Save</button>
          </div>
          {opMsg && <div className="amsg" style={{ color: opMsg.startsWith("✓") ? "var(--win)" : "var(--loss-soft)" }}>{opMsg}</div>}
          <p className="ahint">Multiple accounts? Comma-separate them (e.g. matella, матella). Cyrillic / any UTF-8 is fine.
            Used everywhere (session, matches, widget) and by the Jarvis brief.</p>
        </div>
      </Sec>

      {health && (
        <Sec num="02" title="Upload health" note={<>parser <b>{health.parser_version}</b></>}>
          <div className="ds-panel">
            <div className="ds-label">By status</div>
            <div className="tiles">
              {byStatus.length === 0 && <span className="none">no uploads yet</span>}
              {byStatus.map(([k, n]) => <div key={k} className={`tile ${k}`}><b>{n}</b><span>{k}</span></div>)}
            </div>
            <div className="ds-label" style={{ marginTop: 16 }}>Failures by class</div>
            <div className="tiles">
              {byClass.length === 0 && <span className="none">no failures</span>}
              {byClass.map(([k, n]) => <div key={k} className="tile failed"><b>{n}</b><span>{k}</span></div>)}
            </div>
            <div className="arow" style={{ marginTop: 16 }}>
              <button className="ds-btn" onClick={reprocess}>Re-process stale parser_version ›</button>
            </div>
          </div>
        </Sec>
      )}

      <Sec num={health ? "03" : "02"} title="Upload tokens" note="one per uploader (client-rs)">
        <div className="ds-panel">
          <div className="arow">
            <input className="ds-input" placeholder="name (e.g. matella)" value={tokenName} onChange={(e) => setTokenName(e.target.value)} />
            <button className="ds-btn primary" onClick={createToken}>Create</button>
          </div>
          {newToken && (
            <div className="newtok">
              <span className="ds-label" style={{ margin: 0 }}>New token — copy it now, it won't be shown again</span>
              <code>{newToken}</code>
            </div>
          )}
        </div>
      </Sec>

      <div className="acols">
        <Sec num={health ? "04" : "03"} title="Teams" note={`${(teams ?? []).length}`}>
          <div className="ds-panel flush">
            <div className="arow pad">
              <input className="ds-input" style={{ flex: 1 }} placeholder="team name" value={teamName} onChange={(e) => setTeamName(e.target.value)} />
              <input className="ds-input" style={{ width: 140 }} placeholder="league (optional)" value={teamLeague} onChange={(e) => setTeamLeague(e.target.value)} />
              <button className="ds-btn primary" onClick={createTeam}>Add</button>
            </div>
            {(teams ?? []).map((t: any) => (
              <div key={t.id} className="lrow">
                <strong>{t.name}</strong>
                <input className="ds-input small" placeholder="league" defaultValue={t.league ?? ""} onBlur={(e) => setLeague(t.id, e.target.value)} />
                <span className="cnt">{(t.roster ?? []).length} members</span>
                <button className="ds-btn small danger" onClick={() => del(`/api/teams/${t.id}`, "teams")}>Delete</button>
              </div>
            ))}
            {(teams ?? []).length === 0 && <div className="none pad">no teams</div>}
          </div>
        </Sec>

        <Sec num={health ? "05" : "04"} title="Collections" note={`${(collections ?? []).length}`}>
          <div className="ds-panel flush">
            <div className="arow pad">
              <input className="ds-input" style={{ flex: 1 }} placeholder="collection name" value={collName} onChange={(e) => setCollName(e.target.value)} />
              <button className="ds-btn primary" onClick={createColl}>Add</button>
            </div>
            {(collections ?? []).map((c: any) => (
              <div key={c.id} className="lrow">
                <strong>{c.name}</strong>
                <span className="cnt" style={{ marginLeft: "auto" }}>{c.count} matches</span>
                <button className="ds-btn small danger" onClick={() => del(`/api/collections/${c.id}`, "collections")}>Delete</button>
              </div>
            ))}
            {(collections ?? []).length === 0 && <div className="none pad">no collections</div>}
          </div>
        </Sec>
      </div>
    </div>
  );
}

function Sec({ num, title, note, children }: { num: string; title: string; note?: React.ReactNode; children: React.ReactNode }) {
  return (
    <section className="ds-sec" style={{ marginTop: 34 }}>
      <div className="ds-sec-hd"><span className="num">{num}</span><h2>{title}</h2>{note && <small>{note}</small>}</div>
      {children}
    </section>
  );
}

const CSS = `
.admin .mode.open{font:12px var(--mono);color:var(--muted-2)}
.admin .arow{display:flex;flex-wrap:wrap;gap:8px;align-items:center}
.admin .arow.pad{padding:14px 16px}
.admin .amsg{margin-top:10px;font:12px var(--mono)}
.admin .ahint{margin:12px 0 0;font:11px var(--mono);color:var(--kicker);line-height:1.6}
.admin .tiles{display:flex;flex-wrap:wrap;gap:10px}
.admin .tile{min-width:110px;padding:10px 14px;border-radius:12px;background:var(--panel-2);border:1px solid var(--line);box-shadow:inset 3px 0 0 var(--line-2)}
.admin .tile b{display:block;font:900 34px/1 var(--display)}
.admin .tile span{font:600 10px var(--mono);letter-spacing:.12em;text-transform:uppercase;color:var(--muted-2)}
.admin .tile.done,.admin .tile.parsed,.admin .tile.ok{box-shadow:inset 3px 0 0 var(--win)} .admin .tile.done b,.admin .tile.parsed b,.admin .tile.ok b{color:var(--win)}
.admin .tile.failed,.admin .tile.parse_failed{box-shadow:inset 3px 0 0 var(--loss)} .admin .tile.failed b,.admin .tile.parse_failed b{color:var(--loss-soft)}
.admin .tile.duplicate,.admin .tile.pending{box-shadow:inset 3px 0 0 var(--gold)}
.admin .newtok{margin-top:14px;padding:12px 14px;border-radius:12px;border:1px solid rgba(93,202,165,.35);background:rgba(93,202,165,.07);display:flex;flex-direction:column;gap:6px}
.admin .newtok code{font:600 13px var(--mono);color:var(--win);word-break:break-all;user-select:all}
.admin .acols{display:grid;grid-template-columns:1fr 1fr;gap:0 18px}
.admin .lrow{display:flex;align-items:center;gap:10px;padding:10px 16px;border-top:1px solid var(--line)}
.admin .lrow strong{font:800 17px var(--display);text-transform:uppercase;letter-spacing:.02em;min-width:0;overflow:hidden;text-overflow:ellipsis;white-space:nowrap}
.admin .lrow .ds-input.small{margin-left:auto;width:120px;padding:5px 9px;font-size:12px}
.admin .lrow .cnt{font:11px var(--mono);color:var(--muted-2);white-space:nowrap}
.admin .none{font:12px var(--mono);color:var(--kicker)} .admin .none.pad{padding:16px}
@media (max-width:900px){.admin .acols{grid-template-columns:1fr}}
`;
