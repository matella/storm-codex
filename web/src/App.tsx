import { Routes, Route } from "react-router-dom";
import { Layout } from "./components/Layout";
import { Dashboard } from "./pages/Dashboard";
import { Matches } from "./pages/Matches";
import { MatchDetail } from "./pages/MatchDetail";
import { Heroes } from "./pages/Heroes";
import { Maps } from "./pages/Maps";
import { Player } from "./pages/Player";
import { Widget } from "./pages/Widget";

export default function App() {
  return (
    <Routes>
      {/* widget OBS : standalone (fond transparent, sans topbar) */}
      <Route path="widget" element={<Widget />} />
      <Route element={<Layout />}>
        <Route index element={<Dashboard />} />
        <Route path="matches" element={<Matches />} />
        <Route path="match/:id" element={<MatchDetail />} />
        <Route path="player/:toon" element={<Player />} />
        <Route path="heroes" element={<Heroes />} />
        <Route path="maps" element={<Maps />} />
      </Route>
    </Routes>
  );
}
