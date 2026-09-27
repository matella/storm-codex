# Frontend — SPA & overlays OBS

`web/` : Vite + React 18 + TS + TanStack Query + react-router + uPlot. Design **Nexus Codex ·
Dossier** (spec `docs/specs/2026-09-27-design-dossier-app-design.md`) : tokens (couleurs, polices)
dans `theme.css`, classes globales `ds-` dans `src/ds.css`, composants partagés
`src/components/ds/` (Portrait à tons, Pips, Meter, SecHead, Chips, Lane/CallCard, EvLine…).
Polices **auto-hébergées** (`src/fonts.ts`, Fontsource, SIL OFL : Big Shoulders Display, Figtree,
JetBrains Mono ; ~132 Ko en latin, `unicode-range`) — aucune requête Google Fonts. Chrome
(`Layout`) : barre `ds-top` (marque en titrage, onglets mono capitales, aide, indicateur live),
`AggFilterBar` restylé (pilules), `SafeHtml` = point unique d'insertion du HTML externe (fragment DOM
assaini par DOMPurify, `replaceChildren`), `Portrait` s'abonne au référentiel héros (re-rendu à son arrivée), pièces de partie dans
`components/ds/match.tsx` (ModeTag, AwardTag, GameCard, LevelAdvantage, art de carte),
conteneur `ds-app` (police Figtree, fond atmosphérique) et `ds-shell` (1180 px). Le `body` garde
sa police d'origine : les sources OBS (hors Layout) ne changent pas. Buildée dans `web/dist`, servie par le binaire (`WEB_DIR`),
fallback SPA (`index.html` en `no-cache`). Langue de l'UI : **anglais**.

## Routes (source de vérité : `web/src/App.tsx`)

### Pages (sous `Layout` — topbar + nav)

| Route | Page | Contenu |
|---|---|---|
| `/` | Dashboard | **dossier** : couverture de la dernière session de l'opérateur (parties séparées de < 3 h, « Tonight » si c'est aujourd'hui) — bilan en très grand, pastilles, dernière partie (portrait, award, lien), tuiles winrate / K/D/A / MVP / héros ; cartes des parties de la session ; « Your form » (30 derniers résultats, winrate, 10 derniers, par mode, héros les plus joués). Logique pure `src/session.ts` (tests). Sans `operator_names` : invite vers Admin |
| `/matches` | Matches | **dossier** : couverture « Archive » (nombre de parties, bilan de l'opérateur) avec les filtres pilotés par l'URL (mode, résultat, MVP, carte, héros, compte, dates, reset, export CSV/JSON) ; liste dense (date, mode court, portrait à ton victoire/défaite, carte + héros, résultat, MVP, durée, art de la carte en fond) |
| `/match/:id` | MatchDetail | **dossier** : couverture (minimap découpée, mode, verdict VICTORY/DEFEAT du point de vue de l'opérateur — sinon « BLUE/RED TEAM WINS » —, takedowns, niveau, premier objectif/fort/keep ; la couleur suit l'équipe réelle, l'équipe de l'opérateur en premier), onglets Score / Replay 2D + lien dump brut. Sections : Draft (bans barrés avec phase, résolus via `/api/dim/hero-attributes`, `no ban` pour un tour passé ; picks dans l'ordre, FIRST PICK, YOU), Scoreboard (jauges relatives au max de la partie, stats basic/advanced, talents nommés, awards, total), Level advantage (SVG en escalier, premier fort marqué), Team XP, Timeline (piste + liste), Match chat (filtres pings/callouts), Taunts & pings, Full data |
| `/player/:toon` | Player | **dossier** : couverture au portrait du héros le plus joué (anneau d'univers), alias, tuiles winrate / K/D/T / héros ; réserve de héros en barres (top 18, « show all ») ; parties récentes en lignes de partie |
| `/heroes` · `/hero/:name` | Heroes / Hero | **dossier** : mur de héros (portrait, winrate coloré, jauge, parties), tri « Most played / Win rate », filtre de rôle, filtres d'agrégat ; fiche héros : grand portrait à halo d'univers, tuiles (winrate, K/D/T, meilleure carte ≥ 5 parties), « See games », cartes en tuiles minimap (vert ≥ 55 %, rouge < 45 %), builds gagnants en pas de talents, historique de patchs dépliable (contenu via `SafeHtml`) |
| `/synergies` | Synergies | **dossier** : coéquipiers récurrents et héros affrontés en barres (winrate coloré, bilan), tri winrate / parties |
| `/patches` · `/patch/:id` | Patches / Patch | **dossier** : liste datée (bloc date, type, héros/cartes), filtre de type, accès « Changes by hero » ; détail : couverture, sommaire collant (classifications, liens héros), contenu en prose stylée — titres convertis par `patchHeadings` (`src/patchHtml.ts`, tests) puis assainis par `SafeHtml` (`keepIds`) ; chunk lazy |
| `/hero-changes` | HeroChanges | **dossier** : vue par héros (portrait, nombre de changements, dernière classification, historique dépliable) ou chronologique ; filtres héros / type / patch ; entrées partagées `PatchEntry` (`components/ds/patch.tsx`) |
| `/maps` | Maps | **dossier** : une tuile par carte sur son art (ton bilan et winrate, parties, victoire côté bleu, durée moyenne), clic → Matches filtré sur la carte ; filtres d'agrégat |
| `/trends` | Trends | **dossier** : histogramme de ton winrate par patch (24 derniers, ligne 50 %), puis tableau détaillé (parties, côté bleu, durée) |
| `/leagues` | Leagues | **dossier** : une section par ligue, cartes d'équipe (nom, roster en puces) ; gestion dans Admin |
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
  `useDimHeroes`/`useDimTalents`/`useDimHeroAttributes` (référentiels, `staleTime: Infinity`) ;
  `banHero(ban, attrs)` résout un ban brut (code attribut storm-stats) en nom de héros.
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
