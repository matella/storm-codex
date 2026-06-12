import { useEffect, useRef } from "react";
import uPlot from "uplot";
import "uplot/dist/uPlot.min.css";

interface Seg { start: number; end: number; levelDiff: number }

/** Timeline d'avantage de niveau (match.levelAdvTimeline) — courbe en escalier (uPlot). */
export function LevelChart({ timeline }: { timeline: Seg[] }) {
  const ref = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (!ref.current || !timeline?.length) return;
    // points en escalier : (start, diff) puis (end, diff)
    const xs: number[] = [];
    const ys: number[] = [];
    for (const s of timeline) {
      xs.push(s.start / 60, s.end / 60);
      ys.push(s.levelDiff, s.levelDiff);
    }
    const accent = getComputedStyle(document.body).getPropertyValue("--accent").trim() || "#7f77dd";
    const opts: uPlot.Options = {
      width: ref.current.clientWidth || 760,
      height: 150,
      cursor: { show: true },
      legend: { show: false },
      scales: { x: { time: false } },
      axes: [
        { stroke: "#5d6275", grid: { stroke: "#1a1d2a" }, ticks: { stroke: "#1a1d2a" },
          values: (_u, vals) => vals.map((v) => `${v|0}m`) },
        { stroke: "#5d6275", grid: { stroke: "#1a1d2a" }, ticks: { stroke: "#1a1d2a" } },
      ],
      series: [
        {},
        { stroke: accent, width: 2, points: { show: false }, paths: uPlot.paths.stepped!({ align: 1 }) },
      ],
    };
    const u = new uPlot(opts, [xs, ys], ref.current);
    const onResize = () => u.setSize({ width: ref.current!.clientWidth, height: 150 });
    window.addEventListener("resize", onResize);
    return () => { window.removeEventListener("resize", onResize); u.destroy(); };
  }, [timeline]);
  if (!timeline?.length) return null;
  return <div ref={ref} style={{ padding: "8px 12px" }} />;
}
