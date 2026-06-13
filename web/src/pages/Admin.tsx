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

  const inp = { background: "var(--surface-2)", border: "1px solid var(--hairline-strong)", color: "var(--text)", borderRadius: 6, padding: "5px 9px", fontSize: 12 } as const;

  return (
    <>
      <h1>Admin / Import</h1>
      {adminOpen ? (
        <div className="card">
          <div className="row"><span className="muted" style={{ fontSize: 11 }}>
            🔓 Local mode — no admin token required (server has no ADMIN_TOKEN set).
            Set ADMIN_TOKEN in the server env to re-enable auth (recommended if exposed publicly).
          </span></div>
        </div>
      ) : (
        <div className="card">
          <div className="card-hd"><span className="kick" style={{ margin: 0 }}>Admin token</span></div>
          <div className="row">
            <input style={{ ...inp, flex: 1 }} type="password" placeholder="ADMIN_TOKEN" value={token} onChange={(e) => setToken(e.target.value)} />
            <span className="muted" style={{ fontSize: 10 }}>{token ? "configured" : "required for actions"}</span>
          </div>
        </div>
      )}

      <p className="cap">My identity (operator perspective)</p>
      <div className="card">
        <div className="row">
          <input
            style={{ ...inp, flex: 1 }}
            placeholder="my in-game names, comma-separated (e.g. matella, MatellaSmurf)"
            value={opValue}
            onChange={(e) => setOpNames(e.target.value)}
          />
          <span className="pill on" onClick={saveOperator}>save</span>
          {opMsg && (
            <span className="mono" style={{ fontSize: 11, color: opMsg.startsWith("✓") ? "var(--win)" : "var(--loss)" }}>{opMsg}</span>
          )}
        </div>
        <div className="row"><span className="muted" style={{ fontSize: 10 }}>
          Multiple accounts? Comma-separate them (e.g. matella, матella). Cyrillic / any UTF-8 is fine.
          Used everywhere (session, matches, widget) and by the Jarvis brief.
        </span></div>
      </div>

      {health && (
        <>
          <p className="cap">Upload health</p>
          <div className="card">
            <div className="row"><span className="muted">By status</span><span className="mono" style={{ marginLeft: "auto" }}>{JSON.stringify(health.by_status)}</span></div>
            <div className="row"><span className="muted">Failures by class</span><span className="mono" style={{ marginLeft: "auto" }}>{JSON.stringify(health.by_error_class)}</span></div>
            <div className="row"><span className="muted">parser_version</span><span className="mono" style={{ marginLeft: "auto" }}>{health.parser_version}</span></div>
            <div className="row link" onClick={reprocess}><span style={{ color: "var(--accent)" }}>Re-process (stale parser_version) ›</span></div>
          </div>
        </>
      )}

      <p className="cap">Upload tokens</p>
      <div className="card">
        <div className="row">
          <input style={inp} placeholder="name (e.g. matella)" value={tokenName} onChange={(e) => setTokenName(e.target.value)} />
          <span className="pill on" onClick={createToken}>create</span>
        </div>
        {newToken && <div className="row"><span className="muted">new token (copy now)</span><span className="mono" style={{ marginLeft: "auto", color: "var(--win)" }}>{newToken}</span></div>}
      </div>

      <p className="cap">Teams</p>
      <div className="card">
        <div className="row">
          <input style={inp} placeholder="team name" value={teamName} onChange={(e) => setTeamName(e.target.value)} />
          <input style={inp} placeholder="league (optional)" value={teamLeague} onChange={(e) => setTeamLeague(e.target.value)} />
          <span className="pill on" onClick={createTeam}>add</span>
        </div>
        {(teams ?? []).map((t: any) => (
          <div key={t.id} className="row"><span>{t.name}</span>
            <input style={{ ...inp, marginLeft: "auto", fontSize: 11, width: 120 }} placeholder="league"
                   defaultValue={t.league ?? ""} onBlur={(e) => setLeague(t.id, e.target.value)} />
            <span className="muted" style={{ fontSize: 10 }}>{(t.roster ?? []).length} members</span>
            <span className="pill" onClick={async () => { await adminFetch(`/api/teams/${t.id}`, token, { method: "DELETE" }); qc.invalidateQueries({ queryKey: ["teams"] }); }}>del.</span>
          </div>
        ))}
        {(teams ?? []).length === 0 && <div className="empty">no teams</div>}
      </div>

      <p className="cap">Collections</p>
      <div className="card">
        <div className="row">
          <input style={inp} placeholder="collection name" value={collName} onChange={(e) => setCollName(e.target.value)} />
          <span className="pill on" onClick={createColl}>add</span>
        </div>
        {(collections ?? []).map((c: any) => (
          <div key={c.id} className="row"><span>{c.name}</span><span className="muted" style={{ marginLeft: "auto", fontSize: 10 }}>{c.count} matches</span>
            <span className="pill" onClick={async () => { await adminFetch(`/api/collections/${c.id}`, token, { method: "DELETE" }); qc.invalidateQueries({ queryKey: ["collections"] }); }}>del.</span>
          </div>
        ))}
        {(collections ?? []).length === 0 && <div className="empty">no collections</div>}
      </div>
    </>
  );
}
