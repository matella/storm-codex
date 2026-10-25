// Vue complète d'une partie en cours (feed live). Composant autonome — niveau de visibilité et
// « je suis » mémorisés par navigateur — monté sur /live et destiné à l'état « en partie » de
// /companion. Spec : docs/specs/2026-10-24-feed-live-en-partie-design.md.
import { useEffect, useMemo, useState } from "react";
import { talentInfo, useDimTalents, useLive } from "../api";
import { Avatar } from "./Avatar";
import {
  abilityLabel,
  abilityLagSec,
  currentTalents,
  feedText,
  gameClock,
  isStale,
  loadMe,
  loadScope,
  recentBins,
  saveMe,
  saveScope,
  secondsSince,
  sortAbilities,
  splitTeams,
  teamName,
  type LivePlayer,
  type LiveScope,
  type LiveView,
} from "../live";

const SCOPES: { id: LiveScope; label: string; hint: string }[] = [
  { id: "self", label: "Me + scoreboard", hint: "Your abilities, plus what the in-game scoreboard shows everyone." },
  { id: "allies", label: "+ Allies", hint: "Adds your 4 allies' ability presses." },
  { id: "all", label: "Everything", hint: "Adds the enemy team's ability presses." },
];

const decamel = (s: string) => s.replace(/([a-z])([A-Z])/g, "$1 $2");
const talentName = (id: string) => talentInfo(id)?.name ?? decamel(id);

/** Horloge locale qui avance chaque seconde (retard des données, flux interrompu). */
function useNow(): number {
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    const t = setInterval(() => setNow(Date.now()), 1000);
    return () => clearInterval(t);
  }, []);
  return now;
}

export function LiveGame() {
  useDimTalents(); // peuple les noms de talents
  const [scope, setScopeState] = useState<LiveScope>(loadScope);
  const [manualMe, setManualMe] = useState<number | null>(null);
  const { data, isLoading, isError } = useLive(scope, manualMe);
  const now = useNow();

  // « Je suis » est propre à une partie : on le recharge quand la session change.
  const session = data?.session ?? null;
  useEffect(() => {
    setManualMe(session ? loadMe(session) : null);
  }, [session]);

  const setScope = (s: LiveScope) => {
    setScopeState(s);
    saveScope(s);
  };
  const setMe = (me: number | null) => {
    setManualMe(me);
    if (session) saveMe(session, me);
  };

  if (isLoading) return <div className="ds-empty">loading…</div>;
  if (isError) return <div className="ds-empty">The live feed is unreachable — is the server running?</div>;
  if (!data) {
    return (
      <div className="ds-empty ds-lv-idle">
        <b>Waiting for a game.</b>
        <span>The live view fills in once the uploader sends the first block of an in-progress game.</span>
      </div>
    );
  }
  return <LiveBoard v={data} now={now} scope={scope} setScope={setScope} setMe={setMe} />;
}

