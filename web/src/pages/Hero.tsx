import type { CSSProperties } from "react";
import { useQuery } from "@tanstack/react-query";
import { useParams, Link, useSearchParams } from "react-router-dom";
import {
  fetchHeroDetail, fetchHeroPatches, useDimTalents, useDimHeroes, talentInfo, heroUniverse, fmtTime, searchToAgg, aggToSearch, type AggFilter,
} from "../api";
import { AggFilterBar } from "../components/AggFilterBar";
import { SafeHtml } from "../components/SafeHtml";
import { Portrait, SecHead, heroRing } from "../components/ds";
import { hasMapArt, mapArt } from "../components/ds/match";

const tierNum = (k: string) => parseInt(k.match(/\d+/)?.[0] ?? "0", 10);
const pct = (w: number, g: number) => (g ? Math.round((100 * w) / g) : 0);
const tone = (p: number) => (p >= 55 ? "hot" : p < 45 ? "cold" : "");

/** Build de talents (TierNChoice → treeId) → pas de talents ordonnés par palier. */
function BuildSteps({ talents }: { talents: Record<string, string> }) {
  const picks = Object.entries(talents).filter(([k]) => /^Tier\d/.test(k)).sort((a, b) => tierNum(a[0]) - tierNum(b[0])).map(([, tid]) => tid);
  return (
    <div className="ds-bsteps">
      {picks.map((tid, i) => {
        const info = talentInfo(tid);
        return (
          <div key={i} className="ds-bstep" title={tid}>
            <span>LV {info?.tier ?? "?"}</span>
            <strong>{info?.name ?? tid.replace(/([a-z])([A-Z])/g, "$1 $2")}</strong>
          </div>
        );
      })}
    </div>
  );
}

/** Hero — couverture à grand portrait (couleur d'univers), cartes, builds gagnants, patchs. */
export function Hero() {
  const { name } = useParams();
  const [params, setParams] = useSearchParams();
  const filter: AggFilter = searchToAgg(params);
  useDimTalents();
  const dim = useDimHeroes();
  const { data, isLoading } = useQuery({ queryKey: ["hero", name, params.toString()], queryFn: () => fetchHeroDetail(name!, filter) });
  const setFilter = (f: AggFilter) => setParams(aggToSearch(f), { replace: true });
  const hero = name ?? "";
  const key = hero.toLowerCase().replace(/[^a-z]/g, "");
  const role = dim && Object.entries(dim).find(([n]) => n.toLowerCase().replace(/[^a-z]/g, "") === key)?.[1]?.role;
  const wr = data ? pct(data.wins, data.games) : 0;
  const kda = (data?.avg_takedowns ?? 0) / Math.max(1, data?.avg_deaths ?? 0);
  const maps = [...(data?.by_map ?? [])].sort((a, b) => b.games - a.games);
  const best = maps.filter((x) => x.games >= 5).sort((a, b) => b.wins / b.games - a.wins / a.games)[0];
  const hasGames = !!data && data.games > 0;

  return (
    <div className="ds-page">
      <header className="ds-hhead" style={{ "--ring": heroRing(hero) } as CSSProperties}>
        <Portrait hero={hero} size={180} className="xl" />
        <div className="ds-hhead-txt">
          <div className="ds-kicker">Hero · {heroUniverse(hero) ?? "Nexus"}{role ? ` · ${role}` : ""}</div>
          <h1 className="ds-title" style={{ cursor: "default" }}>{hero}</h1>
          <div className="ds-statline" style={{ marginTop: 16 }}>
            <div className="ds-stat"><span className="ds-label">Win rate</span><strong>{isLoading ? "…" : wr}<span>%</span></strong>
              <small>{data ? `${data.wins}–${data.games - data.wins} · ${data.games} games` : ""}</small></div>
            <div className="ds-stat"><span className="ds-label">Avg K / D / T</span>
              <strong>{data?.avg_kills ?? "—"}<span>/</span>{data?.avg_deaths ?? "—"}<span>/</span>{data?.avg_takedowns ?? "—"}</strong><small>KDA ratio {kda.toFixed(1)}</small></div>
            {best && <div className="ds-stat"><span className="ds-label">Best map · 5+ games</span><strong style={{ fontSize: 24 }}>{best.map}</strong>
              <small>{best.wins}/{best.games} · {pct(best.wins, best.games)}%</small></div>}
            <Link className="ds-btn primary" style={{ alignSelf: "center" }} to={`/matches?hero=${encodeURIComponent(hero)}`}>See games ›</Link>
          </div>
          <AggFilterBar value={filter} onChange={setFilter} mineLabel="My games" />
        </div>
      </header>

      {!isLoading && !hasGames && <div className="ds-empty" style={{ marginTop: 20 }}>No game on {hero} for this filter.</div>}

      {hasGames && data && (
        <>
          <section className="ds-sec">
            <SecHead num={1} title="Maps" sub="green ≥ 55% · red < 45%" />
            <div className="ds-mapts">
              {maps.map((x) => {
                const p = pct(x.wins, x.games);
                return (
                  <Link key={x.map} to={`/matches?hero=${encodeURIComponent(hero)}&map=${encodeURIComponent(x.map)}`}
                    className={hasMapArt(x.map) ? "ds-mapt" : "ds-mapt noart"} style={hasMapArt(x.map) ? mapArt(x.map) : undefined}>
                    <strong>{x.map}</strong>
                    <div className="ds-mapt-f"><span className={`wr ${tone(p)}`}>{p}<i>%</i></span><em>{x.wins}/{x.games}</em></div>
                    <i className={`ds-bar ${tone(p) === "hot" ? "win" : tone(p) === "cold" ? "loss" : ""}`}><b style={{ width: `${p}%` }} /></i>
                  </Link>
                );
              })}
            </div>
          </section>

          {data.builds.length > 0 && (
            <section className="ds-sec">
              <SecHead num={2} title="Builds that win" sub="most played talent paths" />
              <div className="ds-builds">
                {data.builds.map((b, i) => {
                  const p = pct(b.wins, b.games);
                  return (
                    <div key={i} className="ds-build">
                      <div className="ds-build-hd"><strong className={tone(p)}>{p}<i>%</i></strong><em>{b.wins}/{b.games} games</em></div>
                      <BuildSteps talents={b.talents} />
                    </div>
                  );
                })}
              </div>
            </section>
          )}
        </>
      )}

      {name && <HeroPatches hero={name} num={hasGames ? (data && data.builds.length > 0 ? 3 : 2) : 1} />}
    </div>
  );
}

