import { useQuery } from "@tanstack/react-query";
import { fetchMatches, modeBadge, fmtDur, mapImage, useLiveUpdates, useDimHeroes, useSettings, pickOperator, jarvisPhrase, awardLabel } from "../api";
import { Avatar } from "../components/Avatar";
import { OverlayFrame } from "../components/OverlayFrame";

/**
 * Widget OBS (browser source) : résumé de la DERNIÈRE partie du point de vue de l'opérateur,
 * fond transparent, mise à jour live via WS. L'opérateur est désigné par `?me=<nom en jeu>`
 * dans l'URL de la browser source (ex. /widget?me=matella) ; à défaut, le premier joueur.
 */
export function Widget() {
  useDimHeroes(); // référentiel portraits + anneaux (widget hors Layout)
  useSettings(); // operator_names (perspective par défaut sans ?me=)
  const { data, refetch } = useQuery({ queryKey: ["widget-last"], queryFn: () => fetchMatches({ limit: 1 }) });
  useLiveUpdates(() => refetch());

  const m = data?.[0];
  if (!m) return <OverlayFrame anchor="top-right" top={250}><div /></OverlayFrame>;

  const players = m.players ?? [];
  const me = pickOperator(players, new URLSearchParams(location.search).get("me"));
  const won = me?.team != null && m.winner === me.team;
  const mb = modeBadge(m.mode);

  // K/A/D du point de vue opérateur ; KP = takedowns / takedowns de l'équipe.
  const k = me?.kills ?? 0;
  const td = me?.takedowns ?? 0;
  const d = me?.deaths ?? 0;
  const a = Math.max(0, td - k);
  // KP = takedowns / kills de l'équipe (= morts de l'équipe adverse). Sommer les takedowns
  // surcompte (chaque participant en gagne un), d'où un KP faussement bas.
  const teamKills = players
    .filter((p) => me?.team != null && p.team != null && p.team !== me.team)
    .reduce((s, p) => s + (p.deaths ?? 0), 0);
  const kp = teamKills > 0 ? Math.min(100, Math.round((td / teamKills) * 100)) : null;
  const mapBg = mapImage(m.map);

  return (
    <OverlayFrame anchor="top-right" top={250}>
      <div
        style={{
          maxWidth: 360,
          // image de carte en fond, fortement voilée (texte lisible) ; fallback couleur si absente
          backgroundColor: "rgba(14,16,22,.92)",
          backgroundImage: mapBg
            ? `linear-gradient(90deg, rgba(14,16,22,.96) 0%, rgba(14,16,22,.86) 55%, rgba(14,16,22,.68) 100%), url(${mapBg})`
            : undefined,
          backgroundSize: "cover",
          backgroundPosition: "center 30%",
          border: `1px solid ${won ? "var(--win)" : "var(--loss)"}`,
          borderRadius: 12,
          padding: "12px 14px",
          display: "flex",
          alignItems: "center",
          gap: 12,
          boxShadow: "0 8px 30px rgba(0,0,0,.5)",
        }}
      >
        <Avatar hero={me?.hero ?? null} size={44} />
        <div style={{ flex: 1 }}>
          <div style={{ fontSize: 13, fontWeight: 600, display: "flex", alignItems: "center", gap: 7 }}>
            <span style={{ color: won ? "var(--win)" : "var(--loss)" }}>{won ? "VICTORY" : "DEFEAT"}</span>
            <span style={{ color: "var(--text-2)", fontWeight: 400 }}>· {me?.hero ?? "?"} · {m.map}</span>
            {(() => {
              const aw = awardLabel(me?.award);
              if (!aw) return null;
              return (
                <span
                  style={{
                    marginLeft: "auto",
                    fontSize: 10.5,
                    fontWeight: 700,
                    padding: "2px 7px",
                    borderRadius: 999,
                    color: aw.mvp ? "#1a1500" : "var(--text)",
                    background: aw.mvp ? "linear-gradient(90deg,#f5c542,#e0a818)" : "rgba(255,255,255,.1)",
                    boxShadow: aw.mvp ? "0 0 10px rgba(245,197,66,.5)" : "none",
                    whiteSpace: "nowrap",
                  }}
                >
                  {aw.mvp ? "👑 MVP" : `🏅 ${aw.label}`}
                </span>
              );
            })()}
          </div>
          <div className="mono" style={{ fontSize: 12, color: "#cfd3e0", marginTop: 3 }}>
            {k}/{a}/{d}
            {kp != null && <span style={{ color: "var(--text-2)" }}> · KP {kp}%</span>}
            <span style={{ color: "var(--text-2)" }}> · </span>
            <span className={`bdg ${mb.cls}`}>{mb.short}</span>
            <span style={{ color: "var(--text-2)" }}> · {fmtDur(m.length)}</span>
          </div>
          <div style={{ fontSize: 11, color: "var(--u-nexus)", fontStyle: "italic", marginTop: 5 }}>
            « {jarvisPhrase({ won, hero: me?.hero ?? null, deaths: d, takedowns: td })} » — Jarvis
          </div>
        </div>
      </div>
    </OverlayFrame>
  );
}
