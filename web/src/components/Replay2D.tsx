// Onglet « Replay 2D » : charge le modèle de visionneuse (positions normalisées [0,1]) une fois,
// puis scrub 100% côté client — pas de requête réseau par déplacement du curseur (seek(t) pur, cf.
// replay2d.ts). Play/pause + vitesse animent `t` en temps réel via usePlayback (US-11).
import { useEffect, useMemo, useRef, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { fetchReplay2d, fetchMatch, mapImage, minimapImage, mapSlug, universeColor, heroIcon, initials, useDimTalents, talentInfo } from "../api";
import { sampleAt, deathsNear, castFlash, minionsNear, type FeedEvent, type Objective } from "../replay2d";
import { advance, usePlayback } from "../usePlayback";
import { clipFrames, downloadBlob, recordCanvasStream, supportsClipExport } from "../clipExport";
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
// Calibration par carte : le repère de coords du jeu est tourné/mis à l'échelle par rapport à l'image
// minimap (ex. Cursed Hollow : cores à la même hauteur en jeu mais en diagonale dans l'image → rotation).
// On stocke la position IMAGE (fractions) des 2 cores (bleu, rouge) ; à l'exécution on lit leurs coords
// JEU depuis les structures, et on résout une similitude (rotation+échelle+translation) qui pose l'image
// pour que ses cores tombent EXACTEMENT sous les pastilles-core. Sans ancres → pleine image (fallback).
type Anchor2 = { blue: [number, number]; red: [number, number] };
const MAP_ANCHORS: Record<string, Anchor2> = {
  // [xFraction, yFraction] du centre de chaque core dans /images/minimaps/<slug>.jpg (calibré par carte).
  // Vide pour l'instant → fallback pleine image droite (lisible ; calibration fine = projet dédié).
};

/** Similitude (a,b,c,d,e,f pour ctx.setTransform) qui envoie les 2 points image `s` sur les 2 points
 *  canvas `d`. Rotation + échelle uniforme + translation (méthode nombre complexe vd/vs). */
function solveSimilarity(
  s0: [number, number], s1: [number, number], d0: [number, number], d1: [number, number],
): [number, number, number, number, number, number] {
  const vsx = s1[0] - s0[0], vsy = s1[1] - s0[1];
  const vdx = d1[0] - d0[0], vdy = d1[1] - d0[1];
  const den = vsx * vsx + vsy * vsy || 1;
  const a = (vdx * vsx + vdy * vsy) / den; // scale*cosθ
  const b = (vdy * vsx - vdx * vsy) / den; // scale*sinθ
  const c = -b, d = a;
  const e = d0[0] - (a * s0[0] + c * s0[1]);
  const f = d0[1] - (b * s0[0] + d * s0[1]);
  return [a, b, c, d, e, f];
}

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
  // US-26 : minions/camps sont OFF par défaut — pas de coût de dessin/filtrage tant que l'opérateur
  // ne l'active pas explicitement.
  const [showMinions, setShowMinions] = useState(false);
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const [tick, bumpRedraw] = useState(0); // incrémenté quand un portrait finit de charger → redessine
  const duration = data?.meta.durationSec || 0;

  // US-25 : export d'un sous-intervalle [clipStart, clipEnd] en webm — la sélection est en secondes
  // de replay (mêmes unités que `t`), pas de frame index.
  const [clipStart, setClipStart] = useState<number | null>(null);
  const [clipEnd, setClipEnd] = useState<number | null>(null);
  const [recording, setRecording] = useState(false);
  // rAF de la boucle de lecture pilotée (start→end) dédiée à l'export — DÉLIBÉRÉMENT séparée de
  // usePlayback : celle-ci clampe sur `duration` (fin du replay), pas sur `clipEnd`. Réutiliser
  // pb.toggle/onTick forcerait à faire connaître clipEnd au hook de lecture générale pour rien
  // (un enregistrement est un mode ponctuel, pas un mode de lecture permanent).
  const exportRafRef = useRef<number | null>(null);
  // Handle du recorder actif : à l'unmount pendant un enregistrement, il faut appeler stop() —
  // sinon le MediaRecorder n'est jamais arrêté (pas de timeslice → aucun chunk n'est flush, onstop
  // ne se déclenche pas) et la promesse d'export reste pendante à jamais.
  const recorderRef = useRef<{ stop: () => Promise<Blob> } | null>(null);
  useEffect(() => {
    return () => {
      if (exportRafRef.current != null) cancelAnimationFrame(exportRafRef.current);
      recorderRef.current?.stop().catch(() => {}); // libère le recorder ; Blob ignoré (composant démonté)
    };
  }, []);

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

  // Fond de la visionneuse : minimap in-game (prioritaire) → art peint (fallback) → dégradé (conteneur).
  // Chargée en Image et dessinée DANS le canvas (drawImage), pour partager la transform des pastilles.
  const bgRef = useRef<{ img: HTMLImageElement; slug: string; minimap: boolean } | null>(null);
  useEffect(() => {
    bgRef.current = null;
    const map = data?.meta.mapName;
    if (!map) return;
    const img = new Image();
    let stage: 0 | 1 = 0; // 0 = minimap ; 1 = art peint (fallback)
    const load = () => { img.src = (stage === 0 ? minimapImage(map) : mapImage(map)) ?? ""; };
    img.onload = () => {
      bgRef.current = { img, slug: mapSlug(map), minimap: stage === 0 };
      bumpRedraw((n) => n + 1);
    };
    img.onerror = () => {
      if (stage === 0) { stage = 1; load(); } // minimap absente → tenter l'art peint
      else { bgRef.current = null; bumpRedraw((n) => n + 1); } // les deux absents → dégradé
    };
    load();
    return () => { img.onload = null; img.onerror = null; };
  }, [data?.meta.mapName]);

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

  const canExportClip =
    supportsClipExport() && clipStart != null && clipEnd != null && clipStart < clipEnd && !recording;

  /** US-25 : enregistre le canvas LIVE pendant une lecture pilotée clipStart→clipEnd, puis
   *  télécharge le webm. Boucle rAF locale (indépendante de usePlayback, cf. commentaire plus
   *  haut) : avance `t` par dt réel × vitesse courante jusqu'à atteindre clipEnd. */
  const handleExportClip = async () => {
    if (recording || clipStart == null || clipEnd == null || clipStart >= clipEnd) return;
    if (!supportsClipExport()) return;
    const canvas = canvasRef.current;
    if (!canvas) return;

    pb.pause(); // coupe toute lecture en cours avant de prendre le contrôle de `t`
    setRecording(true);
    try {
      const recorder = recordCanvasStream(canvas, 30);
      recorderRef.current = recorder; // exposé au cleanup d'unmount pour libérer le recorder
      setT(clipStart);
      tRef.current = clipStart;
      await new Promise<void>((resolve) => {
        let last = performance.now();
        const loop = (now: number) => {
          const dt = (now - last) / 1000;
          last = now;
          const next = tRef.current + dt * pb.speed;
          if (next >= clipEnd) {
            setT(clipEnd);
            tRef.current = clipEnd;
            exportRafRef.current = null;
            resolve();
            return;
          }
          setT(next);
          tRef.current = next;
          exportRafRef.current = requestAnimationFrame(loop);
        };
        exportRafRef.current = requestAnimationFrame(loop);
      });
      const blob = await recorder.stop();
      downloadBlob(blob, `replay-${id}-${Math.round(clipStart)}-${Math.round(clipEnd)}.webm`);
    } finally {
      recorderRef.current = null; // recorder déjà arrêté (ou en échec) → plus rien à libérer à l'unmount
      setRecording(false);
    }
  };

  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas || !data) return;
    const ctx = canvas.getContext("2d");
    if (!ctx) return;
    const W = canvas.width, H = canvas.height;
    ctx.clearRect(0, 0, W, H);

    // Fond : minimap in-game (ou art peint en fallback), recadrée sur l'aire jouable pour caler les
    // positions, + léger voile pour que les pastilles ressortent. Sinon le dégradé du conteneur reste.
    const bg = bgRef.current;
    if (bg) {
      const iw = bg.img.naturalWidth, ih = bg.img.naturalHeight;
      const anchors = bg.minimap ? MAP_ANCHORS[bg.slug] : undefined;
      const cores = data.structures.filter((s) => s.kind === "core");
      const blueCore = cores.find((s) => s.team === 0), redCore = cores.find((s) => s.team === 1);
      if (anchors && blueCore && redCore) {
        // Similitude image→canvas calée sur les 2 cores : leurs positions image (ancres) sont envoyées
        // sur les positions canvas des pastilles-core (gx*W, (1-gy)*H). Fixe la rotation/échelle par carte.
        const [a, b, c, d, e, f] = solveSimilarity(
          [anchors.blue[0] * iw, anchors.blue[1] * ih],
          [anchors.red[0] * iw, anchors.red[1] * ih],
          [blueCore.x * W, (1 - blueCore.y) * H],
          [redCore.x * W, (1 - redCore.y) * H],
        );
        ctx.save();
        ctx.setTransform(a, b, c, d, e, f);
        ctx.drawImage(bg.img, 0, 0);
        ctx.restore();
      } else {
        ctx.drawImage(bg.img, 0, 0, W, H); // pas d'ancres → pleine image (fallback non calibré)
      }
      ctx.fillStyle = "rgba(12,14,22,0.30)";
      ctx.fillRect(0, 0, W, H);
    }

    // US-26 : minions/camps TOUT en dessous (avant structures ET héros) — dots discrets, fenêtre
    // ±5s autour de t (nearest-window, pas d'interpolation : le signal est déjà dédupliqué/grossier).
    if (showMinions) {
      const neutralColor = resolveColor("var(--muted-2)");
      ctx.globalAlpha = 0.35;
      for (const ms of minionsNear(data.minions, t)) {
        const cx = ms.x * W, cy = (1 - ms.y) * H;
        ctx.beginPath();
        ctx.arc(cx, cy, 2, 0, Math.PI * 2);
        ctx.fillStyle = ms.team === 1 ? teamColor[1] : ms.team === 0 ? teamColor[0] : neutralColor;
        ctx.fill();
      }
      ctx.globalAlpha = 1;
    }

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
  }, [t, data, playerByPlayerId, teamColor, tick, showMinions]);

  if (isLoading) return <div className="empty">loading…</div>;
  if (!data) return <div className="empty">replay unavailable</div>;

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
          disabled={recording}
          style={{ minWidth: 28, textAlign: "center", fontFamily: "inherit", lineHeight: "inherit" }}
        >
          {pb.playing ? "⏸" : "▶"}
        </button>
        <select
          aria-label="playback speed"
          value={pb.speed}
          onChange={(e) => pb.setSpeed(Number(e.target.value))}
          disabled={recording}
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
          disabled={recording}
          style={{ flex: 1 }}
        />
        <span className="mono muted" style={{ fontSize: 11, minWidth: 90, textAlign: "right" }}>
          {fmtClock(t)} / {fmtClock(duration)}
        </span>
        <label style={{ display: "flex", alignItems: "center", gap: 5, fontSize: 11, color: "var(--muted-2)", cursor: "pointer" }}>
          <input
            type="checkbox"
            aria-label="Minions / camps"
            checked={showMinions}
            onChange={(e) => setShowMinions(e.target.checked)}
            disabled={recording}
          />
          Minions / camps
        </label>
      </div>
      <div style={{ display: "flex", alignItems: "center", gap: 10, marginTop: 8, flexWrap: "wrap" }}>
        <span className="cap" style={{ fontSize: 10 }}>Clip export</span>
        <button
          type="button"
          className="pill"
          aria-label="Set clip in point to current time"
          onClick={() => setClipStart(t)}
          disabled={recording}
        >
          Set in
        </button>
        <button
          type="button"
          className="pill"
          aria-label="Set clip out point to current time"
          onClick={() => setClipEnd(t)}
          disabled={recording}
        >
          Set out
        </button>
        <span className="mono muted" style={{ fontSize: 11 }}>
          {clipStart != null && clipEnd != null
            ? `${fmtClock(clipStart)}–${fmtClock(clipEnd)}`
            : "no range selected"}
        </span>
        {clipStart != null && clipEnd != null && clipStart < clipEnd && (
          <span className="muted" style={{ fontSize: 10 }}>
            {clipFrames(clipStart, clipEnd, 30)} frames @30fps
          </span>
        )}
        <button
          type="button"
          className="pill on"
          aria-label="Export clip to webm"
          onClick={handleExportClip}
          disabled={!canExportClip}
          title={
            !supportsClipExport()
              ? "Clip export isn't supported in this browser (missing MediaRecorder / captureStream)"
              : undefined
          }
        >
          Export clip
        </button>
        {recording && (
          <span
            role="status"
            aria-label="Recording clip"
            style={{ display: "flex", alignItems: "center", gap: 4, fontSize: 11, color: "#e5484d" }}
          >
            <span aria-hidden="true">●</span> Recording…
          </span>
        )}
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
