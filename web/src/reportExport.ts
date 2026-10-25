// Export HTML d'un rapport — partie navigateur : charge portraits et minimaps, les réduit et les
// embarque en data: URI (le fichier reste lisible hors ligne), rend via `renderReportHtml` (pur) et
// déclenche le téléchargement.

import { heroIcon, heroUniverse, mapImage, minimapImage } from "./api";
import { exportFileName, heroesOf, mapsOf, renderReportHtml } from "./reportHtml";
import type { ExportAssets } from "./reportHtml";
import type { ScoutingReport } from "./scouting";

/** Couleurs d'univers (miroir des tokens --u-* de theme.css) : le fichier exporté n'a pas le thème. */
export const UNIVERSE_HEX: Record<string, string> = {
  Warcraft: "#ef9f27",
  StarCraft: "#378add",
  Diablo: "#e24b4a",
  Overwatch: "#d85a30",
  Nexus: "#afa9ec",
};

/** Image distante → data: URI réduite (plus grand côté ≤ `max`, portraits recadrés au carré). */
async function toDataUri(url: string | null, max: number, square: boolean): Promise<string | null> {
  if (!url) return null;
  try {
    const res = await fetch(url);
    if (!res.ok || !(res.headers.get("content-type") ?? "").startsWith("image/")) return null;
    const bmp = await createImageBitmap(await res.blob());
    const side = square ? Math.min(bmp.width, bmp.height) : 0;
    const sw = square ? side : bmp.width, sh = square ? side : bmp.height;
    const scale = Math.min(1, max / Math.max(sw, sh));
    const canvas = document.createElement("canvas");
    canvas.width = Math.round(sw * scale);
    canvas.height = Math.round(sh * scale);
    const ctx = canvas.getContext("2d");
    if (!ctx) return null;
    ctx.drawImage(bmp, (bmp.width - sw) / 2, (bmp.height - sh) / 2, sw, sh, 0, 0, canvas.width, canvas.height);
    bmp.close();
    return canvas.toDataURL("image/webp", 0.82);
  } catch {
    return null;
  }
}

export interface ExportResult { fileName: string; bytes: number; portraits: number; heroes: number; maps: number }

export async function exportReportHtml(r: ScoutingReport): Promise<ExportResult> {
  const heroes = heroesOf(r);
  const maps = mapsOf(r);
  const assets: ExportAssets = { hero: {}, map: {}, ring: {} };
  await Promise.all([
    ...heroes.map(async (h) => {
      assets.ring[h] = UNIVERSE_HEX[heroUniverse(h) ?? "Nexus"] ?? UNIVERSE_HEX.Nexus;
      const d = await toDataUri(heroIcon(h), 128, true);
      if (d) assets.hero[h] = d;
    }),
    ...maps.map(async (m) => {
      const d = (await toDataUri(minimapImage(m), 900, false)) ?? (await toDataUri(mapImage(m), 900, false));
      if (d) assets.map[m] = d;
    }),
  ]);
  const generatedAt = new Date().toLocaleString("en-GB", { dateStyle: "medium", timeStyle: "short" });
  const html = renderReportHtml(r, assets, generatedAt);
  const blob = new Blob([html], { type: "text/html;charset=utf-8" });
  const fileName = exportFileName(r.title);
  const url = URL.createObjectURL(blob);
  const link = document.createElement("a");
  link.href = url;
  link.download = fileName;
  document.body.appendChild(link);
  link.click();
  link.remove();
  setTimeout(() => URL.revokeObjectURL(url), 10_000);
  return {
    fileName,
    bytes: blob.size,
    portraits: Object.keys(assets.hero).length,
    heroes: heroes.length,
    maps: Object.keys(assets.map).length,
  };
}
