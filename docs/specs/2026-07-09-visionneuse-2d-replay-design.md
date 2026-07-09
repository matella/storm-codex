# Spec — Visionneuse 2D de replay (intégrée à Storm Codex)

**Date :** 2026-07-09 · **Statut :** design validé (brainstorm), prêt pour plan d'implémentation.
**Objectif :** reconstruire une **abstraction 2D top-down** d'un match HotS à partir des données de
replay déjà décodées, avec une barre de défilement (scrub) à **seek instantané** (< 50 ms), **intégrée
comme onglet du détail de match** dans la SPA existante. Outil de revue de positionnement/décisions,
**pas** un remplacement du client 3D in-game.

## Contexte & constat de départ
La spec source (« HotS 2D Replay Viewer ») était écrite comme si le projet n'avait qu'un « parser Rust
brut ». En réalité, Storm Codex est un produit complet : serveur (archive replay = source de vérité,
décodage à la volée + cache, Postgres, WS), SPA React (design Nexus Codex), portraits/cartes vendorisés.
La visionneuse s'y **intègre** au lieu de reconstruire cette infra.

**`storm-replay` décode déjà, bit-exact, tous les streams nécessaires** :
- `tracker_events()` → `SUnitPositionsEvent`, `SUnitBornEvent`, `SUnitDiedEvent`, `SUnitRevivedEvent`,
  `SStatGameEvent`.
- `game_events()` / `visit_game_events()` → `SCmdEvent` / `SCmdUpdateTargetPointEvent` (avec `TargetPoint`).
- `details()` / `attributes()` → carte, joueurs, mode.

Le travail « Phase 1 » n'est donc **pas** du décodage : c'est une **couche d'extraction/projection**
qui consomme les événements déjà décodés et produit le modèle de la visionneuse.

## Vérités terrain (mesurées sur un vrai replay — Silver City ARAM)
1. **Calibration des coordonnées = quasi gratuite.** Le replay porte l'étendue de carte via
   `SStatGameEvent{ GameStart, MapSizeX, MapSizeY }` (ex. 1 015 808 × 884 736). Toutes les coordonnées
   se normalisent dans cette boîte (coords tracker déjà en espace « tuiles » ~0–250 ; `TargetPoint` des
   game events dans le même espace après division du point-fixe ×4096). **On place donc les héros
   correctement sur une minimap avec les seules données de replay** — pas de table de bornes par carte,
   pas de calage manuel. Le gamedata `jamiephan/HeroesOfTheStorm_Gamedata` ne sert qu'à fournir
   l'**image de minimap** par carte (+ éventuel petit recadrage si `MapSize` inclut une bordure injouable).
2. **Densité de mouvement héros = abondante.** ~20 831 `SCmdUpdateTargetPointEvent` + 15 847 `SCmdEvent`
   (chacun avec `_userid` + un `TargetPoint` monde) contre seulement 38 `SUnitPositionsEvent`. On ancre
   sur les positions exactes (rares) et on densifie avec les cibles de commande (fréquentes).
