import { useState } from "react";
import { Link } from "react-router-dom";

const KEY = "onboarding_done";

/** Assistant de 1er lancement : étapes de setup (identité, uploader, overlays OBS). Auto-affiché si
 *  jamais terminé ; re-ouvrable via le bouton « ? » (prop `force`). */
export function Onboarding({ force, onClose }: { force?: boolean; onClose?: () => void }) {
  const [open, setOpen] = useState(force || !localStorage.getItem(KEY));
  if (!open && !force) return null;
  const close = () => { localStorage.setItem(KEY, "1"); setOpen(false); onClose?.(); };
  const origin = location.origin;
  const Step = ({ n, title, children }: { n: number; title: string; children: React.ReactNode }) => (
    <div style={{ display: "flex", gap: 12, marginBottom: 14 }}>
      <span className="mono" style={{ flexShrink: 0, width: 24, height: 24, borderRadius: 999, background: "var(--accent)", color: "#0b0d12", display: "flex", alignItems: "center", justifyContent: "center", fontWeight: 700, fontSize: 12 }}>{n}</span>
      <div><div style={{ fontWeight: 600, marginBottom: 2 }}>{title}</div><div style={{ fontSize: 13, color: "var(--text-2)", lineHeight: 1.5 }}>{children}</div></div>
    </div>
  );
  return (
    <div onClick={close} style={{ position: "fixed", inset: 0, background: "rgba(0,0,0,.55)", display: "flex", alignItems: "center", justifyContent: "center", zIndex: 1000 }}>
      <div onClick={(e) => e.stopPropagation()} style={{ background: "var(--surface)", border: "1px solid var(--hairline-strong)", borderRadius: 14, padding: "22px 26px", maxWidth: 560, maxHeight: "82vh", overflowY: "auto", boxShadow: "0 20px 60px rgba(0,0,0,.6)" }}>
        <div style={{ display: "flex", alignItems: "baseline", gap: 10, marginBottom: 16 }}>
          <h2 style={{ margin: 0 }}>Welcome to Storm Codex</h2>
          <span className="pill" style={{ marginLeft: "auto" }} onClick={close}>skip</span>
        </div>
        <Step n={1} title="Set your in-game name(s)">
          Open <Link to="/admin" onClick={close} style={{ color: "var(--accent)" }}>Admin → My identity</Link> and enter your HotS name(s),
          comma-separated. This drives all your stats, the session panel and overlays.
        </Step>
        <Step n={2} title="Connect the uploader (your gaming PC)">
          In <Link to="/admin" onClick={close} style={{ color: "var(--accent)" }}>Admin → Upload tokens</Link>, create a token, then run the
          uploader on your PC pointing here (server URL + token + your HotS replays folder). It backfills
          and watches new games automatically.
        </Step>
        <Step n={3} title="Add the OBS overlays (optional)">
          Add these as Browser Sources (1920×1080, transparent):
          <div className="mono" style={{ fontSize: 11, marginTop: 6, lineHeight: 1.8 }}>
            {origin}/queue · {origin}/ticker · {origin}/widget?me=&lt;name&gt; · {origin}/now-playing
          </div>
        </Step>
        <div style={{ display: "flex", justifyContent: "flex-end", marginTop: 8 }}>
          <Link to="/admin" onClick={close} className="pill on">Get started ›</Link>
        </div>
      </div>
    </div>
  );
}
