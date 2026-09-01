# Frontend — SPA & overlays OBS

`web/` : Vite + React 18 + TS + TanStack Query + react-router + uPlot. Design **Nexus Codex**
(tokens dans `theme.css`, sombre). Buildée dans `web/dist`, servie par le binaire (`WEB_DIR`),
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
- Avatars : portraits vendorisés `/images` + anneau couleur d'univers (`useDimHeroes`) ;
  fallback initiales.
- Données du détail de match : **tout vient de `GET /api/matches/{id}`** (l'objet `match`
  storm-stats complet) — pas d'endpoint par sous-bloc ; un nouveau bloc UI se sert dans cet objet.
- Dev : `npm run dev` (port 5180 via `.claude/launch.json`), proxy `/api` + `/ws` → `:8088`
  (`vite.config.ts`). Build : `npm run build` (tsc strict puis Vite).
