import { useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Link } from "react-router-dom";
import { fetchIdentity, putLink, type LinkSuggestion } from "../api";
import { recentNames, seenRange } from "../players";
import { SecHead } from "./ds";

const SHOWN_SUGGESTIONS = 5;

function Suggestion({ s, busy, onDecide }: { s: LinkSuggestion; busy: boolean; onDecide: (status: "confirmed" | "dismissed") => void }) {
  return (
    <div className="ds-panel" style={{ marginTop: 10 }}>
      <div style={{ display: "flex", alignItems: "center", gap: 12, flexWrap: "wrap" }}>
        <span className={`ds-status ${s.level === "high" ? "analyzed" : "stale"}`}>{s.level === "high" ? "HIGH" : "LOW"} {s.score}</span>
        <Link to={`/player/${encodeURIComponent(s.toon)}`} style={{ font: "800 20px var(--display)", textTransform: "uppercase", color: "var(--text)", textDecoration: "none" }}>
          {s.name ?? s.toon}
        </Link>
        <span className="ds-actions">
          <button className="ds-btn small primary" disabled={busy} onClick={() => onDecide("confirmed")}>Confirm</button>
          <button className="ds-btn small" disabled={busy} onClick={() => onDecide("dismissed")}>Not the same person</button>
        </span>
      </div>
      <ul style={{ margin: "8px 0 0", paddingLeft: 18, color: "var(--muted-2)", font: "12px var(--mono)" }}>
        {s.reasons.map((r) => <li key={r.kind}>{r.text}</li>)}
      </ul>
    </div>
  );
}

/** Onglet Identité : historique daté des pseudos + comptes devinés (estimation) + liens confirmés. */
export function PlayerIdentity({ toon }: { toon: string }) {
  const qc = useQueryClient();
  const { data, isLoading, isError } = useQuery({ queryKey: ["identity", toon], queryFn: () => fetchIdentity(toon) });
  const [allNames, setAllNames] = useState(false);
  const [allSugg, setAllSugg] = useState(false);
  const decide = useMutation({
    mutationFn: (v: { other: string; status: "confirmed" | "dismissed" }) => putLink(toon, v.other, v.status),
    // la décision est symétrique : toutes les identités en cache peuvent changer
    onSuccess: () => qc.invalidateQueries({ queryKey: ["identity"] }),
  });

  if (isLoading) return <div className="ds-empty" style={{ marginTop: 24 }}>looking for linked accounts…</div>;
  if (isError || !data) return <div className="ds-empty" style={{ marginTop: 24 }}>Could not load identity.</div>;

  const { shown, hidden } = recentNames(data.names);
  const names = allNames ? data.names : shown;
  const suggestions = allSugg ? data.suggestions : data.suggestions.slice(0, SHOWN_SUGGESTIONS);
  const err = decide.error instanceof Error ? decide.error.message : "failed";

  return (
    <>
      <section className="ds-sec">
        <SecHead num={1} title="Names" sub={hidden > 0
          ? <span className="ds-pill" onClick={() => setAllNames(!allNames)}>{allNames ? "recent only" : `show all ${data.names.length}`}</span>
          : `${data.names.length} seen`} />
        <div className="ds-panel">
          <div className="ds-chips" style={allNames ? { maxHeight: 280, overflowY: "auto" } : undefined}>
            {names.map((n, i) => (
              <span key={n.name} className="ds-chip text">
                {n.name} <em>{i === 0 && !allNames ? "current · " : ""}{seenRange(n)} · {n.games}g</em>
              </span>
            ))}
            {names.length === 0 && <span className="ds-none">No dated names yet.</span>}
          </div>
        </div>
      </section>

      <section className="ds-sec">
        <SecHead num={2} title="Other accounts" sub="estimate" />
        <div className="ds-banner">Guesses from shared teammates, hero pool, talent habits and play hours — never proof. Confirming only adds a note; the two profiles stay separate.</div>
        {decide.isError && (
          <div className="ds-toast bad">
            {err}
            {err === "admin token required" && <> — set it in <Link to="/admin">Admin</Link></>}
          </div>
        )}
        {suggestions.map((s) => (
          <Suggestion key={s.toon} s={s} busy={decide.isPending} onDecide={(status) => decide.mutate({ other: s.toon, status })} />
        ))}
        {data.suggestions.length === 0 && <div className="ds-empty" style={{ marginTop: 10 }}>No likely other accounts.</div>}
        {data.suggestions.length > SHOWN_SUGGESTIONS && (
          <p style={{ marginTop: 10 }}>
            <span className="ds-pill" onClick={() => setAllSugg(!allSugg)}>{allSugg ? "top 5 only" : `show all ${data.suggestions.length}`}</span>
          </p>
        )}
      </section>

      {data.confirmed.length > 0 && (
        <section className="ds-sec">
          <SecHead num={3} title="Confirmed" sub="marked as the same person" />
          <div className="ds-chips">
            {data.confirmed.map((c) => (
              <Link key={c.toon} to={`/player/${encodeURIComponent(c.toon)}`} className="ds-chip text" style={{ textDecoration: "none", color: "inherit" }}>{c.name ?? c.toon}</Link>
            ))}
          </div>
        </section>
      )}
    </>
  );
}
