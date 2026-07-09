// Onglet « Replay 2D » : charge le modèle de visionneuse (positions normalisées [0,1]) une fois,
// puis scrub 100% côté client — pas de requête réseau par déplacement du curseur (seek(t) pur, cf.
// replay2d.ts). Play/pause + vitesse animent `t` en temps réel via usePlayback (US-11).
import { useEffect, useMemo, useRef, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { fetchReplay2d, fetchMatch, mapImage, universeColor, heroIcon, initials, useDimTalents, talentInfo } from "../api";
import { sampleAt, deathsNear, castFlash, type FeedEvent, type Objective } from "../replay2d";
import { advance, usePlayback } from "../usePlayback";
import { Avatar } from "./Avatar";
import { TalentStrip2D } from "./TalentStrip2D";

// US-19 : un pick de talent reste "récent" (badge sur le portrait) pendant cette fenêtre après t.
const TALENT_MARKER_WINDOW_SEC = 3;

const CANVAS_SIZE = 640;
const HERO_R = 10;
const SPEEDS = [0.5, 1, 2, 4, 8];

function fmtClock(t: number): string {
  const m = Math.floor(t / 60);
  const s = Math.floor(t % 60);
  return `${m}:${String(s).padStart(2, "0")}`;
}

const TEAM_NAME = ["Blue", "Red"];

/** Libellé affiché (avec emoji) d'un event du kill-feed, résolu FRONT-side via `players[]`. */
function feedLabel(e: FeedEvent, heroFor: (playerId: number | null) => string | null): string {
  if (e.kind === "takedown") {
    const killer = heroFor(e.killerPlayerId) ?? "Unknown";
    const victim = heroFor(e.victimPlayerId) ?? "Unknown";
    return `💀 ${killer} → ${victim}`;
  }
  if (e.kind === "structure") {
    const team = e.team === 1 ? TEAM_NAME[1] : e.team === 0 ? TEAM_NAME[0] : "";
    return `🏰 ${team} ${e.structureKind ?? "structure"}`.trim();
  }
  if (e.kind === "camp") return "🏕️ Camp captured";
  return e.kind;
}

/** aria-label textuel (sans emoji) : event + timecode mm:ss. */
function feedAriaLabel(e: FeedEvent, heroFor: (playerId: number | null) => string | null): string {
  const time = fmtClock(e.t);
  if (e.kind === "takedown") {
    const killer = heroFor(e.killerPlayerId) ?? "unknown hero";
    const victim = heroFor(e.victimPlayerId) ?? "unknown hero";
    return `Takedown: ${killer} killed ${victim} at ${time}`;
  }
  if (e.kind === "structure") {
    const team = e.team === 1 ? TEAM_NAME[1] : e.team === 0 ? TEAM_NAME[0] : "Unknown";
    return `Structure destroyed: ${team} ${e.structureKind ?? "structure"} at ${time}`;
  }
  if (e.kind === "camp") return `Jungle camp captured at ${time}`;
  return `${e.kind} at ${time}`;
}

// US-21..24 : `Objective` est structuré côté crate (pas de texte) — c'est ICI qu'on compose le
// libellé affiché. V1 : seul "zerg_wave" (Braxis) a un libellé dédié ; toute autre `kind` retombe
// sur un rendu générique plutôt que de planter/rien afficher (une future carte objectif dégrade
// proprement sans changement front).
function objectiveLabel(o: Objective): string {
  if (o.kind === "zerg_wave") return `⚡ Zerg wave${o.value !== null ? ` (${o.value} units)` : ""}`;
  return `📌 ${o.kind}`;
}

function objectiveAriaLabel(o: Objective): string {
  const time = fmtClock(o.t);
  if (o.kind === "zerg_wave") {
    const units = o.value !== null ? ` (${o.value} units)` : "";
    return `Zerg wave${units} at ${time}`;
  }
  return `${o.kind} at ${time}`;
}

/** Réunion event/objectif triée par t — une seule liste de feed, un seul rendu. */
type FeedRow =
  | { source: "event"; t: number; event: FeedEvent }
  | { source: "objective"; t: number; objective: Objective };

/** Résout une valeur `var(--x)` en couleur calculée (le canvas ne comprend pas les custom properties
 *  CSS) ; passe au travers si `v` est déjà une couleur littérale. */
function resolveColor(v: string): string {
  if (!v.startsWith("var(")) return v;
  const name = v.slice(4, -1).trim();
  const val = getComputedStyle(document.documentElement).getPropertyValue(name).trim();
  return val || "#afa9ec";
}

// Cache module-level des portraits chargés (évite de recréer une Image() à chaque frame de scrub).
const iconCache = new Map<string, HTMLImageElement>();
function loadIcon(url: string, onLoad: () => void): HTMLImageElement {
  let img = iconCache.get(url);
  if (!img) {
    img = new Image();
    img.onload = onLoad;
    img.src = url;
    iconCache.set(url, img);
  }
  return img;
}

export function Replay2D({ id }: { id: string }) {
  const { data, isLoading } = useQuery({
    queryKey: ["replay2d", id],
    queryFn: () => fetchReplay2d(id),
    staleTime: Infinity, // replay décodé d'un match fini = immuable (comme dim-heroes/dim-talents)
  });
  // US-27 : résolution best-effort des NOMS de talent pour la bande (US-19/US-27) — la
  // visionneuse elle-même (positions/casts/morts) ne dépend d'AUCUN de ces deux appels ; s'ils
  // échouent ou sont vides, la bande retombe sur "Tier N" (cf. TalentStrip2D). Même queryKey que
  // MatchDetail (["match", id]) → réutilise le cache/la requête en vol, pas de double fetch.
  useDimTalents();
  const { data: matchData } = useQuery({
    queryKey: ["match", id],
    queryFn: () => fetchMatch(id),
    staleTime: Infinity,
  });
  // Clé par NOM de joueur (battletag), PAS par héros : un match ARAM peut avoir deux fois le même
  // héros (miroir) → une clé héros afficherait les talents de l'autre joueur. Le nom est unique
  // par match ; replay2d `players[]` porte le même `name`, donc la jointure est sûre. Si un nom
  // ne matche pas, dégradation gracieuse (Tier N / pas de nom) comme avant.
  const talentsByName = useMemo(() => {
    const m = new Map<string, Record<string, string>>();
    const players = matchData?.players;
    if (players)
      for (const p of Object.values(players) as any[]) {
        if (p?.name && p?.talents) m.set(p.name, p.talents as Record<string, string>);
      }
    return m;
  }, [matchData]);
  const talentNameFor = (name: string | null, tier: number): string | null => {
    if (!name) return null;
    const treeId = talentsByName.get(name)?.[`Tier${tier}Choice`];
    return treeId ? talentInfo(treeId)?.name ?? null : null;
  };
  const [t, setT] = useState(0);
  const [mapBroken, setMapBroken] = useState(false);
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const [tick, bumpRedraw] = useState(0); // incrémenté quand un portrait finit de charger → redessine
  const duration = data?.meta.durationSec || 0;

  // Miroir de `t` lu dans onTick : évite un effet de bord (pause) dans l'updater de setT, qui doit
  // rester pur (StrictMode double-invoque les updaters en dev → pause() jouée deux fois).
  const tRef = useRef(0);
  useEffect(() => { tRef.current = t; }, [t]);

  const pb = usePlayback({
    onTick: (dt) => {
      const r = advance(tRef.current, dt, pb.speed, duration);
      setT(r.t);
      if (!r.playing) pb.pause(); // fin de clip atteinte → coupe la boucle rAF
    },
  });

  // Couleurs d'équipe résolues une fois depuis les tokens CSS (le canvas ne comprend pas var(...)).
  const teamColor = useMemo(() => [resolveColor("var(--tm-blue)"), resolveColor("var(--tm-red)")], []);

  const playerByPlayerId = useMemo(() => {
    const m = new Map<number, { name: string | null; hero: string | null; team: number | null }>();
    if (data) for (const p of data.players) m.set(p.playerId, p);
    return m;
  }, [data]);

  // US-21..24 : events + objectifs fusionnés en une seule liste de feed, triée par t — un
  // objectif (ex. vague zerg Braxis) apparaît chronologiquement parmi les takedowns/structures.
  const feedRows = useMemo((): FeedRow[] => {
    const rows: FeedRow[] = [
      ...(data?.events ?? []).map((event): FeedRow => ({ source: "event", t: event.t, event })),
      ...(data?.objectives ?? []).map((objective): FeedRow => ({ source: "objective", t: objective.t, objective })),
    ];
    rows.sort((a, b) => a.t - b.t);
    return rows;
  }, [data]);

  // Index de la ligne la plus proche de (et ≤) t courant, pour le surlignage du kill-feed.
  const highlightIdx = useMemo(() => {
    let idx = -1;
    for (let i = 0; i < feedRows.length; i++) {
      if (feedRows[i].t <= t) idx = i;
      else break;
    }
    return idx;
  }, [feedRows, t]);

  const feedRowRefs = useRef<(HTMLButtonElement | null)[]>([]);
  const feedListRef = useRef<HTMLDivElement>(null);
  useEffect(() => {
    // Scroll confiné au conteneur de la liste (jamais scrollIntoView, qui remonterait tous les
    // ancêtres scrollables, page incluse, et déplacerait la page pendant la lecture).
    const container = feedListRef.current;
    const row = feedRowRefs.current[highlightIdx];
    if (!container || !row) return;
    const top = row.offsetTop;
    const bottom = top + row.offsetHeight;
    if (top < container.scrollTop) container.scrollTop = top;
    else if (bottom > container.scrollTop + container.clientHeight)
      container.scrollTop = bottom - container.clientHeight;
  }, [highlightIdx]);

  const heroForPlayer = (playerId: number | null): string | null =>
    playerId === null ? null : playerByPlayerId.get(playerId)?.hero ?? null;

  /** Couleur d'équipe d'une ligne du kill-feed : victime pour un takedown, équipe propriétaire
   *  pour une structure/objectif ; neutre (muted) pour un camp / team inconnue. */
  const feedRowColor = (row: FeedRow): string => {
    if (row.source === "objective") {
      const team = row.objective.team;
      return team === 1 ? teamColor[1] : team === 0 ? teamColor[0] : "var(--muted-2)";
    }
    const e = row.event;
    if (e.kind === "takedown") {
      const team = playerByPlayerId.get(e.victimPlayerId ?? -1)?.team;
      return team === 1 ? teamColor[1] : team === 0 ? teamColor[0] : "var(--muted-2)";
    }
    if (e.kind === "structure") {
      return e.team === 1 ? teamColor[1] : e.team === 0 ? teamColor[0] : "var(--muted-2)";
    }
    return "var(--muted-2)";
  };

  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas || !data) return;
    const ctx = canvas.getContext("2d");
    if (!ctx) return;
    const W = canvas.width, H = canvas.height;
    ctx.clearRect(0, 0, W, H);

    // Structures d'abord (sous les héros) : petits carrés/losanges couleur d'équipe, le core plus
    // grand ; détruite (à t courant) → grisée + croix. "other" (ex. HallOfStormsLocationUnit) sauté
    // pour réduire le bruit visuel.
    for (const s of data.structures) {
      if (s.kind === "other") continue;
      const cx = s.x * W, cy = (1 - s.y) * H;
      const destroyed = s.destroyedAt !== null && t >= s.destroyedAt;
      const size = s.kind === "core" ? 9 : 5;
      ctx.fillStyle = destroyed ? "#5a5d6b" : teamColor[s.team === 1 ? 1 : 0];
      ctx.fillRect(cx - size / 2, cy - size / 2, size, size);
      if (destroyed) {
        ctx.strokeStyle = "#e8eaf2";
        ctx.lineWidth = 1;
        const s2 = size / 2 + 1;
        ctx.beginPath();
        ctx.moveTo(cx - s2, cy - s2); ctx.lineTo(cx + s2, cy + s2);
        ctx.moveTo(cx + s2, cy - s2); ctx.lineTo(cx - s2, cy + s2);
        ctx.stroke();
      }
    }

    for (const track of data.heroes) {
      const p = sampleAt(track, t);
      if (!p) continue;
      const meta = playerByPlayerId.get(track.playerId);
      const team = meta?.team === 1 ? 1 : 0;
      const cx = p.x * W;
      const cy = (1 - p.y) * H; // flip Y : le monde monte (y grandit vers le haut), le canvas descend

      ctx.globalAlpha = p.alive ? 1 : 0.4;

      // anneau d'équipe (extérieur, identité d'équipe toujours visible même avec portrait)
      ctx.beginPath();
      ctx.arc(cx, cy, HERO_R + 3, 0, Math.PI * 2);
      ctx.lineWidth = 2;
      ctx.strokeStyle = teamColor[team];
      ctx.stroke();

      // remplissage : portrait clippé en cercle si chargé, sinon pastille couleur d'équipe + initiales
      const iconUrl = meta?.hero ? heroIcon(meta.hero) : null;
      let drewPortrait = false;
      if (iconUrl) {
        const img = loadIcon(iconUrl, () => bumpRedraw((n) => n + 1));
        if (img.complete && img.naturalWidth > 0) {
          ctx.save();
          ctx.beginPath();
          ctx.arc(cx, cy, HERO_R, 0, Math.PI * 2);
          ctx.clip();
          ctx.drawImage(img, cx - HERO_R, cy - HERO_R, HERO_R * 2, HERO_R * 2);
          ctx.restore();
          drewPortrait = true;
        }
      }
      if (!drewPortrait) {
        ctx.beginPath();
        ctx.arc(cx, cy, HERO_R, 0, Math.PI * 2);
        ctx.fillStyle = teamColor[team];
        ctx.fill();
        if (meta?.hero) {
          ctx.fillStyle = "#0b0c11";
          ctx.font = "bold 8px sans-serif";
          ctx.textAlign = "center";
          ctx.textBaseline = "middle";
          ctx.fillText(initials(meta.hero), cx, cy);
        }
      }

      // anneau d'univers héros (par-dessus, comme <Avatar>)
      ctx.beginPath();
      ctx.arc(cx, cy, HERO_R, 0, Math.PI * 2);
      ctx.lineWidth = 1.5;
      ctx.strokeStyle = resolveColor(universeColor(meta?.hero ?? null));
      ctx.stroke();

      // flash de cast d'aptitude (US-18) : anneau blanc qui pulse puis s'estompe, ~0,6s après
      // chaque cast — subtil, pas de tentative d'identifier l'aptitude jouée.
      const flash = castFlash(track.casts, t);
      if (flash > 0) {
        ctx.beginPath();
        ctx.arc(cx, cy, HERO_R + 5, 0, Math.PI * 2);
        ctx.lineWidth = 2;
        ctx.strokeStyle = `rgba(255, 255, 255, ${flash})`;
        ctx.stroke();
      }

      if (!p.alive) {
        ctx.strokeStyle = "#e8eaf2";
        ctx.lineWidth = 1.5;
        const s = 5;
        ctx.beginPath();
        ctx.moveTo(cx - s, cy - s); ctx.lineTo(cx + s, cy + s);
        ctx.moveTo(cx + s, cy - s); ctx.lineTo(cx - s, cy + s);
        ctx.stroke();
      }

      // US-19 : badge de pick de talent récent (fenêtre fixe, pas de fondu — juste "un pick a eu
      // lieu près de maintenant"). Fenêtre UNILATÉRALE : uniquement APRÈS le pick (0 ≤ dt ≤ 3s),
      // pas avant — plus intuitif en lecture/scrub avant. Le timing = le contrat ; le nom est
      // résolu à part (bande).
      const recentTalent = track.talents.find((tp) => {
        const dt = t - tp.t;
        return dt >= 0 && dt <= TALENT_MARKER_WINDOW_SEC;
      });
      if (recentTalent) {
        const bx = cx + HERO_R + 2, by = cy - HERO_R - 2;
        ctx.beginPath();
        ctx.arc(bx, by, 6, 0, Math.PI * 2);
        ctx.fillStyle = "#f5c542";
        ctx.fill();
        ctx.strokeStyle = "#1a1500";
        ctx.lineWidth = 1;
        ctx.stroke();
        ctx.fillStyle = "#1a1500";
        ctx.font = "bold 8px sans-serif";
        ctx.textAlign = "center";
        ctx.textBaseline = "middle";
        ctx.fillText("↑", bx, by + 0.5);
      }
    }
    ctx.globalAlpha = 1;

    for (const d of deathsNear(data.deaths, t)) {
      const cx = d.x * W, cy = (1 - d.y) * H;
      ctx.strokeStyle = teamColor[1];
      ctx.lineWidth = 2;
      const s = 6;
      ctx.beginPath();
      ctx.moveTo(cx - s, cy - s); ctx.lineTo(cx + s, cy + s);
      ctx.moveTo(cx + s, cy - s); ctx.lineTo(cx - s, cy + s);
      ctx.stroke();
    }
  }, [t, data, playerByPlayerId, teamColor, tick]);

  if (isLoading) return <div className="empty">loading…</div>;
  if (!data) return <div className="empty">replay unavailable</div>;

  const bg = !mapBroken ? mapImage(data.meta.mapName) : null;

  return (
    <div className="card" style={{ padding: 14 }}>
      {data.warnings.length > 0 && (
        <div style={{ marginBottom: 10 }}>
          {data.warnings.map((w, i) => (
            <div
              key={i}
              style={{
                fontSize: 11,
                color: "var(--muted-2)",
                background: "var(--hairline-strong)",
                borderRadius: 6,
                padding: "5px 8px",
                marginBottom: 4,
              }}
            >
              ⚠️ {w}
            </div>
          ))}
        </div>
      )}
      <div style={{ display: "flex", gap: 18, flexWrap: "wrap" }}>
        <div
          style={{
            position: "relative",
            width: "min(100%, 560px)",
            aspectRatio: "1 / 1",
            borderRadius: 8,
            overflow: "hidden",
            background: "linear-gradient(135deg, #1a1d2a, #232636)",
            flex: "1 1 360px",
          }}
        >
          {bg && (
            <img
              src={bg}
              alt=""
              onError={() => setMapBroken(true)}
              style={{ position: "absolute", inset: 0, width: "100%", height: "100%", objectFit: "cover" }}
            />
          )}
          <canvas
            ref={canvasRef}
            width={CANVAS_SIZE}
            height={CANVAS_SIZE}
            style={{ position: "absolute", inset: 0, width: "100%", height: "100%" }}
          />
        </div>
        <div style={{ minWidth: 180, flex: "0 0 200px" }}>
          <p className="cap" style={{ margin: "0 0 8px" }}>Players</p>
          {[0, 1].map((team) => (
            <div key={team} style={{ marginBottom: 10 }}>
              <span className={team === 0 ? "tm-blue" : "tm-red"} style={{ fontSize: 10 }}>
                {team === 0 ? "Blue team" : "Red team"}
              </span>
              {data.players.filter((p) => p.team === team).map((p) => (
                <div key={p.playerId} style={{ display: "flex", alignItems: "center", gap: 6, padding: "3px 0" }}>
                  <Avatar hero={p.hero} size={18} />
                  <span style={{ fontSize: 11 }}>{p.name ?? "—"}</span>
                  <span className="muted" style={{ fontSize: 10 }}>{p.hero}</span>
                </div>
              ))}
            </div>
          ))}
        </div>
        <div style={{ minWidth: 200, flex: "1 1 220px", maxWidth: 280 }}>
          <p className="cap" style={{ margin: "0 0 8px" }}>Kill feed</p>
          <div
            ref={feedListRef}
            style={{
              maxHeight: 260,
              overflowY: "auto",
              display: "flex",
              flexDirection: "column",
              gap: 2,
            }}
          >
            {feedRows.map((row, i) => (
              <button
                key={i}
                type="button"
                ref={(el) => { feedRowRefs.current[i] = el; }}
                aria-label={
                  row.source === "objective"
                    ? objectiveAriaLabel(row.objective)
                    : feedAriaLabel(row.event, heroForPlayer)
                }
                onClick={() => { pb.pause(); setT(row.t); }}
                style={{
                  display: "flex",
                  justifyContent: "space-between",
                  alignItems: "center",
                  gap: 8,
                  textAlign: "left",
                  fontFamily: "inherit",
                  fontSize: 11,
                  padding: "4px 6px",
                  borderRadius: 6,
                  border: "1px solid transparent",
                  borderColor: i === highlightIdx ? feedRowColor(row) : "transparent",
                  background: i === highlightIdx ? "var(--hairline-strong)" : "transparent",
                  color: "inherit",
                  cursor: "pointer",
                }}
              >
                <span style={{ color: feedRowColor(row) }}>
                  {row.source === "objective" ? objectiveLabel(row.objective) : feedLabel(row.event, heroForPlayer)}
                </span>
                <span className="mono muted" style={{ fontSize: 10, flexShrink: 0 }}>{fmtClock(row.t)}</span>
              </button>
            ))}
          </div>
        </div>
      </div>
      <div style={{ display: "flex", alignItems: "center", gap: 10, marginTop: 10 }}>
        <button
          type="button"
          className="pill on"
          aria-label={pb.playing ? "pause" : "play"}
          onClick={pb.toggle}
          style={{ minWidth: 28, textAlign: "center", fontFamily: "inherit", lineHeight: "inherit" }}
        >
          {pb.playing ? "⏸" : "▶"}
        </button>
        <select
          aria-label="playback speed"
          value={pb.speed}
          onChange={(e) => pb.setSpeed(Number(e.target.value))}
          style={{ fontSize: 11, background: "transparent", color: "var(--muted-2)", border: "1px solid var(--hairline-strong)", borderRadius: 12, padding: "3px 6px" }}
        >
          {SPEEDS.map((s) => (
            <option key={s} value={s}>{s}×</option>
          ))}
        </select>
        <input
          type="range"
          min={0}
          max={duration}
          step={0.1}
          value={t}
          onChange={(e) => { pb.pause(); setT(Number(e.target.value)); }}
          aria-label="replay time"
          aria-valuetext={fmtClock(t)}
          style={{ flex: 1 }}
        />
        <span className="mono muted" style={{ fontSize: 11, minWidth: 90, textAlign: "right" }}>
          {fmtClock(t)} / {fmtClock(duration)}
        </span>
      </div>
      <TalentStrip2D
        heroes={data.heroes}
        players={data.players}
        levels={data.levels}
        t={t}
        talentNameFor={talentNameFor}
      />
    </div>
  );
}
