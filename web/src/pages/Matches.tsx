import { useQuery } from "@tanstack/react-query";
import { useNavigate, useSearchParams } from "react-router-dom";
import {
  fetchMatches, fetchHeroes, modeBadge, fmtTime, fmtDur, mapImage, pickOperator,
  matchesParams, useSettings, operatorNames, awardLabel, type MatchSummary, type MatchListParams,
} from "../api";
import { Avatar } from "../components/Avatar";
import { SearchSelect } from "../components/SearchSelect";

// Codes officiels (storm-stats GameMode). Brawls/IA rejetés au parse → on liste les modes réels.
const MODE_FILTERS: [string, number | undefined][] = [
  ["All", undefined],
  ["Storm League", 50091],
  ["ARAM", 50101],
  ["Custom", -1],
  ["Hero League", 50061],
  ["QM", 50001],
];

interface MapStat { map: string; games: number }

function ownPlayer(m: MatchSummary) {
  const p = pickOperator(m.players ?? []);
  return { hero: p?.hero ?? null, win: p?.win ?? null, award: p?.award ?? null };
}

const inp = { background: "var(--surface-2)", border: "1px solid var(--hairline-strong)", color: "var(--text)", borderRadius: 6, padding: "4px 8px", fontSize: 12 } as const;
/** date locale +1 jour (pour rendre la borne `to` inclusive côté serveur qui filtre `< to`). */
const dayPlus1 = (d: string) => { const x = new Date(d + "T00:00:00"); x.setDate(x.getDate() + 1); return x.toISOString(); };