3. **Aucune donnée HP/mana dans le replay.** Énumération de tous les types d'événements tracker d'un vrai
   match : `SUnitBornEvent`, `SUnitDiedEvent`, `SUnitPositionsEvent`, `SStatGameEvent`, `SUpgradeEvent`,
   `SUnitOwnerChangeEvent`, `SUnitRevivedEvent`, `SUnitTypeChangeEvent`, `SScoreResultEvent`. **Aucun** ne
   porte de vie/mana continu par unité. HotS re-simule les vitals — ce qui est un **Non-Objectif permanent**
   (« pas de re-simulation moteur »). → **US-13 (barres HP/mana) abandonné** (rejoint le fog-of-war en
   « Won't have »). À la place : **état vivant/mort par héros** (Born/Died/Revived, données solides) +
   **marqueurs de mort**. `SUnitRevivedEvent` existe → adoucit l'avertissement « revives glitchent ».

## Décisions verrouillées
1. **Intégré à Storm Codex** (pas d'outil standalone). Web = la SPA existante ; format d'échange = JSON
   sur HTTP ; génération **à la demande + cache**, jamais matérialisée en masse dans Postgres.
2. **Seek 100 % côté client.** Le serveur émet **toute la timeline du match en une fois** ; le scrub est
   une recherche + interpolation en mémoire navigateur. Budget payload : les ~37 k events de commande ne
   deviennent pas tous des samples — on ne garde que les `TargetPoint` de déplacement (pas chaque cast),
   on déduplique les points quasi-immobiles et on quantifie les floats normalisés (~3 décimales), ce qui
   maintient le JSON dans l'ordre de quelques centaines de Ko. →
   **les « snapshots toutes les N s » de la spec source deviennent inutiles en V1** (ne servent qu'au seek
   serveur ou aux payloads énormes — non applicable ici).
3. **Calibration pilotée par la donnée** (via `MapSize`), images de minimap depuis `jamiephan`, fallback
   sur les images de carte déjà vendorisées / gradient voilé.
4. **Identité héros réutilisée depuis Postgres** : le replay fournit le lien unité→joueur ; le nom
   canonique / portrait / couleur d'univers vient de la **projection de match existante** (déjà corrigée
   du shuffle ARAM via `dim_talents`). La visionneuse n'a jamais à re-résoudre l'identité du héros.

## Périmètre MVP-1 (tranche verticale mince — dé-risque la calibration d'abord)
Fond de minimap correct · 10 pastilles héros (remplissage = **couleur d'équipe** ; l'anneau du portrait =
`universeColor`) + portraits · barre de scrub à seek
instantané côté client · **atténuation vivant/mort + marqueurs de mort** (remplace HP/mana) · **pas
d'animation play/pause encore**. But : prouver la calibration des coordonnées + le seek instantané sur de
vrais matchs, avant d'investir dans l'animation/polish.

**Fast-follows** (après validation MVP-1) : lecture animée play/pause 0,5×–8× (Phase 2 restante) ;
structures vivant/détruit (données déjà dans les tracker events — quasi gratuit) ; kill-feed cliquable ;
indicateurs de talents/niveau ; puis Phase 4 (objectifs par carte) au cas par cas.

## Architecture
- **Nouveau crate `storm-replay-viewer`** (membre du workspace), dépend de `storm-replay` uniquement.
  Consomme les événements décodés → produit le modèle visionneuse. Rationale : `storm-stats` (port fidèle
  hots-parser, sous diff de parité vert) **n'est pas** pollué par l'extraction positionnelle. Trois rôles
  nets : `storm-replay` décode · `storm-stats` fait la parité stats · `storm-replay-viewer` projette les
  positions.
- **Endpoint serveur** `GET /api/match/{id}/replay2d` → JSON. Réutilise le chemin
  fetch-archive-brute + décodage (celui derrière `…/raw`) et le modèle à-la-demande + cache existant.
  Cache disque clé `(hash replay, viewer_version)` : le calcul coûte un parse (~130 ms), donc premier
  ouverture calcule, ré-ouvertures instantanées (satisfait US-6). Aucun nouvel « étage » de données.
- **Front** : **onglet « Replay 2D » dans la page de détail de match** existante (pas de route séparée).
  Rendu sur **`<canvas>`** (substrat correct pour l'animation + minions/camps à venir ; évite une
  réécriture). Réutilise tokens Nexus Codex, portraits vendorisés, pattern `mapImage()` (+ fallback).

## Flux de données
détail de match → onglet « Replay 2D » → `fetch /api/match/{id}/replay2d` → le client détient tout le
modèle → la barre de scrub pilote une fonction pure `seek(t)` → rendu canvas.

## Modèle JSON (émis par le serveur ; **coordonnées pré-normalisées `[0,1]`**)
```
{
  meta:   { mapName, mapSize:{x,y}, durationSec, loopOffset:-610, viewerVersion },
  players:[ { playerId, userId, name, hero, team, universeColor } ],   // depuis la projection Postgres
  heroes: [ { playerId,
              samples:[ {t, x, y, exact} ],   // t=sec (corrigé offset), exact=true si SUnitPositionsEvent
              life:   [ {from, to} ] } ],      // intervalles « vivant » depuis Born/Died/Revived
  deaths: [ {t, x, y, victimPlayerId, killerPlayerId} ],
  warnings:[ "objective data unavailable: <map>" ]   // stub V1, US-7 plus tard
}
```

## Règles d'extraction
- **Base de temps (US-3)** : `t = (gameloop − 610) / 16`, appliqué une seule fois au boundary.
- **Normalisation** : `mapSize` depuis `SStatGameEvent{GameStart}` ; `x' = worldX / mapSizeX` (les
  `TargetPoint` des game events sont d'abord divisés par leur point-fixe ×4096 pour retomber dans le même
  espace que les coords tracker). Un seul endroit possède la transformation.
- **Positions héros** : ancrage sur les `SUnitPositionsEvent` exacts (`exact:true`) ; densification via
  `SCmdUpdateTargetPointEvent`/`SCmdEvent`.`TargetPoint` clé par `_userid` (`exact:false`). Le client
  interpole linéairement entre échantillons (artefact blink/dash accepté par la spec source).
- **Lien unité→joueur** : `SUnitBornEvent.m_controlPlayerId` + tags d'unité pour suivre l'unité-héros de
  chaque joueur ; les game events utilisent `_userid.m_userId`. Le **nom** du héros ne vient PAS du replay
  (immunité au shuffle) mais de la projection Postgres.
- **État de vie** : intervalles Born (spawn) → Died → Revived. Héros mort = atténué ; marqueur de mort
  visible ~4 s de scrub autour de chaque `t` de mort.
- **Morts (`deaths[]`)** : depuis `SUnitDiedEvent` de l'unité-héros suivie — `x,y` = `m_x`/`m_y`
  (normalisés `[0,1]`), `t` corrigé offset, `victimPlayerId` = joueur propriétaire de l'unité morte (via le
  lien unité→joueur), `killerPlayerId` = `m_killerPlayerId`. `m_killerPlayerId = 0` ou absent (mort
  environnementale / par structure) → `killerPlayerId: null`.

