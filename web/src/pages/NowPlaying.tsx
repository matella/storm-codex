import { useQuery } from "@tanstack/react-query";
import { useEffect, useState } from "react";
import { parseTrack } from "../api";
import { OverlayFrame } from "../components/OverlayFrame";

/**
 * Widget musique OBS (browser source) — lecture Spotify LIVE via le proxy storm-codex
 * (/api/now-playing → Orpheus /api/playback/now). Carte étoffée : pochette, titre/artiste/album,
 * barre de progression fluide (avance localement entre deux refetch) et égaliseur animé quand ça
 * joue. Fond transparent, ancré en bas-gauche. « Music — off » si Orpheus non authentifié.
 */
const fmt = (ms: number) => {
  const s = Math.max(0, Math.floor(ms / 1000));
  return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, "0")}`;
};

export function NowPlaying() {
  const { data } = useQuery({
    queryKey: ["now-playing"],
    queryFn: () => fetch("/api/now-playing").then((r) => r.json()),
    refetchInterval: 5000,
  });
  const t = parseTrack(data);

  // progression fluide : on cale sur la valeur serveur à chaque refetch, puis on avance localement.
  const [prog, setProg] = useState(0);
  useEffect(() => { setProg(t.progressMs ?? 0); }, [t.progressMs, t.title]);
  useEffect(() => {
    if (!t.playing || !t.durationMs) return;
    const id = setInterval(() => setProg((p) => Math.min(p + 500, t.durationMs!)), 500);
    return () => clearInterval(id);
  }, [t.playing, t.durationMs]);

  const pct = t.durationMs ? Math.min(100, (prog / t.durationMs) * 100) : 0;

  return (
    <OverlayFrame anchor="bottom-left">
      <style>{`@keyframes eq{0%,100%{height:3px}50%{height:13px}}`}</style>
      <div
        style={{
          width: 400,
          background: "linear-gradient(135deg, rgba(18,20,28,.96), rgba(12,13,18,.96))",
          border: "1px solid var(--hairline-strong)",
          borderRadius: 16,
          padding: 16,
          display: "flex",
          gap: 16,
          alignItems: "center",
          boxShadow: "0 14px 40px rgba(0,0,0,.55)",
        }}
      >
        {/* pochette */}
        {t.playing && t.art ? (
          <img src={t.art} alt="" style={{ width: 80, height: 80, borderRadius: 12, objectFit: "cover", flexShrink: 0, boxShadow: "0 4px 14px rgba(0,0,0,.5)" }} />
        ) : (
          <div style={{ width: 80, height: 80, borderRadius: 12, flexShrink: 0, display: "flex", alignItems: "center", justifyContent: "center", background: "var(--surface-2)", fontSize: 30, color: "var(--text-2)" }}>♫</div>
        )}

        <div style={{ flex: 1, minWidth: 0 }}>
          {/* entête : NOW PLAYING + égaliseur */}
          <div style={{ display: "flex", alignItems: "center", gap: 8, marginBottom: 6 }}>
            <span className="kick" style={{ fontSize: 10, color: "var(--accent)" }}>
              {t.playing ? "NOW PLAYING" : data?.authenticated ? "PAUSED" : "MUSIC"}
            </span>
            {t.playing && (
              <span style={{ display: "inline-flex", alignItems: "flex-end", gap: 2, height: 13 }}>
                {[0, 1, 2, 3].map((i) => (
                  <span key={i} style={{ width: 3, background: "var(--accent)", borderRadius: 1, animation: `eq .9s ease-in-out ${i * 0.18}s infinite` }} />
                ))}
              </span>
            )}
            <span className="kick" style={{ fontSize: 9, marginLeft: "auto" }}>ORPHEUS</span>
          </div>

          {t.playing || t.title ? (
            <>
              <div style={{ fontSize: 17, fontWeight: 700, whiteSpace: "nowrap", overflow: "hidden", textOverflow: "ellipsis", lineHeight: 1.25 }}>{t.title}</div>
              {t.artist && (
                <div style={{ fontSize: 13.5, color: "var(--text-2)", whiteSpace: "nowrap", overflow: "hidden", textOverflow: "ellipsis", marginTop: 1 }}>{t.artist}</div>
              )}
              {t.album && (
                <div style={{ fontSize: 11, color: "var(--text-3, #7c8194)", whiteSpace: "nowrap", overflow: "hidden", textOverflow: "ellipsis", marginTop: 1 }}>{t.album}</div>
              )}

              {/* barre de progression */}
              {t.durationMs ? (
                <div style={{ marginTop: 10 }}>
                  <div style={{ height: 4, borderRadius: 2, background: "rgba(255,255,255,.12)", overflow: "hidden" }}>
                    <div style={{ width: `${pct}%`, height: "100%", background: "var(--accent)", borderRadius: 2, transition: "width .5s linear" }} />
                  </div>
                  <div className="mono" style={{ display: "flex", justifyContent: "space-between", fontSize: 10, color: "var(--text-2)", marginTop: 4 }}>
                    <span>{fmt(prog)}</span>
                    <span>{fmt(t.durationMs)}</span>
                  </div>
                </div>
              ) : null}
            </>
          ) : (
            <div style={{ fontSize: 14, color: "var(--text-2)" }}>
              {data?.authenticated ? "Nothing playing" : "Music — off"}
            </div>
          )}
        </div>
      </div>
    </OverlayFrame>
  );
}
