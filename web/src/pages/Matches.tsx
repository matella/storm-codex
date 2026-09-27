import { useQuery } from "@tanstack/react-query";
import { useNavigate, useSearchParams } from "react-router-dom";
import {
  fetchMatches, fetchHeroes, fmtTime, fmtDur, pickOperator,
  matchesParams, useSettings, operatorNames, awardLabel, type MatchSummary, type MatchListParams,
} from "../api";
import { SearchSelect } from "../components/SearchSelect";
import { Portrait } from "../components/ds";
import { ModeTag, hasMapArt, mapArt } from "../components/ds/match";

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

/** date locale +1 jour (pour rendre la borne `to` inclusive côté serveur qui filtre `< to`). */
const dayPlus1 = (d: string) => { const x = new Date(d + "T00:00:00"); x.setDate(x.getDate() + 1); return x.toISOString(); };

/** Matches — l'archive filtrable. Couverture + filtres (pilotés par l'URL), liste dense de parties. */
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
  const known = (data ?? []).map(ownPlayer).filter((o) => o.win != null);
  const w = known.filter((o) => o.win).length;

  return (
    <div className="ds-page">
      <header className="ds-cover" style={{ marginTop: 18 }}>
        <div className="ds-kicker">Archive</div>
        <h1 className="ds-title" style={{ cursor: "default" }}>Matches</h1>
        <div className="ds-subtitle" style={{ cursor: "default" }}>
          {isLoading ? "loading…" : `${data?.length ?? 0} game${data?.length === 1 ? "" : "s"}${data?.length === 200 ? " (latest 200)" : ""}`}
          {known.length > 0 && <> · <b style={{ color: "var(--win)" }}>{w}</b>–<b style={{ color: "var(--loss-soft)" }}>{known.length - w}</b> for you</>}
          {active && " · filtered"}
        </div>
        <div className="ds-filters">
          <div className="ds-pills">
            {MODE_FILTERS.map(([label, m]) => (
              <span key={label} className={mode === m ? "ds-pill on" : "ds-pill"} onClick={() => setParam("mode", m != null ? String(m) : "")}>{label}</span>
            ))}
            <span className="ds-sep" />
            {(["", "win", "loss"] as const).map((r) => (
              <span key={r || "all"} className={result === r ? "ds-pill on" : "ds-pill"} onClick={() => setParam("result", r)}>
                {r === "" ? "W+L" : r === "win" ? "Wins" : "Losses"}
              </span>
            ))}
            <span className={mvp ? "ds-pill on" : "ds-pill"} onClick={() => setParam("mvp", mvp ? "" : "true")}>👑 MVP</span>
          </div>
          <div className="ds-pills">
            <SearchSelect className="ds-input" style={{ width: 170 }} placeholder="search map…" value={map}
              onChange={(v) => setParam("map", v)} options={(maps ?? []).map((m) => m.map).sort((a, b) => a.localeCompare(b))} />
            <SearchSelect className="ds-input" style={{ width: 170 }} placeholder="search hero…" value={hero}
              onChange={(v) => setParam("hero", v)} options={(heroes ?? []).map((h) => h.hero).sort((a, b) => a.localeCompare(b))} />
            {accounts.length > 1 && (
              <select className="ds-input" value={account} onChange={(e) => setParam("account", e.target.value)}>
                <option value="">All my accounts</option>
                {accounts.map((a) => <option key={a} value={a}>{a}</option>)}
              </select>
            )}
            <label>from <input type="date" className="ds-input" value={from} onChange={(e) => setParam("from", e.target.value)} /></label>
            <label>to <input type="date" className="ds-input" value={to} onChange={(e) => setParam("to", e.target.value)} /></label>
            {active && <span className="ds-pill" onClick={reset}>✕ reset</span>}
            <span style={{ marginLeft: "auto", display: "flex", gap: 6 }}>
              <a href={`/api/matches.csv?${exportQ({ limit: "5000" })}`} className="ds-btn">CSV ↓</a>
              <a href={`/api/matches?${exportQ({ limit: "5000" })}`} className="ds-btn" target="_blank" rel="noreferrer">JSON ↓</a>
            </span>
          </div>
        </div>
      </header>

      <section className="ds-sec" style={{ marginTop: 26 }}>
        {isLoading && <div className="ds-empty">loading…</div>}
        {data?.length === 0 && <div className="ds-empty">No match for this filter.</div>}
        <div className="ds-mlist">
          {data?.map((m) => {
            const o = ownPlayer(m);
            const aw = awardLabel(o.award);
            return (
              <div key={m.id} className={`ds-mrow ${o.win === true ? "w" : o.win === false ? "l" : ""}`}
                style={hasMapArt(m.map) ? mapArt(m.map) : undefined} onClick={() => nav(`/match/${m.id}`)}
                role="link" tabIndex={0} onKeyDown={(e) => { if (e.key === "Enter") nav(`/match/${m.id}`); }}>
                <span className="when">{fmtTime(m.played_at)}</span>
                <span><ModeTag mode={m.mode} short /></span>
                {o.hero ? <Portrait hero={o.hero} size={40} tone={o.win === true ? "pick" : o.win === false ? "loss" : "plain"} /> : <span />}
                <span className="what"><span className="map">{m.map ?? "—"}</span>
                  <span className="hero">{o.hero ?? `${m.winner === 0 ? "Blue" : m.winner === 1 ? "Red" : "?"} team wins`}</span></span>
                <span style={{ display: "flex", gap: 6 }}>
                  {o.win != null && <span className={`ds-tag ${o.win ? "win" : "loss"}`}>{o.win ? "WIN" : "LOSS"}</span>}
                  {aw?.mvp && <span className="ds-tag mvp">👑 MVP</span>}
                </span>
                <span className="len">{fmtDur(m.length)} ›</span>
              </div>
            );
          })}
        </div>
      </section>
    </div>
  );
}
