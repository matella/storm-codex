// Onglet « Replay 2D » : charge le modèle de visionneuse (positions normalisées [0,1]) une fois,
// puis scrub 100% côté client — pas de requête réseau par déplacement du curseur (seek(t) pur, cf.
// replay2d.ts). Play/pause + vitesse animent `t` en temps réel via usePlayback (US-11).
import { useEffect, useMemo, useRef, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { fetchReplay2d, mapImage, universeColor, heroIcon, initials } from "../api";
import { sampleAt, deathsNear } from "../replay2d";
import { advance, usePlayback } from "../usePlayback";
import { Avatar } from "./Avatar";

const CANVAS_SIZE = 640;
const HERO_R = 10;
const SPEEDS = [0.5, 1, 2, 4, 8];

function fmtClock(t: number): string {
  const m = Math.floor(t / 60);
  const s = Math.floor(t % 60);
  return `${m}:${String(s).padStart(2, "0")}`;
}

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
  const [t, setT] = useState(0);
  const [mapBroken, setMapBroken] = useState(false);
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const [tick, bumpRedraw] = useState(0); // incrémenté quand un portrait finit de charger → redessine
  const duration = data?.meta.durationSec || 0;

  const pb = usePlayback({
    onTick: (dt) => {
      setT((prev) => {
        const r = advance(prev, dt, pb.speed, duration);
        if (!r.playing) pb.pause(); // fin de clip atteinte → coupe la boucle rAF
        return r.t;
      });
    },
  });

  // Couleurs d'équipe résolues une fois depuis les tokens CSS (le canvas ne comprend pas var(...)).
  const teamColor = useMemo(() => [resolveColor("var(--tm-blue)"), resolveColor("var(--tm-red)")], []);

  const playerByPlayerId = useMemo(() => {
    const m = new Map<number, { name: string | null; hero: string | null; team: number | null }>();
    if (data) for (const p of data.players) m.set(p.playerId, p);
    return m;
  }, [data]);

  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas || !data) return;
    const ctx = canvas.getContext("2d");
    if (!ctx) return;
    const W = canvas.width, H = canvas.height;
    ctx.clearRect(0, 0, W, H);

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

      if (!p.alive) {
        ctx.strokeStyle = "#e8eaf2";
        ctx.lineWidth = 1.5;
        const s = 5;
        ctx.beginPath();
        ctx.moveTo(cx - s, cy - s); ctx.lineTo(cx + s, cy + s);
        ctx.moveTo(cx + s, cy - s); ctx.lineTo(cx - s, cy + s);
        ctx.stroke();
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
      </div>
      <div style={{ display: "flex", alignItems: "center", gap: 10, marginTop: 10 }}>
        <span
          className="pill on"
          role="button"
          aria-label={pb.playing ? "pause" : "play"}
          onClick={pb.toggle}
          style={{ minWidth: 28, textAlign: "center" }}
        >
          {pb.playing ? "⏸" : "▶"}
        </span>
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
    </div>
  );
}
