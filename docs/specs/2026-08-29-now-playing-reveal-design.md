# Now Playing — reveal animé (grande carte → badge mini) — design

> Spec storm-codex. Brainstorming validé par l'opérateur le 2026-08-29.
> Maquette animée de référence : `docs/specs/2026-08-29-now-playing-reveal-mockup.html`
> (antérieure à la décision « titre sur deux lignes » : sur ce point, la spec fait foi).
> Cible : `writing-plans` puis implémentation TDD.

## But

Quand un morceau démarre, l'overlay musique s'affiche **en grand** (grande pochette, titre et
artiste dessous), tient ~2,6 s, puis **se replie en douceur** sur le badge compact déjà en place
en haut à droite. Le badge reste ensuite affiché jusqu'au morceau suivant.

Aujourd'hui `/now-playing?mini` affiche directement le badge compact : la piste change sans que
rien ne l'annonce. Le reveal donne au titre un moment de lisibilité à l'antenne, sans occuper le
coin plus longtemps qu'aujourd'hui le reste du temps.

## Périmètre

**Troisième variante de la route existante** : `/now-playing?reveal`. La variante par défaut
(carte étoffée) et `?mini` ne bougent pas — la source OBS actuelle continue de fonctionner à
l'identique, on bascule en éditant l'URL de la browser source. Si la variante s'impose à l'usage,
on pourra la promouvoir en défaut de `?mini` plus tard ; ce n'est pas décidé ici.

## Décisions actées (brainstorming)

