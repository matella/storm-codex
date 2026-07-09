// Onglet « Replay 2D » : charge le modèle de visionneuse (positions normalisées [0,1]) une fois,
// puis scrub 100% côté client — pas de requête réseau par déplacement du curseur (seek(t) pur, cf.
// replay2d.ts). Pas d'animation play/pause en MVP-1, juste une barre de scrub.
import { useEffect, useMemo, useRef, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { fetchReplay2d, mapImage, universeColor, heroIcon, initials } from "../api";
import { sampleAt, deathsNear } from "../replay2d";
import { Avatar } from "./Avatar";

// Miroir de --tm-blue / --tm-red (theme.css) : le contexte canvas 2D ne résout pas var(...).
const TEAM_COLOR = ["#85b7eb", "#f09595"];
const CANVAS_SIZE = 640;
const HERO_R = 10;

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
  const { data, isLoading } = useQuery({ queryKey: ["replay2d", id], queryFn: () => fetchReplay2d(id) });
  const [t, setT] = useState(0);
  const [mapBroken, setMapBroken] = useState(false);
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const [, bumpRedraw] = useState(0); // forcé quand un portrait finit de charger en arrière-plan

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
      ctx.strokeStyle = TEAM_COLOR[team];
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
        ctx.fillStyle = TEAM_COLOR[team];
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
      ctx.strokeStyle = "#f09595";
      ctx.lineWidth = 2;
      const s = 6;
      ctx.beginPath();
      ctx.moveTo(cx - s, cy - s); ctx.lineTo(cx + s, cy + s);
      ctx.moveTo(cx + s, cy - s); ctx.lineTo(cx - s, cy + s);
      ctx.stroke();
    }
  }, [t, data, playerByPlayerId]);

  if (isLoading) return <div className="empty">loading…</div>;
  if (!data) return <div className="empty">replay indisponible</div>;

  const bg = !mapBroken ? mapImage(data.meta.mapName) : null;
  const duration = data.meta.durationSec || 0;

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
          <p className="cap" style={{ margin: "0 0 8px" }}>Joueurs</p>
          {[0, 1].map((team) => (
            <div key={team} style={{ marginBottom: 10 }}>
              <span className={team === 0 ? "tm-blue" : "tm-red"} style={{ fontSize: 10 }}>
                {team === 0 ? "équipe bleue" : "équipe rouge"}
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
        <input
          type="range"
          min={0}
          max={duration}
          step={0.1}
          value={t}
          onChange={(e) => setT(Number(e.target.value))}
          style={{ flex: 1 }}
        />
        <span className="mono muted" style={{ fontSize: 11, minWidth: 90, textAlign: "right" }}>
          {fmtClock(t)} / {fmtClock(duration)}
        </span>
      </div>
    </div>
  );
}
