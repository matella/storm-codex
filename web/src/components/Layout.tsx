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
  ["/scouting", "Scouting"],
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
    <div className="ds-app">
      <header className="ds-top">
        <NavLink to="/" className="ds-brand">STORM <b>CODEX</b></NavLink>
        <nav className="ds-topnav">
          {TABS.map(([to, label]) => (
            <NavLink key={to} to={to} end={to === "/"} className={({ isActive }) => (isActive ? "on" : "")}>
              {label}
              {to === "/patches" && newPatch && <i className="dot" title="new patch" />}
            </NavLink>
          ))}
        </nav>
        <span className="ds-help" title="Help / what's new" onClick={() => setHelp("tour")}>?</span>
        <span className={live ? "ds-live" : "ds-live off"}><i />{live ? "online" : "offline"}</span>
      </header>
      <main className="ds-shell">
        {flash && <div className="ds-flash">{flash}</div>}
        <Outlet />
      </main>
      <Onboarding force={help === "tour"} onClose={() => setHelp(null)} />
      <WhatsNew force={help === "whatsnew"} onClose={() => setHelp(null)} />
    </div>
  );
}
