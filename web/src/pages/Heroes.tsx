import { useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { useNavigate } from "react-router-dom";
import { fetchHeroes, aggToSearch, useDimHeroes, type HeroStat, type AggFilter } from "../api";
import { AggFilterBar } from "../components/AggFilterBar";
import { Portrait } from "../components/ds";

type SortKey = "games" | "winrate";

/** Heroes — mur de héros sur l'ensemble filtré : portrait, parties, winrate (jauge colorée). */
export function Heroes() {
  const [sort, setSort] = useState<SortKey>("games");
  const [role, setRole] = useState<string>("");
  const [filter, setFilter] = useState<AggFilter>({});
  const nav = useNavigate();
  const dim = useDimHeroes();
  const { data, isLoading } = useQuery({ queryKey: ["heroes", filter], queryFn: () => fetchHeroes(filter) });

  const roleOf = (h: string) => {
    const k = h.toLowerCase().replace(/[^a-z]/g, "");
    const e = dim && (dim[h] ?? Object.entries(dim).find(([n]) => n.toLowerCase().replace(/[^a-z]/g, "") === k)?.[1]);
    return e?.role ?? null;
  };
  const roles = [...new Set((data ?? []).map((h) => roleOf(h.hero)).filter((r): r is string => !!r && !r.includes(",")))].sort();
  const wr = (h: HeroStat) => (h.games ? h.wins / h.games : 0);
  const rows = [...(data ?? [])].filter((h) => !role || roleOf(h.hero) === role).sort((a, b) => (sort === "games" ? b.games - a.games : wr(b) - wr(a)));
  const total = (data ?? []).reduce((s, h) => s + h.games, 0);

  return (
    <div className="ds-page">
      <header className="ds-cover" style={{ marginTop: 18 }}>
        <div className="ds-kicker">Heroes</div>
        <h1 className="ds-title" style={{ cursor: "default" }}>Hero pool</h1>
        <div className="ds-subtitle" style={{ cursor: "default" }}>{data ? `${data.length} heroes · ${total} picks over the filtered set` : "loading…"} — “My heroes” keeps only your accounts.</div>
        <AggFilterBar value={filter} onChange={setFilter} mineLabel="My heroes" />
        <div className="ds-filterbar" style={{ marginTop: 10 }}>
          <span className="ds-label" style={{ margin: 0 }}>Sort</span>
          <span className={sort === "games" ? "ds-pill on" : "ds-pill"} onClick={() => setSort("games")}>Most played</span>
          <span className={sort === "winrate" ? "ds-pill on" : "ds-pill"} onClick={() => setSort("winrate")}>Win rate</span>
          {roles.length > 0 && <span className="ds-sep" />}
          {roles.length > 0 && <span className={role === "" ? "ds-pill on" : "ds-pill"} onClick={() => setRole("")}>All roles</span>}
          {roles.map((r) => <span key={r} className={role === r ? "ds-pill on" : "ds-pill"} onClick={() => setRole(r)}>{r}</span>)}
        </div>
      </header>

      <section className="ds-sec" style={{ marginTop: 26 }}>
        {isLoading && <div className="ds-empty">loading…</div>}
        {data?.length === 0 && <div className="ds-empty">No hero for this filter.</div>}
        <div className="ds-herowall">
          {rows.map((h) => {
            const p = Math.round(100 * wr(h));
            const cls = p >= 55 ? "hot" : p < 45 ? "cold" : "";
            return (
              <div key={h.hero} className="ds-htile" role="link" tabIndex={0}
                onClick={() => { const q = aggToSearch(filter).toString(); nav(`/hero/${encodeURIComponent(h.hero)}${q ? `?${q}` : ""}`); }}
                onKeyDown={(e) => { if (e.key === "Enter") nav(`/hero/${encodeURIComponent(h.hero)}`); }}>
                <Portrait hero={h.hero} size={64} />
                <strong>{h.hero}</strong>
                <span className={`wr ${cls}`}>{p}<i>%</i></span>
                <i className={`ds-bar ${cls === "hot" ? "win" : cls === "cold" ? "loss" : ""}`}><b style={{ width: `${p}%` }} /></i>
                <em>{h.wins}/{h.games} games</em>
              </div>
            );
          })}
        </div>
      </section>
    </div>
  );
}
