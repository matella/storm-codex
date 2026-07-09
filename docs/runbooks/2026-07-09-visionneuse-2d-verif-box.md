# Runbook — Vérification box de la Visionneuse 2D (calage minimap)

**But :** dernière marche du MVP-1 (jalon Lot 4) — vérifier sur le box, sur un **vrai match d'une
carte AVEC image de fond** (Cursed Hollow, Dragon Shire, Sky Temple… pas une ARAM sans image), que
les héros se placent au bon endroit sur la bonne carte, que le scrub est instantané, et **nudger le
flip-Y / recadrage si besoin**. Le pipeline, l'endpoint, le canvas et le seek 100 % client sont
**déjà prouvés en local** (smoke 2026-07-09, cf. STATUS) — il ne reste que le calage visuel
carte-relatif, qui exige une image de carte + un match reconnaissable.

## Pré-requis
- Branche `feat/replay2d` mergée (ou déployée telle quelle) sur le box.
- Le box tourne (le soir), Postgres backfillé, images vendorisées (`/images/battlegrounds/*.png`
  présentes — c'est ce qui manquait en local).

## Déploiement (rsync + docker compose build — PAS de git pull box)
```sh
# depuis le Mac, dans le repo (⚠️ TOUJOURS --exclude .env — cf. STATUS piège déploiement)
rsync -az --delete --exclude .env --exclude target --exclude node_modules --exclude .git \
  ./ matella@192.168.129.85:~/apps/storm-codex/
ssh matella@192.168.129.85 'cd ~/apps/storm-codex && docker compose up -d --build'
```

## Vérification (navigateur, via Tailscale)
1. Ouvrir `http://192.168.129.85:5102/matches`, choisir un match récent **sur une carte imagée**
   (Cursed Hollow / Dragon Shire / Sky Temple / Braxis Holdout…), pas une ARAM.
2. Onglet **« Replay 2D »**. Attendu :
   - Le **fond = l'image de la bonne carte** (plus le dégradé de secours).
   - 10 pastilles héros **portraits réels** (les images vendorisées existent sur le box), anneau
     couleur d'univers, remplissage couleur d'équipe.
   - Scrub 0 → fin : positions cohérentes, **instantané** (aucun aller-retour réseau par déplacement
     du curseur — déjà prouvé).
3. **Juger l'orientation** en scrubbant sur un moment connu (ex. un teamfight près d'un objectif, ou
   la mort d'un héros dont on sait où elle a eu lieu) :
   - Les héros sont-ils du bon côté (base bleue vs base rouge au bon bout de la carte) ?
   - Le **haut/bas** est-il correct ? Si tout est **inversé verticalement** → basculer le **flip-Y**.

## Si calage à corriger (code)
- **Inversion verticale** : dans `web/src/components/Replay2D.tsx`, la boucle de dessin utilise
  `cy = (1 - y) * H` (héros ET marqueurs de mort). Basculer en `cy = y * H` (aux DEUX endroits :
  pastilles + `deathsNear`). Un seul flip, cohérent.
- **Inversion horizontale** (rare) : idem sur `cx` (`cx = x*W` ↔ `(1-x)*W`).
- **Bordure injouable / héros écrasés au bord** : `MapSize` peut inclure une marge injouable. Ajouter
  une constante de recadrage par carte (fraction de marge) appliquée à la normalisation — soit côté
  crate (`storm-replay-viewer`, dans la normalisation), soit côté front au dessin. **Ne pas
  sur-ajuster** : viser « clairement lisible », pas le pixel. Documenter la valeur retenue.
- Après tout changement front : `cd web && npm run build` (vérifier `✓ built`), redéployer.

## Critère d'acceptation MVP-1 (à cocher)
- [ ] Bonne image de carte en fond.
- [ ] 10 héros bien placés (bon côté, bonne orientation) sur un match reconnu.
- [ ] Scrub instantané, atténuation vivant/mort + marqueurs de mort au bon endroit.
- [ ] Portraits réels affichés.
→ Une fois coché : mettre à jour `docs/STATUS.md` (« Visionneuse 2D MVP-1 : livré + vérifié box »),
puis finaliser la branche (merge / PR).

## Rappels
- HP/mana **hors périmètre permanent** (donnée absente du replay) → vivant/mort à la place.
- Fast-follows (hors MVP-1) : animation play/pause, structures vivant/détruit, kill-feed cliquable,
  indicateurs talents/niveau, objectifs par carte.
