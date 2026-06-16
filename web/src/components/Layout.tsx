import { NavLink, Outlet } from "react-router-dom";
import { useState } from "react";
import { useLiveUpdates, useDimHeroes, useSettings } from "../api";

const TABS: [string, string][] = [
  ["/", "Session"],
  ["/matches", "Matches"],
  ["/heroes", "Heroes"],
  ["/maps", "Maps"],
  ["/synergies", "Synergies"],
  ["/patches", "Patch Notes"],
  ["/trends", "Trends"],
  ["/leagues", "Leagues"],
  ["/admin", "Admin"],
];

export function Layout() {
  const [live, setLive] = useState(false);
  const [flash, setFlash] = useState<string | null>(null);
  useDimHeroes(); // peuple le référentiel héros (anneaux d'univers)
  useSettings(); // peuple operator_names (perspective opérateur partout)
  useLiveUpdates((ev) => {
    setLive(true);
    setFlash(`● new replay — ${ev.map ?? "match"} added`);
    setTimeout(() => setFlash(null), 6000);
  });
  return (
    <>
      <div className="topbar">
        <span className="brand">STORM CODEX</span>
        <nav className="nav">
          {TABS.map(([to, label]) => (
            <NavLink key={to} to={to} end={to === "/"} className={({ isActive }) => (isActive ? "on" : "")}>
              {label}
            </NavLink>
          ))}
        </nav>
        <span className={live ? "live" : "live off"}>
          ● {live ? "online" : "offline"}
        </span>
      </div>
      <div className="shell">
        {flash && <div className="toast mono">{flash}</div>}
        <Outlet />
      </div>
    </>
  );
}
