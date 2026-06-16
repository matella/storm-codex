# Spec — Storm Codex Suite (all-in-one HotS)

**Date :** 2026-06-16 · **Statut :** design validé (brainstorm), prêt pour plan d'implémentation.
**Objectif :** regrouper la suite HotS (stats + overlays + patch notes + uploader) en **un produit
unifié, auto-hébergeable, que n'importe qui lance avec `docker compose up`**.

## Vision
Aujourd'hui la suite est éclatée : storm-codex (Rust, stats/overlays), HotsPatchNotes (.NET, patch
notes + référentiel héros/talents), uploader client-rs (Rust, PC de jeu), Orpheus (Node, musique).
But : **un seul site web**, **une seule base**, **un seul `docker compose up`**, **pré-seedé**, que
quiconque peut prendre et faire tourner **en local** sans scraper ni configurer une stack complexe.

## Décisions verrouillées (avec rationale)
1. **Pas « une image » monolithique → un `docker compose` unique.** Mélanger Rust/.NET/Node dans une
   image est un anti-pattern. Pour l'utilisateur, `docker compose up` est tout aussi simple.
2. **Produit utilisateur = 100% Rust** : storm-codex (stats + overlays + **patch notes intégrés**) +
   uploader. **HotsPatchNotes (.NET) sort du bundle** et devient un **outil mainteneur d'ingestion**
   (scrape BlueTracker + fetch `heroes-talents`). Rationale : le scraping est un job de mainteneur,
   pas d'utilisateur ; le produit reste mono-stack, sans réécrire le scraper (qui marche).
3. **Un seul Postgres** partagé dans le compose (base `storm_codex` ; le référentiel patches/héros/
   talents vit aussi dedans, voir §Référentiel). Pas de SQLite/Mongo côté produit.
4. **Orpheus exclu** du bundle public (dépend d'une auth Spotify perso). Le widget musique dégrade en
   « Music — off ».
5. **Distribution : images pré-build publiées sur ghcr.io.** L'utilisateur ne build rien.
6. **Usage purement local → sécurité simple.** Mode ouvert par défaut (ADMIN_TOKEN optionnel, déjà
   en place), backups locaux basiques. Pas de sécu/auth avancée (reverse-proxy/TLS documenté si
   exposition, hors scope produit).

## Architecture
- **Repo `storm-codex-suite`** (nouveau, parapluie) : `docker-compose.yml` (référence les images
  ghcr.io) + `.env.example` + README. Aucun code applicatif.
- **Services du compose** : `postgres` (partagé) · `storm-codex-server` (Rust, sert l'API + le React
  + les overlays + les patch notes) · `uploader-headless` (option Docker, voir §Uploader).
- **storm-codex = LE site** : son React est l'unique front. On abandonne `HotsPatchNotes.Web`.

## Référentiel par snapshot (héros / talents / patches / images)
**Problème :** le produit ne doit pas dépendre d'un HotsPatchNotes vivant ni faire scraper l'utilisateur.
**Solution :** distribuer la **donnée déjà produite**, pas le scraper.
- **Production (mainteneur)** : HotsPatchNotes (.NET) exporte un **snapshot versionné** =
  `referential-<version>.tar.gz` (`heroes.json`, `talents.json`, `patches.json` + images portraits/
  cartes). Publié comme **GitHub Release** du repo suite.
- **Consommation (storm-codex)** : nouveau `REFERENTIAL_URL` (défaut = dernière release). Au démarrage
  + check périodique (~24 h) + bouton Admin : si version plus récente → télécharge, ingère dans
  Postgres (**pattern `dim_heroes`/`dim_talents` existant, étendu à `dim_patches`**), décompresse les
  images dans `images_dir`. Idempotent. **Fallback** : snapshot baké dans l'image si offline.
- **Conséquence** : l'utilisateur **ne scrape jamais, ne lance jamais .NET, n'a jamais de base vide**.

## Fraîcheur des patch notes
- Patch Blizzard → **GitHub Action en cron** (côté mainteneur) lance l'ingestion → produit le snapshot
  → publie une nouvelle Release.
- Utilisateurs : auto-frais au boot / périodiquement / via bouton, sous ≤24 h.

## Notifications « nouveau patch »
La détection d'une version de snapshot plus récente déclenche :
- **In-app** : broadcast WS `patch.new` → toast + pastille sur l'onglet « Patch Notes » (réutilise le
  système toast/WS existant des nouveaux replays).
