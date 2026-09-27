import { useQuery } from "@tanstack/react-query";
import { Link } from "react-router-dom";
import { SecHead } from "../components/ds";

interface Team { id: number; name: string; roster: string[] | null; league: string | null }

/** Leagues — équipes groupées par ligue (une ligue est au-dessus des équipes). Lecture seule ; les
 *  équipes se créent et se rangent dans une ligue depuis Admin. */
export function Leagues() {
  const { data: teams, isLoading } = useQuery({ queryKey: ["teams"], queryFn: async () => (await fetch("/api/teams")).json() as Promise<Team[]> });
  const groups = new Map<string, Team[]>();
  for (const t of teams ?? []) {
    const key = t.league?.trim() || "Unassigned";
    (groups.get(key) ?? groups.set(key, []).get(key)!).push(t);
  }
  const ordered = [...groups.entries()].sort((a, b) => (a[0] === "Unassigned" ? 1 : b[0] === "Unassigned" ? -1 : a[0].localeCompare(b[0])));

  return (
    <div className="ds-page">
      <header className="ds-cover" style={{ marginTop: 18 }}>
        <div className="ds-kicker">Leagues</div>
        <h1 className="ds-title" style={{ cursor: "default" }}>Leagues</h1>
        <div className="ds-subtitle" style={{ cursor: "default" }}>
          {teams ? `${teams.length} team${teams.length === 1 ? "" : "s"} in ${ordered.length} league${ordered.length === 1 ? "" : "s"}` : "loading…"} — create teams and set their league in Admin.
        </div>
        <div className="ds-cover-foot"><Link className="ds-btn" to="/admin">Manage teams in Admin ›</Link></div>
      </header>
      {isLoading && <div className="ds-empty" style={{ marginTop: 20 }}>loading…</div>}
      {teams && ordered.length === 0 && <div className="ds-empty" style={{ marginTop: 20 }}>No teams yet — create them in Admin.</div>}
      {ordered.map(([league, ts], i) => (
        <section key={league} className="ds-sec">
          <SecHead num={i + 1} title={league} sub={`${ts.length} team${ts.length > 1 ? "s" : ""}`} />
          <div className="ds-teams">
            {ts.map((t) => (
              <div key={t.id} className="ds-team">
                <strong>{t.name}</strong>
                <div className="ds-chips">
                  {(t.roster ?? []).length ? (t.roster ?? []).map((r) => <span key={r} className="ds-chip text"><span>{r}</span></span>) : <span className="ds-none">no roster</span>}
                </div>
              </div>
            ))}
          </div>
        </section>
      ))}
    </div>
  );
}
