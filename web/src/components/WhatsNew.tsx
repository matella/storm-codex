import { useState } from "react";
import { APP_VERSION, CHANGELOG } from "../whatsnew";

const KEY = "whatsnew_seen";

/** Modale « What's new » : à l'ouverture de l'app, si la version courante > dernière vue, montre les
 *  nouveautés depuis. Dismiss = marqué vu. Re-ouvrable via le bouton « ? » du topbar (prop `force`). */
export function WhatsNew({ force, onClose }: { force?: boolean; onClose?: () => void }) {
  const seen = localStorage.getItem(KEY) ?? "";
  // auto-affichage seulement pour un utilisateur existant (onboarding fait) → pas de double modale au 1er run
  const [open, setOpen] = useState(force || (!!localStorage.getItem("onboarding_done") && seen < APP_VERSION));
  if (!open && !force) return null;
  const entries = force ? CHANGELOG : CHANGELOG.filter((c) => c.version > seen);
  if (!entries.length) return null;
  const close = () => { localStorage.setItem(KEY, APP_VERSION); setOpen(false); onClose?.(); };
  return (
    <div onClick={close} style={{ position: "fixed", inset: 0, background: "rgba(0,0,0,.55)", display: "flex", alignItems: "center", justifyContent: "center", zIndex: 1000 }}>
      <div onClick={(e) => e.stopPropagation()} style={{ background: "var(--surface)", border: "1px solid var(--hairline-strong)", borderRadius: 14, padding: "20px 24px", maxWidth: 520, maxHeight: "80vh", overflowY: "auto", boxShadow: "0 20px 60px rgba(0,0,0,.6)" }}>
        <div style={{ display: "flex", alignItems: "baseline", gap: 10, marginBottom: 12 }}>
          <h2 style={{ margin: 0 }}>What's new</h2>
          <span className="mono muted" style={{ fontSize: 11 }}>{APP_VERSION}</span>
          <span className="pill" style={{ marginLeft: "auto" }} onClick={close}>got it</span>
        </div>
        {entries.map((c) => (
          <div key={c.version} style={{ marginBottom: 14 }}>
            <div className="kick" style={{ fontSize: 12, marginBottom: 6 }}>{c.title}</div>
            <ul style={{ margin: 0, paddingLeft: 18, lineHeight: 1.6, fontSize: 13.5 }}>
              {c.items.map((it, i) => <li key={i}>{it}</li>)}
            </ul>
          </div>
        ))}
      </div>
    </div>
  );
}
