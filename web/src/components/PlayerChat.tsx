import { useQuery } from "@tanstack/react-query";
import { fetchChat } from "../api";
import { chatSummary } from "../players";

const pct = (x: number) => `${Math.round(x * 100)}%`;

/** Habitudes de chat d'un compte : mesures seulement (jamais le texte). `compact` = une ligne. */
export function PlayerChat({ toon, compact }: { toon: string; compact?: boolean }) {
  const { data, isError } = useQuery({ queryKey: ["chat", toon], queryFn: () => fetchChat(toon), retry: false });
  if (isError || !data) return null; // compte absent du corpus : rien à dire
  const summary = chatSummary(data);
  if (compact) return summary ? <div className="ds-pstats" title="From your own archived games with this account">Chat <b>{summary}</b> · from your games</div> : null;
  if (!data.enough) return <div className="ds-empty">Not enough games ({data.games}) for chat habits.</div>;
  const t = data.timing;
  return (
    <div className="ds-panel">
      <div className="ds-statline">
        <div className="ds-stat"><span className="ds-label">Messages</span><strong>{data.messages}</strong><small>{data.per_game.toFixed(1)} per game</small></div>
        <div className="ds-stat"><span className="ds-label">Team chat</span><strong>{pct(data.team_share)}</strong><small>all chat {pct(data.all_share)}</small></div>
        <div className="ds-stat"><span className="ds-label">When</span><strong>{pct(t.early)}<span>/</span>{pct(t.mid)}<span>/</span>{pct(t.late)}</strong><small>early / mid / late</small></div>
      </div>
      {data.top_words.length > 0 && (
        <div className="ds-chips" style={{ marginTop: 12 }}>
          {data.top_words.map((w) => <span key={w.word} className="ds-chip text">{w.word} <em>×{w.count}</em></span>)}
        </div>
      )}
    </div>
  );
}