export function Matches() {
  useSettings();
  const accounts = operatorNames();
  const nav = useNavigate();
  // Filtres pilotés par l'URL (query string) : ils PERSISTENT au retour navigateur (recherche →
  // clic dans une partie → retour restaure l'état). `replace` pour ne pas polluer l'historique.
  const [sp, setSp] = useSearchParams();
  const get = (k: string) => sp.get(k) ?? "";
  const setParam = (k: string, v: string) =>
    setSp((prev) => { const n = new URLSearchParams(prev); v ? n.set(k, v) : n.delete(k); return n; }, { replace: true });

  const mode = sp.get("mode") ? Number(sp.get("mode")) : undefined;
  const map = get("map"), hero = get("hero"), account = get("account");
  const result = get("result"); // "" | "win" | "loss"
  const mvp = sp.get("mvp") === "true";
  const from = get("from"), to = get("to"); // YYYY-MM-DD bruts (lisibles dans l'URL)

  const params: MatchListParams = {
    mode,
    map: map || undefined,
    hero: hero || undefined,
    account: account || undefined,
    result: (result as "win" | "loss") || undefined,
    mvp: mvp || undefined,
    from: from ? new Date(from + "T00:00:00").toISOString() : undefined,
    to: to ? dayPlus1(to) : undefined,
  };
  const { data, isLoading } = useQuery({
    queryKey: ["matches", params],
    queryFn: () => fetchMatches({ ...params, limit: 200 }),
  });
  const { data: maps } = useQuery({
    queryKey: ["maps-filter"],
    queryFn: async () => (await fetch("/api/maps")).json() as Promise<MapStat[]>,
    staleTime: Infinity,
  });
  const { data: heroes } = useQuery({ queryKey: ["heroes-filter"], queryFn: () => fetchHeroes(), staleTime: Infinity });

  const active = [...sp.keys()].length > 0;
  const reset = () => setSp({}, { replace: true });
  const exportQ = (extra: Record<string, string>) => { const q = matchesParams(params); Object.entries(extra).forEach(([k, v]) => q.set(k, v)); return q.toString(); };

  return (
    <>
      <h1>Matches</h1>
      <div className="card">
        {/* ligne 1 : modes + résultat + MVP */}
        <div className="card-hd" style={{ flexWrap: "wrap", gap: 6 }}>
          {MODE_FILTERS.map(([label, m]) => (
            <span key={label} className={mode === m ? "pill on" : "pill"} onClick={() => setParam("mode", m != null ? String(m) : "")}>{label}</span>
          ))}
          <span style={{ width: 1, alignSelf: "stretch", background: "var(--hairline)", margin: "0 4px" }} />
          {(["", "win", "loss"] as const).map((r) => (
            <span key={r || "all"} className={result === r ? "pill on" : "pill"} onClick={() => setParam("result", r)}>
              {r === "" ? "W+L" : r === "win" ? "Wins" : "Losses"}
            </span>
          ))}
          <span className={mvp ? "pill on" : "pill"} onClick={() => setParam("mvp", mvp ? "" : "true")}>👑 MVP</span>
        </div>
        {/* ligne 2 : carte / héros (recherche textuelle) / compte / dates / reset / export */}
        <div className="row" style={{ flexWrap: "wrap", gap: 8, alignItems: "center" }}>
          <SearchSelect
            style={{ ...inp, width: 150 }}
            placeholder="search map…"
            value={map}
            onChange={(v) => setParam("map", v)}
            options={(maps ?? []).map((m) => m.map).sort((a, b) => a.localeCompare(b))}
          />
          <SearchSelect
            style={{ ...inp, width: 150 }}
            placeholder="search hero…"
            value={hero}
            onChange={(v) => setParam("hero", v)}
            options={(heroes ?? []).map((h) => h.hero).sort((a, b) => a.localeCompare(b))}
          />
          {accounts.length > 1 && (
            <select style={inp} value={account} onChange={(e) => setParam("account", e.target.value)}>
              <option value="">All my accounts</option>
              {accounts.map((a) => <option key={a} value={a}>{a}</option>)}
            </select>
          )}
          <label style={{ fontSize: 10, color: "var(--text-2)" }}>from <input type="date" style={inp} value={from} onChange={(e) => setParam("from", e.target.value)} /></label>
          <label style={{ fontSize: 10, color: "var(--text-2)" }}>to <input type="date" style={inp} value={to} onChange={(e) => setParam("to", e.target.value)} /></label>
          {active && <span className="pill" onClick={reset}>✕ reset</span>}
          <a href={`/api/matches.csv?${exportQ({ limit: "5000" })}`} className="pill" style={{ marginLeft: "auto" }}>CSV ↓</a>
          <a href={`/api/matches?${exportQ({ limit: "5000" })}`} className="pill" target="_blank" rel="noreferrer">JSON ↓</a>
          <span style={{ fontSize: 10, color: "var(--kicker)" }}>{data?.length ?? 0} matches</span>
        </div>

        {isLoading && <div className="empty">loading…</div>}
        {data?.length === 0 && <div className="empty">no matches for this filter</div>}
        {data?.map((m) => {
          const mb = modeBadge(m.mode);
          const o = ownPlayer(m);
          const aw = awardLabel(o.award);
          const bg = mapImage(m.map);
          return (
            <div
              key={m.id}
              className="row link"
              onClick={() => nav(`/match/${m.id}`)}
              style={bg ? {
                backgroundImage: `linear-gradient(90deg, var(--surface) 0%, rgba(14,16,22,.82) 45%, rgba(14,16,22,.62) 100%), url(${bg})`,
                backgroundSize: "cover", backgroundPosition: "center 30%",
              } : undefined}
            >
              <span className="mono muted" style={{ minWidth: 92, fontSize: 11 }}>{fmtTime(m.played_at)}</span>
              <span className={`bdg ${mb.cls}`}>{mb.short}</span>
              <Avatar hero={o.hero} />
              <span style={{ fontSize: 12 }}>{m.map ?? "—"}</span>
              {o.win != null && <span className={`bdg ${o.win ? "b-win" : "b-loss"}`}>{o.win ? "W" : "L"}</span>}
              {aw?.mvp && <span title="MVP" style={{ fontSize: 9, fontWeight: 700, padding: "1px 5px", borderRadius: 999, color: "#1a1500", background: "linear-gradient(90deg,#f5c542,#e0a818)" }}>MVP</span>}
              <span style={{ marginLeft: "auto", color: "var(--kicker)", fontSize: 10 }}>
                {fmtDur(m.length)} · {m.winner === 0 ? "blue" : m.winner === 1 ? "red" : "?"} team wins ›
              </span>
            </div>
          );
        })}
      </div>
    </>
  );
}
