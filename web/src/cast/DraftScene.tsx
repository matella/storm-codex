import { useEffect, useState } from "react";
import { useDraft, sideOfStep, heroIcon, initials, type DraftState, type Side } from "../api";
import { isReplayReady, timerFraction } from "../draftOverlay";
import "./draft-overlay.css";

/** Portrait héros qui remplit son conteneur (splash) ; repli sur les initiales si pas d'image. */
function Portrait({ hero }: { hero: string }) {
  const icon = heroIcon(hero);
  if (icon) return <div className="por" style={{ backgroundImage: `url(${icon})` }} />;
  return <div className="por none">{initials(hero)}</div>;
}

export const SKINS = ["nexus", "glass", "tactical", "mono"];
const PHASE_SECONDS = 45;

/** Skin demandé par `?skin=` (repli `nexus`). */
export function skinFromSearch(search: string): string {
  const p = new URLSearchParams(search).get("skin") ?? "nexus";
  return SKINS.includes(p) ? p : "nexus";
}

/** Vue overlay du draft (Timeline sur son propre fond `.stage`) : partagée par `/draft/overlay` et la scène `draft`
 *  de `/cast/overlay`. */
export function DraftScene({ d, skin }: { d: DraftState | undefined; skin: string }) {
  return (
    <div className="draft-overlay" data-skin={skin}>
      <div className="stage"><div className="stars" />{d && <Timeline d={d} />}</div>
    </div>
  );
}

/** Scène `draft` de `/cast/overlay` : lit l'état du draft (WS `draft.updated`) et le skin (`?skin=`). */
export function DraftSceneLive() {
  const { data: d } = useDraft();
  return <DraftScene d={d} skin={skinFromSearch(window.location.search)} />;
}

/** Début (index) du bloc d'étapes consécutives du même rôle d'ordre contenant le curseur. */
function phaseStart(d: DraftState): number {
  if (d.cursor >= d.steps.length) return -1;
  const order = d.steps[d.cursor].order;
  let s = d.cursor;
  while (s > 0 && d.steps[s - 1].order === order) s--;
  return s;
}

function Timeline({ d }: { d: DraftState }) {
  const ps = phaseStart(d);
  const [secs, setSecs] = useState(PHASE_SECONDS);
  useEffect(() => {
    setSecs(PHASE_SECONDS);
    if (ps < 0) return;
    const id = setInterval(() => setSecs((s) => Math.max(0, s - 1)), 1000);
    return () => clearInterval(id);
  }, [ps]);

  // en mode replay le timer suit le prochain tick de lecture auto : horloge rafraîchie 4×/s
  const inReplay = d.replay?.current != null;
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    if (!inReplay) return;
    const id = setInterval(() => setNow(Date.now()), 250);
    return () => clearInterval(id);
  }, [inReplay]);

  const activeSide: Side | null = d.cursor < d.steps.length ? sideOfStep(d, d.steps[d.cursor]) : null;

  // picks dans l'ordre chrono + ordinal par côté (→ pseudo) + côté
  let bc = 0, rc = 0;
  const pickMeta = d.steps
    .map((s, i) => ({ s, i }))
    .filter(({ s }) => s.action === "pick")
    .map(({ s, i }) => {
      const side = sideOfStep(d, s);
      const ordinal = side === "blue" ? bc++ : rc++;
      return { i, side, ordinal };
    });

  const row = (side: Side) =>
    pickMeta.map((m, k) => {
      if (m.side !== side) return <div key={k} className="pk ghost" />;
      const hero = d.assignments[m.i];
      const player = (side === "blue" ? d.blue : d.red).players[m.ordinal] ?? "";
      return (
        <div key={k} className={`pk ${hero ? "" : "empty"} ${m.i === d.cursor ? "active" : ""}`}>
          {hero && <Portrait hero={hero} />}
          <span className="nm"><b>{hero ?? (m.i === d.cursor ? "…" : "—")}</b>{player && <i>{player}</i>}</span>
        </div>
      );
    });

  // bans dans l'ordre, avec un séparateur entre deux phases de bans (saut d'index = picks intercalés)
  const banSteps = d.steps.map((s, i) => ({ s, i })).filter(({ s }) => s.action === "ban");
  const bans: React.ReactNode[] = [];
  banSteps.forEach(({ s, i }, k) => {
    if (k > 0 && i - banSteps[k - 1].i > 1) bans.push(<span key={`sep${k}`} className="sep" />);
    const hero = d.assignments[i];
    const side = sideOfStep(d, s);
    bans.push(
      <span key={i} className={`bn ${side} ${hero ? "" : "empty"} ${i === d.cursor ? "active" : ""}`}>
        {hero && <Portrait hero={hero} />}
      </span>
    );
  });

  const fearless = d.format === "fearless" && d.series_bans.length > 0;

  return (
    <>
      <div className="tl-head">
        <div className="map">{d.map}{d.bo > 1 ? ` — Bo${d.bo}` : ""}</div>
        <div className="teams">
          <span className="b">{activeSide === "blue" && <span className="dot" />}{d.blue.name}</span>
          <span className="s">{d.score[0]} — {d.score[1]}</span>
          <span className="r">{d.red.name}{activeSide === "red" && <span className="dot" />}</span>
        </div>
        {isReplayReady(d) && <div className="ready">Draft ready — waiting</div>}
        <div className="tl-timer"><span style={{ width: `${timerFraction(d, now, secs, PHASE_SECONDS) * 100}%`, transition: inReplay ? "width .25s linear" : undefined }} /></div>
      </div>

      {fearless && (
        <div className="tl-series">
          <div className="lab">Series bans · {d.series_bans.length}</div>
          <div className="pool">{d.series_bans.map((h) => <span key={h} className="si"><Portrait hero={h} /></span>)}</div>
        </div>
      )}

      <div className="tl-stack">
        <div className="tl-picks b">{row("blue")}</div>
        <div className="tl-bans">{bans}</div>
        <div className="tl-picks r">{row("red")}</div>
      </div>
    </>
  );
}
