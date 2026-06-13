import { useQuery } from "@tanstack/react-query";
import { useEffect } from "react";

/**
 * Persistent "now playing" music widget (OBS browser source) — reads Orpheus via the storm-codex
 * proxy (/api/now-playing). Transparent background; meant to stay on every scene. Shows the track
 * when Orpheus is authenticated/playing, otherwise a compact idle pill. English UI.
 */
export function NowPlaying() {
  const { data } = useQuery({
    queryKey: ["now-playing"],
    queryFn: () => fetch("/api/now-playing").then((r) => r.json()),
    refetchInterval: 5000, // léger polling (la musique change lentement)
  });
  useEffect(() => {
    document.body.style.background = "transparent";
    return () => { document.body.style.background = ""; };
  }, []);

  // tolérant sur la forme renvoyée par Orpheus (title/artist/name/track…)
  const cur = data?.current ?? {};
  const title: string | undefined = cur.title ?? cur.name ?? cur.track ?? cur.song;
  const artist: string | undefined = cur.artist ?? cur.artists ?? cur.author;
  const playing = data?.authenticated && !!title;

  return (
    <div style={{ padding: 12, maxWidth: 340 }}>
      <div
        style={{
          background: "rgba(14,16,22,.92)",
          border: "1px solid var(--hairline-strong)",
          borderRadius: 12,
          padding: "9px 12px",
          display: "flex",
          alignItems: "center",
          gap: 10,
          boxShadow: "0 8px 30px rgba(0,0,0,.5)",
        }}
      >
        <span style={{ fontSize: 16 }}>{playing ? "♪" : "♫"}</span>
        <div style={{ flex: 1, minWidth: 0 }}>
          {playing ? (
            <>
              <div style={{ fontSize: 12.5, fontWeight: 600, whiteSpace: "nowrap", overflow: "hidden", textOverflow: "ellipsis" }}>
                {title}
              </div>
              {artist && (
                <div style={{ fontSize: 11, color: "var(--text-2)", whiteSpace: "nowrap", overflow: "hidden", textOverflow: "ellipsis" }}>
                  {artist}
                </div>
              )}
            </>
          ) : (
            <div style={{ fontSize: 11.5, color: "var(--text-2)" }}>
              {data?.authenticated ? "Nothing playing" : "Music — off"}
            </div>
          )}
        </div>
        <span className="kick" style={{ fontSize: 9 }}>ORPHEUS</span>
      </div>
    </div>
  );
}
