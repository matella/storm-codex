import { useQuery } from "@tanstack/react-query";

interface Team { id: number; name: string; roster: string[] | null; league: string | null }

/** Leagues — teams grouped by their league (a league sits above teams). Read-only here; create
 *  teams and assign their league in Admin. */
export function Leagues() {
  const { data: teams } = useQuery({ queryKey: ["teams"], queryFn: async () => (await fetch("/api/teams")).json() as Promise<Team[]> });

  const groups = new Map<string, Team[]>();
  for (const t of teams ?? []) {
    const key = t.league?.trim() || "Unassigned";
    (groups.get(key) ?? groups.set(key, []).get(key)!).push(t);
  }
  const ordered = [...groups.entries()].sort((a, b) =>
    a[0] === "Unassigned" ? 1 : b[0] === "Unassigned" ? -1 : a[0].localeCompare(b[0]));

  return (
    <>
      <h1>Leagues</h1>
      {ordered.length === 0 && <div className="card"><div className="empty">No teams yet — create them in Admin.</div></div>}
      {ordered.map(([league, ts]) => (
        <div key={league}>
          <p className="cap">{league} · {ts.length} team{ts.length > 1 ? "s" : ""}</p>
          <div className="card">
            {ts.map((t) => (
              <div key={t.id} className="row">
                <span style={{ fontSize: 13, fontWeight: 500 }}>{t.name}</span>
                <span className="mono muted" style={{ marginLeft: "auto", fontSize: 11 }}>
                  {(t.roster ?? []).length ? (t.roster ?? []).join(" · ") : "no roster"}
                </span>
              </div>
            ))}
          </div>
        </div>
      ))}
    </>
  );
}
