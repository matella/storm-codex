# Frontend — SPA & overlays OBS

`web/` : Vite + React 18 + TS + TanStack Query + react-router + uPlot. Design **Nexus Codex ·
Dossier** (spec `docs/specs/2026-09-27-design-dossier-app-design.md`) : tokens (couleurs, polices)
dans `theme.css`, classes globales `ds-` dans `src/ds.css`, composants partagés
`src/components/ds/` (Portrait à tons, Pips, Meter, SecHead, Chips, Lane/CallCard, EvLine…).
Polices **auto-hébergées** (`src/fonts.ts`, Fontsource, SIL OFL : Big Shoulders Display, Figtree,
JetBrains Mono ; ~132 Ko en latin, `unicode-range`) — aucune requête Google Fonts. Chrome
(`Layout`) : barre `ds-top` (marque en titrage, onglets mono capitales, aide, indicateur live),
conteneur `ds-app` (police Figtree, fond atmosphérique) et `ds-shell` (1180 px). Le `body` garde
sa police d'origine : les sources OBS (hors Layout) ne changent pas. Buildée dans `web/dist`, servie par le binaire (`WEB_DIR`),
fallback SPA (`index.html` en `no-cache`). Langue de l'UI : **anglais**.

## Routes (source de vérité : `web/src/App.tsx`)

### Pages (sous `Layout` — topbar + nav)

| Route | Page | Contenu |
|---|---|---|
| `/` | Dashboard | session courante, perspective opérateur (`operator_names`) |
| `/matches` | Matches | liste filtrable (mode/carte/héros/joueur/dates), export CSV/JSON, temps réel WS |
| `/match/:id` | MatchDetail | score 2 équipes (stats basic/advanced, talents nommés, awards/MVP), draft, **Match chat** (chat + filtres pings/callouts), level advantage (uPlot), XP, timeline des événements, table BM/pings, lien dump brut |
| `/player/:toon` | Player | résumé + hero pool |
| `/heroes` · `/hero/:name` | Heroes / Hero | agrégats triables ; fiche héros + patchs le concernant |
| `/synergies` | Synergies | paires (avec/contre) |
| `/patches` · `/patch/:id` | Patches / Patch | patch notes (DOMPurify, chunk lazy) |
| `/hero-changes` | HeroChanges | sections héros des patchs (buff/nerf) |
| `/maps` | Maps | agrégat par carte |
| `/trends` | Trends | winrate/durée par patch |
| `/leagues` | Leagues | équipes groupées par ligue |
| `/draft` | Draft | console opérateur du simulateur de draft |
| `/scouting` | Scouting | style « dossier » : couverture (nom d'équipe + titre → New report), liste des rapports (plus récent d'abord : équipe en titrage, titre, bilan, nb de replays, période, statut) |
| `/scouting/:id` | ScoutingReport | **page « dossier »**, même langage visuel que l'export HTML : couverture (équipe et titre éditables en place, bilan + pastilles, first pick, durée, avertissement petit échantillon, médaillons du roster, statut, actions Copy pack / .md / .xlsx / Import analysis / **Export HTML** / Delete), barre de sections collante (une puce par carte), bandeaux (lot ambigu → ancre, replays exclus, plan périmé, pas de plan), puis sections numérotées : Draft identity, Map choice, **Map by map** (tableau de draft par carte : minimap, confiance, We ban / We pick / They will likely pick, Consider, ce qu'ils ont joué/banni), Any map, Their players, What they faced, Game flow, **Replays** (dépôt glisser-déposer, journal des parties ; cliquer un côté = côté scouté manuel, `auto` / `remove`), **Roster** (cases roster, ancres ⚓, Save roster). Composants `components/ds/`, classes `ds-` (`src/ds.css`). Spec `docs/specs/2026-09-26-rapports-scouting-design.md` |
| `/admin` | Admin | santé uploads, tokens, équipes/collections, réglages, reprocess |

### Sources OBS standalone (fond transparent, hors Layout)

| Route | Usage OBS |
|---|---|
| `/widget?me=<nom>` | post-game : V/D, héros, carte, K/A/D + KP, phrase Jarvis |
| `/queue` | scène entre-games 1920×1080 (panneau session, slots cam/game, musique) |
| `/ticker` | bandeau défilant |
| `/now-playing` | piste en cours (proxy Orpheus) — carte étoffée : pochette, album, progression |
| `/now-playing?mini` | badge compact 290×68 : pochette + titre + artiste |
| `/now-playing?reveal` | annonce en grand (300×408) à chaque démarrage de lecture, tenue 2,6 s, puis repli en fondu sur le badge compact. Sondage 2 s (les deux autres variantes restent à 5 s). Spec `docs/specs/2026-08-29-now-playing-reveal-design.md` |
| `/draft/overlay` | overlay de draft 1920×1080 (état par WS `draft.updated`) |

## Conventions

- **Couche API unique** : `web/src/api.ts` — types, fetchers, `useLiveUpdates` (WS + reconnexion
  2 s, invalidation TanStack), helpers d'affichage (`modeBadge`, `fmtTime`…), caches
  `useDimHeroes`/`useDimTalents` (référentiels, `staleTime: Infinity`).
- Les **constantes miroir** du parser (modes 500xx, `MessageType`, `MessageTarget`…) sont
  répliquées là où le front les affiche — si `constants.json` (storm-stats) bouge, synchroniser.
- Scouting : types + helpers purs dans `web/src/scouting.ts` (tests `scouting.test.ts`), hooks
  `useScoutingList`/`useScoutingReport` et `scoutingWrite` (Bearer du token admin en
  localStorage) dans `api.ts` ; export HTML : `reportHtml.ts` (rendu pur) + `reportExport.ts` ;
  pages : composants `components/ds/` + classes `ds-` de `src/ds.css` (design system partagé).
- Avatars : portraits vendorisés `/images` + anneau couleur d'univers (`useDimHeroes`) ;
  fallback initiales.
- Données du détail de match : **tout vient de `GET /api/matches/{id}`** (l'objet `match`
  storm-stats complet) — pas d'endpoint par sous-bloc ; un nouveau bloc UI se sert dans cet objet.
- Dev : `npm run dev` (port 5180 via `.claude/launch.json`), proxy `/api` + `/ws` → `:8088`
  (`vite.config.ts`). Build : `npm run build` (tsc strict puis Vite).
