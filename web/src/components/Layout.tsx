import { NavLink, Outlet, useLocation } from "react-router-dom";
import { useEffect, useState } from "react";
import { useLiveUpdates, useDimHeroes, useSettings } from "../api";
import { WhatsNew } from "./WhatsNew";
import { Onboarding } from "./Onboarding";

const TABS: [string, string][] = [
  ["/", "Session"],
  ["/matches", "Matches"],
  ["/heroes", "Heroes"],
  ["/maps", "Maps"],
  ["/synergies", "Synergies"],
  ["/patches", "Patch Notes"],
  ["/trends", "Trends"],
  ["/leagues", "Leagues"],
  ["/draft", "Draft"],
  ["/admin", "Admin"],
];

export function Layout() {
  const [live, setLive] = useState(false);
  const [flash, setFlash] = useState<string | null>(null);
  const [newPatch, setNewPatch] = useState(false); // pastille sur l'onglet Patch Notes
  const [help, setHelp] = useState<null | "tour" | "whatsnew">(null);
  const loc = useLocation();
  useDimHeroes(); // peuple le référentiel héros (anneaux d'univers)
  useSettings(); // peuple operator_names (perspective opérateur partout)
  useLiveUpdates((ev) => {
    setLive(true);
    if (ev.type === "patch.new") {
      setNewPatch(true);
      setFlash(`🆕 new patch — ${ev.name ?? "patch notes"}`);
    } else {
      setFlash(`● new replay — ${ev.map ?? "match"} added`);
    }
    setTimeout(() => setFlash(null), 6000);
  });
  // la pastille disparaît quand on visite la page Patch Notes
  useEffect(() => { if (loc.pathname.startsWith("/patches")) setNewPatch(false); }, [loc.pathname]);
  return (
    <>
      <div className="topbar">
        <span className="brand">STORM CODEX</span>
        <nav className="nav">
          {TABS.map(([to, label]) => (
            <NavLink key={to} to={to} end={to === "/"} className={({ isActive }) => (isActive ? "on" : "")}>
              {label}
              {to === "/patches" && newPatch && <span style={{ marginLeft: 5, color: "var(--accent)" }}>●</span>}
            </NavLink>
          ))}
        </nav>
        <span className="pill" title="Help / what's new" style={{ marginLeft: "auto", cursor: "pointer" }} onClick={() => setHelp("tour")}>?</span>
        <span className={live ? "live" : "live off"} style={{ marginLeft: 10 }}>
          ● {live ? "online" : "offline"}
        </span>
      </div>
      <div className="shell">
        {flash && <div className="toast mono">{flash}</div>}
        <Outlet />
      </div>
      <Onboarding force={help === "tour"} onClose={() => setHelp(null)} />
      <WhatsNew force={help === "whatsnew"} onClose={() => setHelp(null)} />
    </>
  );
}