const PTAG: Record<string, string> = { BUFF: "buff", NERF: "nerf", MIXED: "mixed", BUGFIX: "fix", REWORK: "upd", NEW: "upd" };

/** Sens héros → patch : ajustements de ce héros à travers les patch notes (récent d'abord),
 *  dépliables ; chaque entrée lie vers la section du patch concerné. */
function HeroPatches({ hero, num }: { hero: string; num: number }) {
  const { data } = useQuery({ queryKey: ["hero-patches", hero], queryFn: () => fetchHeroPatches(hero) });
  if (!data || data.length === 0) return null;
  return (
    <section className="ds-sec">
      <SecHead num={num} title="Patch history" sub={`${data.length} change${data.length > 1 ? "s" : ""} · from HotsPatchNotes`} />
      <div className="ds-patches">
        {data.map((p) => {
          const c = (p.classification ?? "").toUpperCase();
          return (
            <details key={p.patchInternalId + p.anchor} className="ds-patch">
              <summary>
                <span className="pdate">{p.liveDate ? fmtTime(p.liveDate).slice(0, 5) + "/" + p.liveDate.slice(0, 4) : "?"}</span>
                <span className={`ds-ptag ${PTAG[c] ?? "upd"}`}>{c || "UPDATE"}</span>
                <span className="pname">{p.patchName}{p.shortSummary ? ` — ${p.shortSummary}` : ""}</span>
                <Link className="popen" to={`/patch/${encodeURIComponent(p.patchInternalId)}#${p.anchor}`} onClick={(e) => e.stopPropagation()}>open ›</Link>
              </summary>
              {p.content && <SafeHtml className="pbody patch-content" html={p.content} />}
            </details>
          );
        })}
      </div>
    </section>
  );
}
