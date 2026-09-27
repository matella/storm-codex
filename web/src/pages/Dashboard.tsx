import { useQuery } from "@tanstack/react-query";
import { Link } from "react-router-dom";
import { fetchMatches, fmtDur, matchOperator, modeBadge, operatorNames, useSettings } from "../api";
import { Pips, Portrait, SecHead } from "../components/ds";
import { AwardTag, GameCard, mapArt } from "../components/ds/match";
import { byHero, byMode, kda, lastSession, myGames, pctOf } from "../session";

const MODE_NAME: Record<string, string> = { SL: "Storm League", HL: "Hero League", TL: "Team League", UD: "Unranked", QM: "Quick Match", ARAM: "ARAM", CUSTOM: "Custom" };
const modeName = (mode: number | null) => MODE_NAME[modeBadge(mode).short] ?? modeBadge(mode).short;
const modeFam = (label: string) => (label === "ARAM" ? "aram" : ["Storm League", "Hero League", "Team League", "Unranked"].includes(label) ? "sl" : "qm");

/** Session — la dernière soirée de l'opérateur en couverture, ses parties, puis sa forme récente. */
export function Dashboard() {
  useSettings(); // operator_names
  const { data: matches, isLoading } = useQuery({ queryKey: ["matches", "dashboard"], queryFn: () => fetchMatches({ limit: 500 }) });
  const configured = operatorNames().length > 0;
  const games = myGames(matches ?? [], matchOperator);
  const session = lastSession(games);

  if (isLoading) return <div className="ds-page"><div className="ds-empty" style={{ marginTop: 24 }}>loading…</div></div>;
  if (!configured || games.length === 0) {
    return (
      <div className="ds-page">
        <header className="ds-cover" style={{ marginTop: 18 }}>
          <div className="ds-kicker">Session</div>
          <h1 className="ds-title" style={{ cursor: "default" }}>No session yet</h1>
          <div className="ds-subtitle" style={{ cursor: "default" }}>
            {configured ? "None of your accounts appears in the archive yet — upload replays with the uploader." : "Tell Storm Codex which accounts are yours in Admin → operator names."}
          </div>
          <div className="ds-cover-foot"><Link className="ds-btn primary" to="/admin">Open Admin</Link><Link className="ds-btn" to="/matches">Browse matches</Link></div>
        </header>
      </div>
    );
  }

  const wins = session.filter((g) => g.won).length;
  const last = session[session.length - 1];
  const first = session[0];
  const played = session.reduce((s, g) => s + (g.m.length ?? 0), 0);
  const tot = session.reduce((s, g) => { const x = kda(g.me); return { k: s.k + x.k, d: s.d + x.d, a: s.a + x.a }; }, { k: 0, d: 0, a: 0 });
  const mvps = session.filter((g) => (g.me.award ?? "").includes("MVP")).length;
  const heroes = [...new Set(session.map((g) => g.me.hero).filter(Boolean))] as string[];
  const d0 = new Date(first.m.played_at ?? 0);
  const isToday = new Date().toDateString() === new Date(last.m.played_at ?? 0).toDateString();
  const hhmm = (iso: string | null) => (iso ? new Date(iso).toLocaleTimeString("fr-FR", { hour: "2-digit", minute: "2-digit" }) : "");
  const verdict = wins === session.length ? "Perfect night" : wins === 0 ? "Rough night" : wins > session.length - wins ? "Winning night" : wins === session.length - wins ? "Even night" : "Tough night";

  const form = games.map((g) => g.won);
  const formWins = form.filter(Boolean).length;
  const heroes6 = byHero(games).slice(0, 6);
  const maxHero = heroes6[0]?.games ?? 1;
  const modes = byMode(games, modeName);

  return (
    <div className="ds-page">
      <header className="ds-cover" style={{ marginTop: 18, ...mapArt(last.m.map) }}>
        <div className="ds-kicker">
          Session · {d0.toLocaleDateString("en-GB", { weekday: "short", day: "numeric", month: "long", year: "numeric" })} · {hhmm(first.m.played_at)} → {hhmm(last.m.played_at)}
        </div>
        <div className="ds-cover-grid">
          <div>
            <h1 className="ds-title" style={{ cursor: "default" }}>{isToday ? "Tonight" : "Last session"}</h1>
            <div className="ds-bigrec">{wins}<span>–</span>{session.length - wins}</div>
            <Pips games={session.map((g) => ({ won: g.won }))} />
            <div className="ds-subtitle" style={{ cursor: "default", marginTop: 6 }}>{verdict} — {session.length} game{session.length > 1 ? "s" : ""}, {fmtDur(played)} played</div>
          </div>
          <Link className="ds-lastgame" to={`/match/${last.m.id}`}>
            {last.me.hero && <Portrait hero={last.me.hero} size={132} tone={last.won ? "pick" : "loss"} />}
            <div>
              <span className="ds-label">Last game</span>
              <strong>{last.me.hero ?? "?"}</strong>
              <em>{last.m.map} · {modeName(last.m.mode)}</em>
              <span style={{ display: "flex", gap: 8, alignItems: "center", flexWrap: "wrap" }}>
                <span className={`ds-tag ${last.won ? "win" : "loss"}`}>{last.won ? "WIN" : "LOSS"}</span>
                <AwardTag raw={last.me.award} />
                <span className="ds-btn primary small">Open match ›</span>
              </span>
            </div>
          </Link>
        </div>
        <div className="ds-statline">
          <div className="ds-stat"><span className="ds-label">Win rate</span><strong>{pctOf(wins, session.length)}<span>%</span></strong><small>{wins}/{session.length}</small></div>
          <div className="ds-stat"><span className="ds-label">K / D / A</span><strong>{tot.k}<span>/</span>{tot.d}<span>/</span>{tot.a}</strong>
            <small>{((tot.k + tot.a) / Math.max(tot.d, 1)).toFixed(1)} KDA ratio</small></div>
          <div className="ds-stat"><span className="ds-label">MVP</span><strong>{mvps}</strong><small>of {session.length} game{session.length > 1 ? "s" : ""}</small></div>
          <div className="ds-stat"><span className="ds-label">Heroes</span><strong>{heroes.length}</strong><small>{heroes.join(" · ")}</small></div>
        </div>
      </header>

      <section className="ds-sec">
        <SecHead num={1} title={isToday ? "Tonight's games" : "Session games"} sub="newest first" />
        <div className="ds-gcards">{[...session].reverse().map((g) => <GameCard key={g.m.id} m={g.m} me={g.me} won={g.won} />)}</div>
      </section>

      <section className="ds-sec">
        <SecHead num={2} title="Your form" sub={`last ${games.length} games in the archive`} />
        <div className="ds-grid2">
          <div className="ds-panel">
            <span className="ds-label">Last {Math.min(30, form.length)} results · newest left</span>
            <Pips games={form.slice(0, 30).map((won) => ({ won }))} />
            <div className="ds-formstats">
              <div><strong>{pctOf(formWins, form.length)}<span>%</span></strong><em>win rate · {formWins}/{form.length}</em></div>
              <div><strong>{form.slice(0, 10).filter(Boolean).length}<span>/{Math.min(10, form.length)}</span></strong><em>last ten</em></div>
            </div>
            <span className="ds-label" style={{ marginTop: 22 }}>By mode</span>
            {modes.map((t) => (
              <div key={t.key} className="ds-modrow">
                <span><span className={`ds-mode ${modeFam(t.key)}`}>{t.key}</span></span>
                <i className="ds-bar"><b style={{ width: `${pctOf(t.wins, t.games)}%` }} /></i>
                <em>{pctOf(t.wins, t.games)}% · {t.wins}/{t.games}</em>
              </div>
            ))}
          </div>
          <div className="ds-panel">
            <span className="ds-label">Most played</span>
            {heroes6.map((h) => (
              <Link key={h.key} to={`/hero/${encodeURIComponent(h.key)}`} className="ds-hrow">
                <Portrait hero={h.key} size={34} />
                <span>{h.key}</span>
                <i className="ds-bar"><b style={{ width: `${Math.round((h.games * 100) / maxHero)}%` }} /></i>
                <em>{h.games}g · {pctOf(h.wins, h.games)}%</em>
              </Link>
            ))}
          </div>
        </div>
      </section>
    </div>
  );
}