## Seek client
`seek(t)` pure : pour chaque héros, recherche binaire dans `samples`, lerp entre les deux encadrant `t`,
résolution vivant/mort par intervalle. Zéro round-trip → garantit < 50 ms.
**Interpolation vs intervalle mort** : ne pas lerper à travers un trou mort→respawn (le respawn est à la
base, loin du lieu de mort → la pastille glisserait à travers la carte pendant qu'elle est atténuée).
Règle V1 : pendant un intervalle mort, figer la pastille sur la dernière position vivante (pas de lerp),
puis sauter à la position de respawn au retour en vie.

## Cas limites / erreurs
- Carte sans image de minimap → fallback gradient voilé existant (déterministe, silencieux) ; héros
  quand même bien placés via coords normalisées.
- Replay non décodable / non archivé → l'onglet affiche « replay indisponible » (réutilise le statut
  archive/reprocess déjà exposé).
- Surbalayage bordure injouable de `MapSize` → constante de recadrage par carte optionnelle ; MVP-1 sans,
  on juge à l'œil sur de vrais matchs.

## Tests
- **Extraction** (crate) : tests unitaires sur le mini-corpus committé — maths d'offset, bornes de
  normalisation `[0,1]`, segmentation des intervalles de vie ; un **golden-JSON** snapshot pour un replay
  connu (exclure `viewerVersion` du comparé, ou régénérer volontairement le golden à chaque bump de
  version, pour que le test ne devienne pas bruyant à chaque raffinement d'extraction).
- **Client** : test unitaire de `seek(t)`.
- **Bout-en-bout** : vérification visuelle via les outils de preview sur un vrai match du backfill.

## Hors périmètre (rappel Non-Objectifs — permanents)
Rendu 3D · fog of war / vision par joueur · hitbox/géométrie exacte des sorts · multi-joueur live ·
re-simulation moteur complète · **HP/mana continu (donnée absente du replay)**.

## Découpage en lots (ordre de build)
1. **Crate `storm-replay-viewer`** : extraction → modèle JSON (meta, players via Postgres au niveau
   serveur, heroes samples + life, deaths) ; tests unitaires + golden-JSON.
2. **Endpoint `/api/match/{id}/replay2d`** : chemin décodage-à-la-demande + cache disque `(hash, version)`.
3. **Onglet front « Replay 2D »** : fetch modèle, fond minimap (jamiephan + fallback), 10 pastilles +
   portraits, barre de scrub, `seek(t)` client, atténuation vivant/mort + marqueurs de mort.
4. **Vérif visuelle** sur un vrai match ; calage/recadrage minimap si nécessaire. → MVP-1 livré.

Chaque lot suivant (animation, structures, kill-feed, objectifs par carte) est livrable indépendamment
sans rouvrir l'architecture.