- **Webhook sortant optionnel** : `PATCH_WEBHOOK_URL` configurable → POST `{patchName, type, date,
  link}`, compatible Discord / ntfy / endpoint générique (spine Jarvis chez le mainteneur). Off par
  défaut.

## Patch notes dans storm-codex (intégration complète)
- storm-codex-server **proxifie/sert** les patches depuis sa propre base (`dim_patches`), comme
  dim/now-playing aujourd'hui → une seule origine, pas de CORS.
- **Vues React (design Nexus Codex)** : onglet « Patch Notes » → liste (nom, type, date, nb héros/
  cartes) + détail d'un patch (changements héros/talents/cartes). Réutilise portraits/talents.

## Uploader headless (option Docker)
- Nouvelle image `uploader-headless` : client-rs **sans GUI**, config par env (URL serveur + token +
  chemin replays), **bind-mount** du dossier HotS, **re-scan périodique** (code déjà en place).
- Le `.exe` natif Windows reste l'option principale (file-watching fiable). L'image headless cible les
  utilisateurs tout-Docker (Docker Desktop + bind-mount).

## Onboarding & nouveautés (in-app)
- **Assistant 1er lancement** : au boot initial, parcours guidé → pseudo opérateur, **génération d'un
  token d'upload**, **URLs OBS** à copier, lien/commande pour brancher l'uploader. Re-déclenchable via
  un bouton « ? ». Page Admin « Connect the uploader ».
- **« What's new »** : à chaque montée de version de l'app, panneau des nouveautés depuis la dernière
  visite (version vue en localStorage → modale, dismiss = vu). Distinct des patch notes HotS.

## Sauvegardes (simple, local)
- Volume Postgres nommé + `pg_dump` planifié (script/cron léger) + **restore documenté**. Pas de
  chiffrement/offsite (usage local). Filet de sécurité pour l'historique de matchs.

## CI / publication d'images
- Chaque repo (storm-codex, HotsPatchNotes-api, uploader) gagne un **GitHub Actions** : build + push
  vers `ghcr.io/matella/*`. Le repo suite référence ces tags + une Action cron pour le snapshot.

## Hors scope (YAGNI)
i18n FR/EN · comptes multi-utilisateurs / auth avancée · thème clair · données de démo · réécriture de
HotsPatchNotes en Rust (seul l'ingester pourrait l'être un jour, optionnel).

## Découpage en lots (ordre de build)
1. **Postgres partagé** — HotsPatchNotes-api → Postgres (au lieu de SQLite) ; un seul service DB.
2. **Snapshot référentiel + `dim_patches`** — export .NET + ingestion Rust (boot/périodique/fallback).
3. **Vues patch notes** — onglet + liste + détail dans le React storm-codex.
4. **Notifs nouveau patch** — WS `patch.new` + toast/pastille + webhook optionnel.
5. **Image uploader headless** — Dockerfile sans GUI + config env + bind-mount.
6. **CI publication d'images** — Actions par repo → ghcr.io.
7. **Repo suite + compose + docs** — `docker compose up` de bout en bout.
8. **Onboarding + assistant 1er lancement** (+ page « connect uploader »).
9. **« What's new » / changelog in-app.**
10. **Sauvegardes/restore** (simple, local).

Chaque lot est livrable indépendamment ; de nouveaux lots peuvent s'ajouter sans rouvrir l'architecture.
