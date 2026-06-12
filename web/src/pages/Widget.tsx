import { useQuery } from "@tanstack/react-query";
import { useEffect } from "react";
import { fetchMatches, modeBadge, fmtDur, useLiveUpdates } from "../api";
import { Avatar } from "../components/Avatar";

/** Widget OBS (browser source) : résumé de la dernière partie, fond transparent, live via WS. */
export function Widget() {
  const { data, refetch } = useQuery({ queryKey: ["widget-last"], queryFn: () => fetchMatches({ limit: 1 }) });
  useLiveUpdates(() => refetch());
  useEffect(() => {
    document.body.style.background = "transparent";
    return () => { document.body.style.background = ""; };
  }, []);

  const m = data?.[0];
  if (!m) return <div style={{ padding: 16 }} />;
  const p = m.players?.[0];
  const won = m.winner === p?.team;
  const mb = modeBadge(m.mode);
  return (
    <div style={{ padding: 14, maxWidth: 360 }}>
      <div
        style={{
          background: "rgba(14,16,22,.92)",
          border: `1px solid ${won ? "var(--win)" : "var(--loss)"}`,
          borderRadius: 12,
          padding: "12px 14px",
          display: "flex",
          alignItems: "center",
          gap: 12,
          boxShadow: "0 8px 30px rgba(0,0,0,.5)",
        }}
      >
        <Avatar hero={p?.hero ?? null} size={44} />
        <div style={{ flex: 1 }}>
          <div style={{ fontSize: 13, fontWeight: 600, display: "flex", alignItems: "center", gap: 7 }}>
            {p?.hero ?? "?"} · {m.map}
            <span className={`bdg ${won ? "b-win" : "b-loss"}`}>{won ? "VICTOIRE" : "DÉFAITE"}</span>
          </div>
          <div className="mono" style={{ fontSize: 11, color: "var(--text-2)", marginTop: 3 }}>
            <span className={`bdg ${mb.cls}`}>{mb.short}</span> · {fmtDur(m.length)} · équipe{" "}
            {m.winner === 0 ? "bleue" : "rouge"} gagne
          </div>
        </div>
      </div>
    </div>
  );
}
