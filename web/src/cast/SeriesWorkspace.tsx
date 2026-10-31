// Espace de travail de la scène Series : partie montrée, diaporama (slide, auto/manuel, intervalle) et
// dépôt du replay Custom de chaque partie (→ post-game breakdown). Les erreurs serveur (`not_custom`,
// `parse_failed`, `incomplete`, `UnknownGame`…) remontent dans le bandeau de la régie via `run`.
import { useState } from "react";
import { fmtClock as clock } from "../api";
import { castGameReplay, castSlide } from "../castApi";
import { useSlideTick } from "./useSlideTick";
import type { CastSeries, GameRecord, GameReplayResult } from "../castApi";
import { INTERVAL_S, SLIDE_NAMES, intervalSeconds, replayErrorText, replayNote, shownGame, slideCount, slideIndex, slideLabel } from "../castState";

type Run = (fn: () => Promise<unknown>) => Promise<void>;

function GameRow({ g, run }: { g: GameRecord; run: Run }) {
  const [busy, setBusy] = useState(false);
  const [swap, setSwap] = useState(false);
  const [note, setNote] = useState<string | null>(null);
  const load = async (file: File | undefined) => {
    if (!file) return;
    setBusy(true);
    setNote(null);
    let res: GameReplayResult | null = null;
    try {
      await run(async () => {
        try {
          res = await castGameReplay(g.number, file, swap);
          return res;
        } catch (e) {
          throw new Error(replayErrorText(e instanceof Error ? e.message : String(e)));
        }
      });
      setNote(res ? replayNote(res) : null);
    } finally {
      setBusy(false);
      setSwap(false); // « Swap sides » vaut pour UN dépôt : décoché après chaque tentative
    }
  };
  const sm = g.summary;
  return (
    <div className="cr-game">
      <b>Game {g.number}</b> <span>{g.map}</span>{" "}
      <span className="cr-note">
        {sm
          ? `replay loaded · ${sm.winner} won · ${sm.takedowns[0]}–${sm.takedowns[1]} · ${clock(sm.duration_secs)}`
          : g.winner ? `${g.winner} won · no replay` : "no replay"}
      </span>
      <label className="cr-note" title="The series' Blue team played as the replay's second team">
        <input type="checkbox" checked={swap} onChange={(e) => setSwap(e.target.checked)} aria-label={`Game ${g.number} swap sides`} /> Swap sides
      </label>
      <label className="ds-btn small">
        {busy ? "Decoding…" : sm ? "Replace replay" : "Load replay"}
        <input type="file" accept=".StormReplay" hidden disabled={busy} aria-label={`Game ${g.number} replay file`}
          onChange={(e) => { void load(e.target.files?.[0]); e.target.value = ""; }} />
      </label>
      {note && <div className="cr-note cr-warn" role="status">{note}</div>}
    </div>
  );
}

/** Scène Series : jeu dont les données sont montrées, slide courante (‹ k / n ›), défilement auto/manuel, replays. */
export function SeriesWorkspace({ series, run }: { series: CastSeries; run: Run }) {
  const sl = series.slide;
  const shown = shownGame(series);
  const n = slideCount(series);
  const tick = useSlideTick(series);
  const idx = slideIndex(series, tick);
  const label = slideLabel(series, tick);
  return (
    <section className="cr-box">
      <div className="cr-k">Series · slides</div>
      <div className="cr-steppers">
        <label className="cr-note">Game shown{" "}
          <select className="ds-input" aria-label="Game shown" value={sl.game ?? 0} disabled={series.games.length === 0}
            onChange={(e) => run(() => castSlide({ game: Number(e.target.value) }))}>
            <option value={0}>Latest (auto){sl.game === null && shown ? ` · game ${shown.number}` : ""}</option>
            {series.games.map((g) => <option key={g.number} value={g.number}>Game {g.number} · {g.map}</option>)}
          </select>
        </label>
        <div className="cr-stepper">
          <button type="button" className="ds-btn small" aria-label="Previous slide" disabled={n <= 1}
            onClick={() => run(() => castSlide({ index: (idx + n - 1) % n }))}>‹</button>
          <b title={label.title}>{label.text}</b>
          <button type="button" className="ds-btn small" aria-label="Next slide" disabled={n <= 1}
            onClick={() => run(() => castSlide({ index: (idx + 1) % n }))}>›</button>
        </div>
        <span className="cr-note" title={label.title}>{sl.auto && n > 1 ? `rotating ${n} slides · same clock as the preview and OBS` : SLIDE_NAMES[idx]}</span>
        <button type="button" className={`ds-btn small${sl.auto ? " on" : ""}`} aria-pressed={sl.auto}
          onClick={() => run(() => castSlide({ auto: !sl.auto }))}>{sl.auto ? "Auto" : "Manual"}</button>
        <label className="cr-note">Every{" "}
          <input className="ds-input" type="number" min={INTERVAL_S.min} max={INTERVAL_S.max} step={1} aria-label="Slide interval (seconds)"
            style={{ width: 70 }} defaultValue={Math.round(sl.interval_ms / 1000)} key={sl.interval_ms}
            onBlur={(e) => {
              const v = intervalSeconds(e.target.value);
              if (v === null) { e.target.value = String(Math.round(sl.interval_ms / 1000)); return; }
              if (v * 1000 !== sl.interval_ms) run(() => castSlide({ interval_ms: v * 1000 }));
            }} /> s
        </label>
      </div>
      <div className="cr-note">
        {n > 1
          ? "Slide 1 = map strip, 2–5 = post-game breakdown of the shown game."
          : "Only the map strip: load the shown game's Custom replay to add the post-game breakdown."}
      </div>
      {series.games.length > 0 && (
        <div className="cr-games">{series.games.map((g) => <GameRow key={g.number} g={g} run={run} />)}</div>
      )}
    </section>
  );
}