function LiveBoard({ v, now, scope, setScope, setMe }: {
  v: LiveView;
  now: number;
  scope: LiveScope;
  setScope: (s: LiveScope) => void;
  setMe: (me: number | null) => void;
}) {
  const heroOf = useMemo(() => {
    const m = new Map(v.players.map((p) => [p.player_id, p.hero ?? `Player ${p.player_id}`]));
    return (id: number) => m.get(id) ?? `Player ${id}`;
  }, [v.players]);
  const [team0, team1] = splitTeams(v.players);
  const stale = isStale(v, now);
  const lag = abilityLagSec(v);
  const sinceChunk = secondsSince(v.last_chunk_at, now);
  const myTeam = v.players.find((p) => p.player_id === v.me)?.team ?? null;
  const withAbilities = v.players.filter((p) => p.abilities !== null);

  return (
    <div className="ds-lv">
      <section className="ds-panel ds-lv-head">
        <div className="ds-lv-clock">
          <span className="ds-label">Game clock</span>
          <strong>{gameClock(v.tracker_loop, v.gates_loop)}</strong>
        </div>
        <div className="ds-lv-meta">
          <div className="ds-lv-line">
            <StatusTag v={v} stale={stale} />
            {v.map && <span className="ds-lv-map">{v.map}</span>}
          </div>
          <div className="ds-lv-line ds-lv-lag">
            {v.status === "ended" ? (
              <span>final data — game length {gameClock(v.tracker_loop, v.gates_loop)}</span>
            ) : (
              <>
                <span>stats up to {gameClock(v.tracker_loop, v.gates_loop)}</span>
                <span>abilities {lag == null ? "waiting for data" : `${lag}s behind the stats`}</span>
                <span>last data {sinceChunk}s ago</span>
              </>
            )}
          </div>
          {stale && <div className="ds-lv-warn">No new data for {sinceChunk}s — the game may be paused or disconnected.</div>}
          {v.status === "unreadable" && <div className="ds-lv-warn">Live data can't be decoded ({v.reason}). A game patch may have changed the format.</div>}
          {v.protocol.assumed_latest && (
            <div className="ds-lv-note">Decoded with the newest known protocol (the game build isn't readable mid-game).</div>
          )}
        </div>
        <div className="ds-lv-levels">
          <TeamLevel team={0} level={v.team_levels[0]} mine={myTeam === 0} />
          <TeamLevel team={1} level={v.team_levels[1]} mine={myTeam === 1} />
        </div>
      </section>

      <section className="ds-lv-controls">
        <div className="ds-pills" role="radiogroup" aria-label="Visibility">
          <span className="ds-label">Show</span>
          {SCOPES.map((s) => (
            <span
              key={s.id}
              role="radio"
              aria-checked={scope === s.id}
              tabIndex={0}
              title={s.hint}
              className={`ds-pill${scope === s.id ? " on" : ""}`}
              onClick={() => setScope(s.id)}
              onKeyDown={(e) => (e.key === "Enter" || e.key === " ") && setScope(s.id)}
            >
              {s.label}
            </span>
          ))}
        </div>
        <label className="ds-lv-me">
          <span className="ds-label">I am</span>
          <select
            className="ds-input"
            value={v.me ?? ""}
            onChange={(e) => setMe(e.target.value === "" ? null : Number(e.target.value))}
          >
            <option value="">{v.me == null ? "not detected — pick" : "auto"}</option>
            {[...team0, ...team1].map((p) => (
              <option key={p.player_id} value={p.player_id}>
                {teamName(p.team)} · {p.hero ?? `Player ${p.player_id}`}
              </option>
            ))}
          </select>
          {v.me_source && <small>{v.me_source === "explicit" ? "picked" : v.me_source === "lobby" ? "from lobby" : "only human player"}</small>}
        </label>
      </section>
      {scope === "all" && (
        <div className="ds-lv-warn">
          Everything includes enemy actions you can't see in game. That is a live information advantage, and how
          Blizzard views it is unknown.
        </div>
      )}

      <section className="ds-grid2 ds-lv-teams">
        <TeamBoard label={teamName(0)} players={team0} me={v.me} />
        <TeamBoard label={teamName(1)} players={team1} me={v.me} />
      </section>

      <section className="ds-grid2 ds-lv-lower">
        <div className="ds-panel flush">
          <div className="ds-panel-hd">
            <strong>Ability presses</strong>
            <small className="ds-lv-sub">key presses, not confirmed casts · IDs not named yet · last 3 min</small>
          </div>
          {withAbilities.length === 0 ? (
            <div className="ds-empty-row">
              {v.me == null && scope !== "all" ? "Pick who you are to see your abilities." : "Hidden at this visibility level."}
            </div>
          ) : (
            withAbilities.map((p) => <AbilityBlock key={p.player_id} p={p} gameLoop={v.game_loop} hasData={v.bytes.game > 0} you={p.player_id === v.me} />)
          )}
        </div>
        <div className="ds-panel flush">
          <div className="ds-panel-hd"><strong>Game feed</strong><small className="ds-lv-sub">newest first</small></div>
          {v.feed.length === 0 ? (
            <div className="ds-empty-row">Nothing yet.</div>
          ) : (
            <ol className="ds-lv-feed">
              {v.feed.map((f, i) => (
                <li key={`${f.loop}-${i}`} className={`k-${f.kind}${f.team === 0 ? " t0" : f.team === 1 ? " t1" : ""}`}>
                  <time>{gameClock(f.loop, v.gates_loop)}</time>
                  <span>{feedText(f, heroOf, talentName)}</span>
                </li>
              ))}
            </ol>
          )}
        </div>
      </section>
    </div>
  );
}

function StatusTag({ v, stale }: { v: LiveView; stale: boolean }) {
  if (v.status === "ended") return <span className="ds-status ready">game over</span>;
  if (v.status === "unreadable") return <span className="ds-status stale">unreadable</span>;
  if (stale) return <span className="ds-status stale">interrupted</span>;
  return <span className="ds-status analyzed ds-lv-pulse">live</span>;
}

function TeamLevel({ team, level, mine }: { team: 0 | 1; level: number; mine: boolean }) {
  return (
    <div className={`ds-lv-tl t${team}${mine ? " mine" : ""}`}>
      <span className="ds-label">{teamName(team)}{mine ? " · you" : ""}</span>
      <strong>{level || "—"}</strong>
    </div>
  );
}

function TeamBoard({ label, players, me }: { label: string; players: LivePlayer[]; me: number | null }) {
  const red = players[0]?.team === 1;
  return (
    <div className={`ds-dside ${red ? "red" : "blue"}`}>
      <div className="ds-dside-hd"><strong>{label}</strong><small className="ds-lv-sub">takedowns / deaths</small></div>
      {players.map((p) => (
        <div key={p.player_id} className={`ds-lv-prow${p.player_id === me ? " me" : ""}`}>
          <Avatar hero={p.hero} size={34} />
          <div className="ds-lv-pmain">
            <div className="ds-lv-pname">
              {p.hero ?? `Player ${p.player_id}`}
              {p.player_id === me && <span className="ds-tag you">you</span>}
            </div>
            <div className="ds-talents">
              {p.talents.length === 0 ? (
                <span className="ds-none">no talent yet</span>
              ) : (
                currentTalents(p.talents, (id) => talentInfo(id)?.tier ?? null).map((t, i) => (
                  <span key={t.tree_id + i} className="ds-talent" title={t.tier ? `Level ${t.tier}` : undefined}>
                    <b>{t.tier ?? i + 1}</b>
                    {talentName(t.tree_id)}
                  </span>
                ))
              )}
            </div>
          </div>
          <div className="ds-lv-kd"><b>{p.takedowns}</b>/<span>{p.deaths}</span></div>
        </div>
      ))}
    </div>
  );
}

function AbilityBlock({ p, gameLoop, hasData, you }: { p: LivePlayer; gameLoop: number; hasData: boolean; you: boolean }) {
  const list = sortAbilities(p.abilities ?? []);
  return (
    <div className="ds-lv-ab">
      <div className="ds-lv-ab-hd">
        <Avatar hero={p.hero} size={22} />
        <strong>{p.hero ?? `Player ${p.player_id}`}</strong>
        {you && <span className="ds-tag you">you</span>}
        <small>{list.reduce((n, a) => n + a.count, 0)} presses</small>
      </div>
      {list.length === 0 ? (
        <div className="ds-lv-ab-empty">{hasData ? "no ability press yet" : "waiting for the first ability data (it lags behind the stats)"}</div>
      ) : (
        list.map((a) => {
          const bins = recentBins(a.recent, gameLoop);
          const peak = Math.max(1, ...bins);
          return (
            <div key={`${a.link}-${a.cmd}`} className="ds-lv-ab-row">
              <code>{abilityLabel(a)}</code>
              <span className="ds-lv-spark" aria-label={`${a.recent.length} presses in the last 3 minutes`}>
                {bins.map((b, i) => <i key={i} style={{ height: `${b ? 20 + (80 * b) / peak : 0}%` }} />)}
              </span>
              <b>{a.count}</b>
            </div>
          );
        })
      )}
    </div>
  );
}
