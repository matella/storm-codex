import { useEffect, useState, type CSSProperties } from "react";
import { keepPreviousData, useQuery } from "@tanstack/react-query";
import { Link, useSearchParams } from "react-router-dom";
import { fmtTime, searchPlayers, type PlayerHit } from "../api";
import { highlightSegments } from "../players";

const MIN_CHARS = 2;
const pct = (w: number, g: number) => (g ? Math.round((100 * w) / g) : 0);
const initials = (n: string) => n.replace(/[^\p{L}\p{N}]/gu, "").slice(0, 2).toUpperCase() || "?";

function Marked({ text, q }: { text: string; q: string }) {
  return <>{highlightSegments(text, q).map((s, i) => (s.hit ? <mark key={i}>{s.text}</mark> : <span key={i}>{s.text}</span>))}</>;
}

function HitRow({ h, q }: { h: PlayerHit; q: string }) {
  const shown = h.matched_old ? h.name ?? h.matched_name : h.matched_name;
  const wr = pct(h.wins, h.games);
  return (
    <Link to={`/player/${encodeURIComponent(h.toon)}`} className="ds-rm" style={{ marginTop: 10, textDecoration: "none", color: "inherit" }}>
      <span className="ds-pt" style={{ "--s": "44px", "--ring": "var(--accent)" } as CSSProperties}><span className="ds-ini">{initials(shown)}</span></span>
      <div style={{ flex: 1, minWidth: 0 }}>
        <strong>
          {h.matched_old ? shown : <Marked text={shown} q={q} />}
          {h.matched_old && <span className="ds-pill" style={{ marginLeft: 8 }}>was “<Marked text={h.matched_name} q={q} />”</span>}
        </strong>
        <span>{h.roles.length ? h.roles.map(([r]) => r).join(" · ") : "—"}</span>
        <em>{h.games} games · last seen {h.last_seen ? fmtTime(h.last_seen) : "—"}</em>
      </div>
      <div style={{ textAlign: "right" }}>
        <strong style={{ color: wr >= 55 ? "var(--win)" : wr < 45 ? "var(--loss)" : undefined }}>{wr}%</strong>
        <em>win rate</em>
      </div>
    </Link>
  );
}

/** Players — recherche par pseudo actuel, ancien pseudo ou fragment (`?q=` dans l'URL). */
export function Players() {
  const [sp, setSp] = useSearchParams();
  const q = sp.get("q") ?? "";
  const [text, setText] = useState(q);
  // debounce : l'URL (donc la requête) suit la saisie après une courte pause
  useEffect(() => {
    const t = setTimeout(() => {
      if (text !== q) setSp(text ? { q: text } : {}, { replace: true });
    }, 250);
    return () => clearTimeout(t);
  }, [text, q, setSp]);
  const term = q.trim();
  const enabled = term.length >= MIN_CHARS;
  const { data, isFetching, isError } = useQuery({
    queryKey: ["players-search", term],
    queryFn: () => searchPlayers(term),
    enabled,
    placeholderData: keepPreviousData,
  });
  const hits = enabled ? data ?? [] : [];

  return (
    <div className="ds-page">
      <div className="ds-crumb">Players</div>
      <div className="ds-kicker">Search</div>
      <h1 className="ds-title" style={{ cursor: "default", fontSize: "clamp(40px, 6vw, 72px)" }}>Players</h1>
      <div className="ds-subtitle" style={{ cursor: "default" }}>Current names, old names, or part of a name — only players seen in your replays.</div>
      <input
        className="ds-input" style={{ width: "100%", marginTop: 18, fontSize: 16 }} autoFocus
        placeholder="Search a player…" value={text} onChange={(e) => setText(e.target.value)} aria-label="Search a player"
      />
      {!enabled ? (
        <div className="ds-empty" style={{ marginTop: 18 }}>Type at least {MIN_CHARS} characters.</div>
      ) : isError ? (
        <div className="ds-empty" style={{ marginTop: 18 }}>Search failed. Try again.</div>
      ) : (
        <>
          <span className="ds-label" style={{ marginTop: 16 }}>{isFetching && !data ? "searching…" : `${hits.length} player${hits.length === 1 ? "" : "s"} found`}</span>
          {hits.map((h) => <HitRow key={h.toon} h={h} q={term} />)}
          {data && hits.length === 0 && <div className="ds-empty" style={{ marginTop: 10 }}>No player matches “{term}”.</div>}
        </>
      )}
    </div>
  );
}
