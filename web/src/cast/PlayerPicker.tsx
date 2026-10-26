// Sélecteur de joueur de la régie : pseudo saisi → GET /api/players/search → liste (pseudo, parties,
// dernière vue) → l'opérateur choisit. Aucun résultat = joueur « non lié » (le pseudo saisi reste).
// Le premier résultat n'est JAMAIS pris automatiquement.
import { useState } from "react";
import { fmtTime, searchPlayers } from "../api";
import type { PlayerHit } from "../api";
import type { CastPlayer } from "../castApi";

/** Ce que le choix d'un résultat écrit dans le joueur (toon + pseudo actuel). */
export function pickPatch(h: PlayerHit): Pick<CastPlayer, "toon" | "name"> {
  return { toon: h.toon, name: h.name ?? h.matched_name };
}

export function linkLabel(p: CastPlayer): string {
  return p.toon ? "linked" : "unlinked";
}

type Found = { q: string; hits: PlayerHit[] } | "loading" | "error" | null;

export function PlayerPicker({ player, label, onChange }: {
  player: CastPlayer;
  label: string;
  onChange: (patch: Partial<CastPlayer>) => void;
}) {
  const [found, setFound] = useState<Found>(null);
  const q = player.name.trim();
  const search = async () => {
    if (q.length < 2) return;
    setFound("loading");
    try {
      setFound({ q, hits: await searchPlayers(q) });
    } catch {
      setFound("error");
    }
  };
  return (
    <div className="cr-picker">
      <div className="cr-pk-row">
        <input className="ds-input" placeholder="pseudo" value={player.name} aria-label={`${label} name`}
          onChange={(e) => onChange({ name: e.target.value, toon: null })} // retaper un pseudo rompt le lien (jamais de toon périmé)
          onKeyDown={(e) => { if (e.key === "Enter") void search(); }} />
        <button type="button" className="ds-btn small" disabled={q.length < 2 || found === "loading"}
          aria-label={`${label} find player`} onClick={() => void search()}>Find</button>
      </div>
      <div className={`cr-pk-st${player.toon ? " on" : ""}`}>
        {linkLabel(player)}
        {player.toon && (
          <button type="button" className="ds-btn small" aria-label={`${label} unlink`}
            onClick={() => { onChange({ toon: null }); setFound(null); }}>unlink</button>
        )}
      </div>
      {found === "loading" && <div className="cr-note">searching…</div>}
      {found === "error" && <div className="cr-note">search failed — the player stays {linkLabel(player)}</div>}
      {found && typeof found === "object" && (
        found.hits.length === 0
          ? <div className="cr-note">No player found for &quot;{found.q}&quot; — stays unlinked (typed name kept).</div>
          : (
            <ul className="cr-pk-list" aria-label={`${label} results`}>
              {found.hits.map((h) => (
                <li key={h.toon}>
                  <button type="button" className="cr-pk-hit" onClick={() => { onChange(pickPatch(h)); setFound(null); }}>
                    <b>{h.name ?? h.matched_name}</b>
                    {h.matched_old && <i> was {h.matched_name}</i>}
                    <span>{h.games} games · {fmtTime(h.last_seen)}</span>
                  </button>
                </li>
              ))}
            </ul>
          )
      )}
    </div>
  );
}
