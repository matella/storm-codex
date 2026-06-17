import { lazy, Suspense } from "react";
import { Routes, Route } from "react-router-dom";
import { Layout } from "./components/Layout";
import { Dashboard } from "./pages/Dashboard";
import { Matches } from "./pages/Matches";
import { MatchDetail } from "./pages/MatchDetail";
import { Heroes } from "./pages/Heroes";
import { Hero } from "./pages/Hero";
import { Synergies } from "./pages/Synergies";
import { Patches } from "./pages/Patches";
import { HeroChanges } from "./pages/HeroChanges";
// détail patch lazy : isole DOMPurify hors du bundle principal (overlays légers)
const Patch = lazy(() => import("./pages/Patch").then((m) => ({ default: m.Patch })));
import { Maps } from "./pages/Maps";
import { Player } from "./pages/Player";
import { Widget } from "./pages/Widget";
import { Queue } from "./pages/Queue";
import { Ticker } from "./pages/Ticker";
import { NowPlaying } from "./pages/NowPlaying";
import { Trends } from "./pages/Trends";
import { Leagues } from "./pages/Leagues";
import { Admin } from "./pages/Admin";

export default function App() {
  return (
    <Routes>
      {/* sources OBS standalone (fond transparent, sans topbar) */}
      <Route path="widget" element={<Widget />} />
      <Route path="queue" element={<Queue />} />
      <Route path="ticker" element={<Ticker />} />
      <Route path="now-playing" element={<NowPlaying />} />
      <Route element={<Layout />}>
        <Route index element={<Dashboard />} />
        <Route path="matches" element={<Matches />} />
        <Route path="match/:id" element={<MatchDetail />} />
        <Route path="player/:toon" element={<Player />} />
        <Route path="heroes" element={<Heroes />} />
        <Route path="hero/:name" element={<Hero />} />
        <Route path="synergies" element={<Synergies />} />
        <Route path="patches" element={<Patches />} />
        <Route path="hero-changes" element={<HeroChanges />} />
        <Route path="patch/:id" element={<Suspense fallback={<div className="empty">loading…</div>}><Patch /></Suspense>} />
        <Route path="maps" element={<Maps />} />
        <Route path="trends" element={<Trends />} />
        <Route path="leagues" element={<Leagues />} />
        <Route path="admin" element={<Admin />} />
      </Route>
    </Routes>
  );
}
