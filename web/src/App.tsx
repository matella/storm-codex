import { lazy, Suspense } from "react";
import type React from "react";
import { Routes, Route, Navigate } from "react-router-dom";
import { Layout } from "./components/Layout";
import { LEGACY_REDIRECTS } from "./legacyRoutes";

// Pages chargées à la demande : chaque page a son propre chunk. Les sources OBS (overlays, widget,
// ticker) ne téléchargent plus toute l'appli (uPlot, Replay2D, scouting, DOMPurify…).
const page = <K extends string>(load: () => Promise<Record<K, React.ComponentType>>, name: K) =>
  lazy(() => load().then((m) => ({ default: m[name] })));
const Dashboard = page(() => import("./pages/Dashboard"), "Dashboard");
const Patch = page(() => import("./pages/Patch"), "Patch");
const Matches = page(() => import("./pages/Matches"), "Matches");
const MatchDetail = page(() => import("./pages/MatchDetail"), "MatchDetail");
const Heroes = page(() => import("./pages/Heroes"), "Heroes");
const Hero = page(() => import("./pages/Hero"), "Hero");
const Synergies = page(() => import("./pages/Synergies"), "Synergies");
const Patches = page(() => import("./pages/Patches"), "Patches");
const HeroChanges = page(() => import("./pages/HeroChanges"), "HeroChanges");
const Maps = page(() => import("./pages/Maps"), "Maps");
const Player = page(() => import("./pages/Player"), "Player");
const Players = page(() => import("./pages/Players"), "Players");
const Widget = page(() => import("./pages/Widget"), "Widget");
const Queue = page(() => import("./pages/Queue"), "Queue");
const Ticker = page(() => import("./pages/Ticker"), "Ticker");
const NowPlaying = page(() => import("./pages/NowPlaying"), "NowPlaying");
const Trends = page(() => import("./pages/Trends"), "Trends");
const Leagues = page(() => import("./pages/Leagues"), "Leagues");
const Admin = page(() => import("./pages/Admin"), "Admin");
const DraftOverlay = page(() => import("./pages/DraftOverlay"), "DraftOverlay");
const CastOverlay = page(() => import("./pages/CastOverlay"), "CastOverlay");
const Cast = page(() => import("./pages/Cast"), "Cast");
const Scouting = page(() => import("./pages/Scouting"), "Scouting");
const ScoutingReport = page(() => import("./pages/ScoutingReport"), "ScoutingReport");
const Live = page(() => import("./pages/Live"), "Live");

/** Overlays OBS : rien pendant le chargement (fond transparent). */
const bare = (el: React.ReactNode) => <Suspense fallback={null}>{el}</Suspense>;
/** Pages de l'appli : indicateur dans le cadre du Layout. */
const framed = (el: React.ReactNode) => <Suspense fallback={<div className="empty">loading…</div>}>{el}</Suspense>;

export default function App() {
  return (
    <Routes>
      {/* sources OBS standalone (fond transparent, sans topbar) */}
      <Route path="widget" element={bare(<Widget />)} />
      <Route path="queue" element={bare(<Queue />)} />
      <Route path="ticker" element={bare(<Ticker />)} />
      <Route path="now-playing" element={bare(<NowPlaying />)} />
      <Route path="draft/overlay" element={bare(<DraftOverlay />)} />
      <Route path="cast/overlay" element={bare(<CastOverlay />)} />
      <Route element={<Layout />}>
        <Route index element={framed(<Dashboard />)} />
        <Route path="live" element={framed(<Live />)} />
        <Route path="matches" element={framed(<Matches />)} />
        <Route path="match/:id" element={framed(<MatchDetail />)} />
        <Route path="players" element={framed(<Players />)} />
        <Route path="player/:toon" element={framed(<Player />)} />
        <Route path="heroes" element={framed(<Heroes />)} />
        <Route path="hero/:name" element={framed(<Hero />)} />
        <Route path="synergies" element={framed(<Synergies />)} />
        <Route path="patches" element={framed(<Patches />)} />
        <Route path="hero-changes" element={framed(<HeroChanges />)} />
        <Route path="patch/:id" element={framed(<Patch />)} />
        <Route path="maps" element={framed(<Maps />)} />
        <Route path="trends" element={framed(<Trends />)} />
        <Route path="leagues" element={framed(<Leagues />)} />
        {LEGACY_REDIRECTS.map((r) => <Route key={r.path} path={r.path} element={<Navigate to={r.to} replace />} />)}
        <Route path="cast" element={framed(<Cast />)} />
        <Route path="scouting" element={framed(<Scouting />)} />
        <Route path="scouting/:id" element={framed(<ScoutingReport />)} />
        <Route path="admin" element={framed(<Admin />)} />
      </Route>
    </Routes>
  );
}