1. **Morph choisi : « Dissolve »** (option A de la maquette). La boîte s'anime en taille ; les deux
   contenus (grand / mini) se fondent l'un dans l'autre. Retenu parce qu'il conserve le style
   existant et réemploie le balisage du badge mini tel quel. Les options écartées : « Morph »
   (pochette et texte voyageant réellement — impose un texte aligné à gauche dans les deux états,
   sinon l'alignement saute en vol) et « Swap » (deux cartes distinctes, coupe franche).
2. **Déclencheur : tout démarrage de lecture.** Changement de piste, reprise après pause, et
   premier montage de la page avec quelque chose en cours. Motivation opérateur : cohérence, et
   il ne change pas de scène. **Conséquence assumée** : un rechargement de la browser source
   (démarrage du stream, redémarrage d'OBS, refresh) réannonce le morceau déjà en cours.
3. **Fréquence de sondage : 2 s** (au lieu de 5 s). Sous la règle 2, une reprise après pause
   déclenche un reveal ; à 5 s le reveal arrive visiblement en retard sur la musique.
4. **Skip rapide : retargeting sur place.** Une nouvelle piste pendant que la grande carte est
   affichée remplace le contenu et **relance le hold**, sans repasser par le badge mini. Évite le
   yoyo de taille quand on enchaîne les skips.
5. **Timings** : entrée 440 ms, hold 2,6 s, morph 620 ms, easing `cubic-bezier(.22,.9,.24,1)`.
   Réglés à la main sur la maquette et validés en l'état.
6. **Contenu de la grande carte** : pochette, kicker `NOW PLAYING` + égaliseur, titre, artiste.
   **Pas d'album ni de barre de progression** — c'est une carte de 2,6 s, pas un lecteur.

## Comportement

Machine à trois états : `hidden` → `big` → `mini`.

| Événement | État courant | Transition |
|---|---|---|
| `playing` passe faux → vrai | `hidden` | → `big`, hold armé |
| Identité de piste change | `big` | reste `big`, contenu remplacé, **hold relancé** |
| Identité de piste change | `mini` | → `big`, hold armé |
| Hold expiré | `big` | → `mini` |
| `playing` passe vrai → faux | tout | → `hidden` |
| Sondage identique | tout | aucun effet |

Le badge mini persiste indéfiniment tant que la lecture continue — comportement actuel inchangé.
Pause et arrêt masquent la carte, comme aujourd'hui.

### Identité de piste

`parseTrack` (`web/src/api.ts`) n'extrait aucun identifiant : deux morceaux de même titre ne
seraient pas distingués. On ajoute au `Track` la clé `id` (champ `id` ou `uri` Spotify quand
présent), avec repli sur `titre|artiste`. C'est cette clé, pas le titre, qui déclenche le reveal.

## Découpage

`NowPlaying.tsx` porte déjà ~110 lignes et deux mises en page en ligne ; y ajouter une troisième
plus une machine à états le surcharge. Découpage sur la couture déjà utilisée par le repo
(`usePlayback.ts` + fonction pure `advance` testée en environnement node — pas de RTL ici) :

- **`revealState.ts`** — `nextRevealState(prev, incoming)`, réducteur **pur** rendant l'état
  (`hidden|big|mini`) et s'il faut réarmer le hold. Toute la politique de déclenchement et de
  retargeting vit là. Ni React ni DOM : testable unitairement.
- **`useTrackReveal.ts`** — hook mince autour du réducteur : minuterie de hold, nettoyage au
  démontage.
- **`RevealCard.tsx`** — présentation pure. Reçoit `{ track, state }`, rend la boîte qui se morphe.
- **`NowPlaying.tsx`** — aiguillage entre les trois variantes, inchangé par ailleurs.

## Animation

Une seule boîte `.card` transitionnant `width` / `height` / `border-radius` sur 620 ms en
`cubic-bezier(.22,.9,.24,1)`. Deux couches de contenu en position absolue se fondent sur ~340 ms
avec un délai, de sorte que la couche entrante arrive quand la boîte a déjà fait l'essentiel de son
changement de taille.

- **Grand** : 300×408, padding 18, pochette 264×264 (rayon 12), kicker + égaliseur en haut à
  gauche, titre 20 px et artiste 14 px centrés sous la pochette. La hauteur intègre le bloc de
  titre fixe de 52 px (deux lignes, cf. « Cas limites ») : la maquette, antérieure à cette
  décision, montre 382 sur une seule ligne. Ces cotes sont celles de la **boîte extérieure**
  (`box-sizing: border-box` global dans `theme.css`, bordure 1 px) : l'intérieur disponible est
  donc 298×406 et le contenu consomme 1 px de marge sur chaque bord. Écart assumé par l'opérateur
  le 2026-08-29 — c'est le rendu validé sur la maquette, qui portait la même bordure ; rien ne
  déborde de la carte.
- **Mini** : 290×68, padding 10, pochette 48×48 (rayon 8), titre 14 px, artiste 12 px — identique
  à l'existant.
- **Ancrage** : `OverlayFrame anchor="top-right" pad={36}`, inchangé.

## Cas limites

- **Titres longs.** 264 px à 20 px tronquent vers 22 caractères — inacceptable pour une carte dont
  le seul rôle est d'annoncer le titre. La zone de titre de la grande carte est un bloc de hauteur
  **fixe 52 px** autorisant **deux lignes**, ellipse au-delà : la hauteur reste fixe, donc la boîte
  reste animable. Le mini reste sur une ligne.
- **`prefers-reduced-motion`** : bascule sèche entre états, pas de morph.
- **Pochette absente** : le placeholder `♫` existant, dimensionné selon l'état.
- **Orpheus absent ou non authentifié** : `{authenticated:false}` → `playing:false` → `hidden`.
  Inchangé.
- **Démontage pendant un reveal** : minuteries annulées.

## Hypothèse à vérifier (non bloquante)

Le passage à 2 s suppose qu'Orpheus ne relaie pas l'API Spotify à chaque appel sans cache. ~30
requêtes/min par source ouverte devrait rester confortable, mais **ce n'est pas vérifié** :
Orpheus n'est pas dans ce repo et le box ne tourne que le soir. À confirmer sur le box avant de
considérer le budget tenu ; si Orpheus n'a pas de cache, le repli est le push WS (voir Hors scope).

## Tests

Tests unitaires du réducteur (vitest, environnement node), sur le modèle de `usePlayback.test.ts` :

- premier sondage avec lecture en cours → `big`, hold armé ;
- reprise après pause → `big` ;
- changement de piste en état `big` → reste `big`, hold relancé ;
- changement de piste en état `mini` → `big` ;
- hold expiré → `mini` ;
- arrêt de lecture → `hidden` ;
- sondage identique répété → aucun changement d'état ni réarmement.

Vérification visuelle : maquette animée + contrôle en direct dans le navigateur contre le serveur
de dev.

## Hors scope

- **Push WS** (`music.changed` diffusé par le serveur, à la manière de `lobby.detected`) : correct
  architecturalement et sous la seconde, mais c'est du travail serveur pour une fioriture visuelle.
  À rouvrir seulement si le sondage à 2 s se révèle insuffisant ou si Orpheus n'encaisse pas.
- Album et barre de progression dans la grande carte.
- Son.
- Toute modification des variantes par défaut et `?mini`.
