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
  const { data: health } = useQuery({
    queryKey: ["admin-uploads", token],
    queryFn: async () => (token ? (await adminFetch("/api/admin/uploads", token)).json() : null),
    enabled: !!token,
  });
  const { data: teams } = useQuery({ queryKey: ["teams"], queryFn: async () => (await fetch("/api/teams")).json() });
  const { data: collections } = useQuery({ queryKey: ["collections"], queryFn: async () => (await fetch("/api/collections")).json() });

  const [tokenName, setTokenName] = useState("");
  const [newToken, setNewToken] = useState<string | null>(null);
  const [teamName, setTeamName] = useState("");
  const [collName, setCollName] = useState("");

  const createToken = async () => {
    const r = await adminFetch("/api/admin/tokens", token, { method: "POST", body: JSON.stringify({ name: tokenName }) });
    if (r.ok) { setNewToken((await r.json()).token); setTokenName(""); }
  };
  const createTeam = async () => {
    await adminFetch("/api/teams", token, { method: "POST", body: JSON.stringify({ name: teamName, roster: [] }) });
    setTeamName(""); qc.invalidateQueries({ queryKey: ["teams"] });
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
      <div className="card">
        <div className="card-hd"><span className="kick" style={{ margin: 0 }}>Jeton admin</span></div>
        <div className="row">
          <input style={{ ...inp, flex: 1 }} type="password" placeholder="ADMIN_TOKEN" value={token} onChange={(e) => setToken(e.target.value)} />
          <span className="muted" style={{ fontSize: 10 }}>{token ? "configuré" : "requis pour les actions"}</span>
        </div>
      </div>

      {health && (
        <>
          <p className="cap">Santé des uploads</p>
          <div className="card">
            <div className="row"><span className="muted">Par statut</span><span className="mono" style={{ marginLeft: "auto" }}>{JSON.stringify(health.by_status)}</span></div>
            <div className="row"><span className="muted">Échecs par classe</span><span className="mono" style={{ marginLeft: "auto" }}>{JSON.stringify(health.by_error_class)}</span></div>
            <div className="row"><span className="muted">parser_version</span><span className="mono" style={{ marginLeft: "auto" }}>{health.parser_version}</span></div>
            <div className="row link" onClick={reprocess}><span style={{ color: "var(--accent)" }}>Re-process (parser_version périmé) ›</span></div>
          </div>
        </>
      )}

      <p className="cap">Tokens d'upload</p>
      <div className="card">
        <div className="row">
          <input style={inp} placeholder="nom (ex. matella)" value={tokenName} onChange={(e) => setTokenName(e.target.value)} />
          <span className="pill on" onClick={createToken}>créer</span>
        </div>
        {newToken && <div className="row"><span className="muted">nouveau token (copier maintenant)</span><span className="mono" style={{ marginLeft: "auto", color: "var(--win)" }}>{newToken}</span></div>}
      </div>

      <p className="cap">Équipes</p>
      <div className="card">
        <div className="row">
          <input style={inp} placeholder="nom d'équipe" value={teamName} onChange={(e) => setTeamName(e.target.value)} />
          <span className="pill on" onClick={createTeam}>ajouter</span>
        </div>
        {(teams ?? []).map((t: any) => (
          <div key={t.id} className="row"><span>{t.name}</span><span className="muted" style={{ marginLeft: "auto", fontSize: 10 }}>{(t.roster ?? []).length} membres</span>
            <span className="pill" onClick={async () => { await adminFetch(`/api/teams/${t.id}`, token, { method: "DELETE" }); qc.invalidateQueries({ queryKey: ["teams"] }); }}>suppr.</span>
          </div>
        ))}
        {(teams ?? []).length === 0 && <div className="empty">aucune équipe</div>}
      </div>

      <p className="cap">Collections</p>
      <div className="card">
        <div className="row">
          <input style={inp} placeholder="nom de collection" value={collName} onChange={(e) => setCollName(e.target.value)} />
          <span className="pill on" onClick={createColl}>ajouter</span>
        </div>
        {(collections ?? []).map((c: any) => (
          <div key={c.id} className="row"><span>{c.name}</span><span className="muted" style={{ marginLeft: "auto", fontSize: 10 }}>{c.count} matchs</span>
            <span className="pill" onClick={async () => { await adminFetch(`/api/collections/${c.id}`, token, { method: "DELETE" }); qc.invalidateQueries({ queryKey: ["collections"] }); }}>suppr.</span>
          </div>
        ))}
        {(collections ?? []).length === 0 && <div className="empty">aucune collection</div>}
      </div>
    </>
  );
}
