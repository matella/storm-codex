# Spec — Fonds de minimap in-game pour la visionneuse 2D

**Date :** 2026-07-09 · **Statut :** design validé (brainstorm), prêt pour plan.
**Objectif :** remplacer le fond de la visionneuse 2D (aujourd'hui l'**art peint de loading-screen**,
visuellement chargé) par la **vraie minimap tactique in-game** (vue top-down schématique) — pour une
**lisibilité nettement meilleure** ET une **calibration plus précise** des positions de héros.

## Motivation / double gain
Aujourd'hui `mapImage()` sert `/images/battlegrounds/<slug>.png` = l'art peint du loading-screen
(texture chargée, voilée d'un dégradé). Deux problèmes :
1. **Lisibilité** : les pastilles héros se lisent mal sur l'art peint (ex. le violet chargé de Cursed
   Hollow).
2. **Calibration** : on normalise les positions par `MapSize` (= aire **jouable**) puis on les étire
   sur l'art peint, dont la **bordure décorative ne correspond pas** à l'aire jouable → léger décalage.

La **minimap in-game correspond ~1:1 à l'aire jouable / camera bounds** → plus propre à lire **et**
mieux calée. Deux gains d'un coup.

## Constat de sourcing (spike fait, 2026-07-09)
Les minimaps propres **ne sont PAS téléchargeables publiquement**. HeroesDataParser (écosystème
`heroes-images`, celui de HotsPatchNotes) ne publie que `loadingscreens` et `replaypreviews` — les deux
sont l'**art peint** (vérifié : `replayspreviewimage_cursedhollow.png` = la scène violette, pas une
minimap). Les vraies textures de minimap sont des `.dds`/`.tga` **dans les fichiers du jeu**
(`base.stormassets/Assets/Textures/`, et/ou `Minimap.tga` baké dans chaque archive de carte).
→ **Il faut les extraire de l'installation HotS** (sur le PC de jeu).

## Décisions verrouillées
1. **Fond = vraie minimap in-game** (full schématique, pas juste un contour ; on ne dessine pas notre
   propre schéma).
2. **Source = extraction depuis les fichiers du jeu** (PC), pas de scraping wiki, pas de dépendance à un
   téléchargement public.
3. **Distribution = assets bakés/vendorisés** (comme le référentiel : « distribuer la donnée produite,
   l'utilisateur ne scrape jamais, fallback baké »). Fallback : art peint actuel si pas de minimap.
4. **Cartes ciblées** : les ~20 battlegrounds standards joués (+ ARAM si minimap dispo). Priorité aux
   cartes réellement présentes dans l'archive du box.

## Lot 1 — Extraction (PC de jeu, opérateur, guidé par l'agent)
- **Outil** : **CascView** (zezula.net) — ouvre le CASC HotS en lecture seule, browse/extract par chemin.
- **Cibles candidates** (à confirmer sur la 1re carte) : `Minimap.tga` baké dans l'archive de chaque
  carte (convention SC2/HotS), **et/ou** une texture de minimap dédiée sous
  `base.stormassets/Assets/Textures/`.
- **Procédé** : **confirmer sur Cursed Hollow d'abord** (extraire la texture candidate, vérifier à l'œil
  que c'est bien le top-down propre voulu), **puis batch** sur les ~20 battlegrounds. Convertir
  TGA/DDS → **PNG**.
- **Nommage de sortie** : `<slug>.png` où `slug` = **même convention que `mapImage()`** (min., apostrophes
  supprimées, espaces→tirets ; ex. `cursed-hollow.png`, `braxis-holdout.png`).
- **Livraison** : l'opérateur dépose les PNG (sur le box `~/apps/storm-codex/…/minimaps` ou dans le repo).
- **Repli si `Minimap.tga` trop plat** : texture dédiée, ou version wiki pour cette carte précise
  (au cas par cas, non systématique).
- **Runbook détaillé** à écrire dans `docs/runbooks/` (étapes CascView exactes + conversion + slugs).

## Lot 2 — Vendoring + service (agent, box)
- Nouveau dossier servi `/images/minimaps/<slug>.png` (`IMAGES_DIR/minimaps`), servi comme `/images`
  existant (ServeDir).
- **Bakés dans l'image / vendorisés** : les PNG extraits sont livrés avec le produit (commit ou baked
  layer), **pas de fetch runtime** (cohérent suite-design). Si un futur `dim::vendor_*` doit les rafraîchir,
  ce sera depuis un snapshot produit, pas un scrape.
- **Pas de changement du pipeline HotsPatchNotes** (il continue de fournir l'art peint = fallback).

## Lot 3 — Front swap + calibration + vérif E2E (agent)
- **Front** : nouveau `minimapImage(map)` → `/images/minimaps/<slug>.png` ; `Replay2D.tsx` l'utilise en
  fond **prioritaire**, chaîne de fallback **minimap → art peint (`mapImage`) → dégradé**. Voile
  **allégé** (la minimap est déjà sombre/schématique) — juste un léger assombrissement pour que les
  pastilles ressortent.
- **Calibration** : la minimap ≈ camera bounds. Ré-vérifier la transform `MapSize`-normalisée **par
  carte** ; ajouter la **table de recadrage/échelle par carte** différée au MVP-1 (petites constantes
  `crop`/`scale` là où la minimap a une marge). Utiliser les `CameraBounds` du gamedata si ça aide. Le
  **flip-Y est déjà correct** (validé Cursed Hollow) — on n'y touche pas.
- **Vérif E2E** : comme au MVP-1 — serveur local + images réelles + replays imagés (Cursed Hollow +
  1–2 autres), preview navigateur : héros bien placés sur la minimap propre, lisibilité nettement
  meilleure, calibration serrée. Screenshots.

## Modèle de coordonnées (rappel)
Le crate/endpoint est **inchangé** — il émet déjà des coords normalisées `[0,1]` par `MapSize`. Toute la
calibration minimap est **côté front** (choix de l'image + éventuel `crop`/`scale` par carte au dessin).
Aucun bump `VIEWER_VERSION`, aucun changement de payload.

## Hors périmètre (YAGNI)
- Dessiner notre propre schéma (option écartée).
- Scraper les wikis automatiquement (repli manuel au cas par cas seulement).
- Toucher le pipeline HotsPatchNotes / HeroesDataParser.
- Minimaps animées / objectifs baked (on garde nos propres overlays).
- Extraction automatisée depuis le Mac (pas d'accès au CASC du PC ; c'est une tâche PC guidée).

## Risques / inconnues
- **Qualité/format de la texture extraite** : confirmé au Lot 1 sur Cursed Hollow avant le batch.
- **Couverture** : une carte sans minimap propre retombe sur l'art peint (fallback silencieux).
- **Marge de la minimap vs `MapSize`** : gérée par la table de recadrage par carte (Lot 3), calée à l'œil
  sur de vrais matchs (viser « clairement mieux », pas le pixel).
- **Conversion DDS→PNG** : CascView exporte, sinon un convertisseur (ImageMagick/texconv) — précisé au runbook.

## Découpage
1. **Extraction guidée (PC/CascView)** → PNG des ~20 battlegrounds (confirm Cursed Hollow → batch).
2. **Vendoring + service** `/images/minimaps/`.
3. **Front swap + calibration + vérif E2E**, puis STATUS + (option) redéploiement box.
