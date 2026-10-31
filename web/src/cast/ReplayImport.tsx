// Import groupé des replays d'une série (rail, mode « batch ») : on dépose les `.StormReplay` de la série,
// chaque fichier est décodé sans rien écrire (`castReplayInspect`), le lot propose une carte du pool
// lançable par replay (`proposeImports`), l'opérateur corrige, puis l'import crée les parties dans l'ordre
// de date (`castReplayImport`). Un échec n'arrête pas les autres replays (import partiel).
import { useState } from "react";
import { castReplayImport, castReplayInspect } from "../castApi";
import type { CastSeries, GameSummary } from "../castApi";
import { proposeImports, replayErrorText, replayImportOrder, replayNote, startableMaps } from "../castState";

type Run = (fn: () => Promise<unknown>) => Promise<void>;

interface Item {
  key: number;
  file: File;
  summary?: GameSummary;
  error?: string;
  map: string | null;
  state: "ready" | "importing" | "done" | "failed";
  note?: string | null;
}

const when = (iso: string | null) => (iso ? iso.replace("T", " ").slice(0, 16) : "date unknown");

export function ReplayImport({ series, run }: { series: CastSeries; run: Run }) {
  const [items, setItems] = useState<Item[]>([]);
  const [busy, setBusy] = useState(false);

  const candidates = startableMaps(series);

  // Carte attribuable à un replay : une carte lançable, pas déjà visée par un autre replay du lot.
  const optionsFor = (it: Item) => {
    const taken = new Set(items.filter((o) => o.key !== it.key && o.map).map((o) => o.map));
    return candidates.filter((m) => m === it.map || !taken.has(m));
  };

  const addFiles = async (files: File[]) => {
    setBusy(true);
    const decoded = await Promise.all(
      files.map(async (file) => {
        try {
          return { file, summary: await castReplayInspect(file) };
        } catch (e) {
          return { file, error: replayErrorText(e instanceof Error ? e.message : String(e)) };
        }
      }),
    );
    setBusy(false);
    setItems((prev) => {
      let key = prev.reduce((k, it) => Math.max(k, it.key), 0);
      const added: Item[] = decoded.map((d) => ({ key: ++key, file: d.file, summary: d.summary, error: d.error, map: null, state: "ready" }));
      // propositions pour les nouveaux seulement : les choix déjà faits restent, leurs cartes sont retirées
      const taken = prev.map((it) => it.map).filter((m): m is string => m !== null);
      const free = candidates.filter((m) => !taken.includes(m));
      const props = proposeImports(added.map((it) => ({ map: it.summary?.map ?? null, played_at: it.summary?.played_at ?? null })), free);
      return [...prev, ...added.map((it, i) => ({ ...it, map: props[i] }))];
    });
  };

  const setMap = (key: number, map: string | null) =>
    setItems((prev) => prev.map((it) => (it.key === key ? { ...it, map } : it)));

  const importAll = () =>
    run(async () => {
      const order = replayImportOrder(items.map((it) => it.summary?.played_at ?? null));
      setBusy(true);
      for (const i of order) {
        const it = items[i];
        if (!it.map || it.state !== "ready") continue;
        setItems((prev) => prev.map((x) => (x.key === it.key ? { ...x, state: "importing" } : x)));
        try {
          const res = await castReplayImport(it.map, it.file);
          const note = res.orientation === "assumed"
            ? "no linked player to tell the sides apart: check Blue/Red in the Series tab"
            : replayNote(res);
          setItems((prev) => prev.map((x) => (x.key === it.key ? { ...x, state: "done", note } : x)));
        } catch (e) {
          const error = replayErrorText(e instanceof Error ? e.message : String(e));
          setItems((prev) => prev.map((x) => (x.key === it.key ? { ...x, state: "failed", error } : x)));
        }
      }
      setBusy(false);
    });

  const importable = items.filter((it) => it.map && it.state === "ready").length;

  return (
    <section className="cr-box" aria-label="Batch replay import">
      <div className="cr-k">Replays · batch import</div>
      {candidates.length === 0 && (
        <div className="cr-note">Mark the maps of this series as picked or decider in the pool first: a replay becomes a game of a picked map.</div>
      )}
      <label className="ds-btn small">
        {busy ? "Reading…" : "Add .StormReplay files"}
        <input type="file" accept=".StormReplay" multiple hidden disabled={busy}
          onChange={(e) => {
            const files = Array.from(e.target.files ?? []);
            e.target.value = "";
            if (files.length) void addFiles(files);
          }} />
      </label>
      {items.length > 0 && (
        <ol className="cr-games" aria-label="Replays to import">
          {items.map((it) => (
            <li key={it.key} className="cr-game">
              <b>{it.file.name}</b>{" "}
              {it.summary && (
                <span className="cr-note">
                  {when(it.summary.played_at)} · {it.summary.map ?? "unknown map"} · {it.summary.winner} won · {it.summary.takedowns[0]}–{it.summary.takedowns[1]}
                </span>
              )}
              {it.error && <span className="cr-note cr-warn">{it.error}</span>}
              {it.summary && it.state === "ready" && (
                <label className="cr-note">{" "}Game{" "}
                  <select className="ds-input" aria-label={`Game for ${it.file.name}`} value={it.map ?? ""}
                    onChange={(e) => setMap(it.key, e.target.value || null)}>
                    <option value="">skip</option>
                    {optionsFor(it).map((m) => <option key={m} value={m}>{m}</option>)}
                  </select>
                </label>
              )}
              {it.state === "importing" && <span className="cr-note">importing…</span>}
              {it.state === "done" && <span className="cr-note" role="status">imported{it.note ? ` · ${it.note}` : ""}</span>}
              {it.state === "failed" && <span className="cr-note cr-warn" role="alert">failed · {it.error}</span>}
            </li>
          ))}
        </ol>
      )}
      {items.length > 0 && (
        <button type="button" className="ds-btn primary cr-wide" disabled={busy || importable === 0} onClick={() => void importAll()}>
          Import {importable} replay{importable === 1 ? "" : "s"} as game{importable === 1 ? "" : "s"}
        </button>
      )}
    </section>
  );
}
