// Éditeur d'une équipe dans le rail de la régie : nom, logo (fichier ou URL), bilan, 5 joueurs
// (pseudo + `PlayerPicker` pour lier un toon, tagline, capitaine).
import { useState } from "react";
import type { Side } from "../api";
import type { CastTeam } from "../castApi";
import { PlayerPicker } from "./PlayerPicker";

export function TeamEditor({ side, team: t, onEdit, onUpload, onLogoTyped }: {
  side: Side;
  team: CastTeam;
  onEdit: (t: CastTeam) => void;
  onUpload: (file: File) => void;
  /** L'opérateur a saisi lui-même l'URL du logo (elle l'emporte alors sur le logo du serveur). */
  onLogoTyped: () => void;
}) {
  // URL du logo qui n'a pas pu s'afficher (une autre URL réessaie d'elle-même)
  const [badLogo, setBadLogo] = useState<string | null>(null);
  const setPlayer = (i: number, patch: Partial<CastTeam["players"][number]>) =>
    onEdit({ ...t, players: t.players.map((q, j) => (j === i ? { ...q, ...patch } : q)) });
  return (
    <fieldset className={`cr-team tone-${side}`}>
      <legend>{side === "blue" ? "Blue" : "Red"}</legend>
      <input className="ds-input" placeholder={side === "blue" ? "Team Blue" : "Team Red"} value={t.name}
        onChange={(e) => onEdit({ ...t, name: e.target.value })} aria-label={`${side} team name`} />
      <div className="cr-logo">
        {t.logo && t.logo !== badLogo
          ? <img src={t.logo} alt="" onError={() => setBadLogo(t.logo)} />
          : <span className="cr-nologo">no logo</span>}
        <input type="file" accept="image/png,image/jpeg,image/webp" aria-label={`${side} logo file`}
          onChange={(e) => {
            const f = e.target.files?.[0];
            if (f) onUpload(f);
            e.target.value = "";
          }} />
      </div>
      <input className="ds-input" placeholder="logo URL (optional)" value={t.logo ?? ""} aria-label={`${side} logo URL`}
        onChange={(e) => { onLogoTyped(); onEdit({ ...t, logo: e.target.value.trim() === "" ? null : e.target.value }); }} />
      <input className="ds-input" placeholder="record (e.g. 4-1)" value={t.record} aria-label={`${side} record`}
        onChange={(e) => onEdit({ ...t, record: e.target.value })} />
      {t.players.map((p, i) => (
        <div key={i} className="cr-player">
          <PlayerPicker player={p} label={`${side} player ${i + 1}`} onChange={(patch) => setPlayer(i, patch)} />
          <div className="cr-pl2">
            <input className="ds-input" placeholder="tagline" value={p.tagline} aria-label={`${side} player ${i + 1} tagline`}
              onChange={(e) => setPlayer(i, { tagline: e.target.value })} />
            <button type="button" className={`ds-btn small${t.captain === i ? " on" : ""}`} title="Captain" aria-pressed={t.captain === i}
              onClick={() => onEdit({ ...t, captain: t.captain === i ? null : i })}>C</button>
          </div>
        </div>
      ))}
    </fieldset>
  );
}
