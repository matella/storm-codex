# Design « dossier » pour toute l'app

> Spec storm-codex. Demande opérateur du 2026-09-27 : « le style du scouting est vraiment bien,
> peut-on faire quelque chose de similaire pour toute l'app ? ». Overlays OBS : « maquette pour
> voir, mais très probablement hors périmètre ».
> **En attente de validation** — rien n'est implémenté.
> Maquettes : `2026-09-27-design-dossier-app-mockup.html` (Session, Match detail, Hero page) et
> `2026-09-27-design-dossier-overlays-mockup.html` (exploration OBS).

## Contexte et décision à prendre

`01-architecture.md` verrouille « design Nexus Codex ». Cette spec **ne le remplace pas** : elle
propose une **évolution** — « Nexus Codex · Dossier » — qui garde la palette et les tokens de la
maquette du 2026-06-12 (fond `#06070b`, accent `#7f77dd`, victoire `#5dcaa5`, défaite `#e24b4a`,
couleurs d'équipe et d'univers) et y ajoute la couche éditoriale née avec les rapports de scouting
(export HTML, pages `/scouting`). Rouvrir une décision verrouillée revient à l'opérateur : valider
cette spec vaut décision.

## Le langage visuel

| Élément | Règle |
|---|---|
| Typographie | **Big Shoulders Display** (titrage condensé) pour titres de page, noms de héros/cartes et grands chiffres ; **Figtree** pour le texte ; **JetBrains Mono** pour libellés, métadonnées et nombres de tableaux |
| Couverture | chaque page de détail ouvre sur un panneau « couverture » : kicker (contexte), titre en très grand, tuiles de stats, art de la carte en fond (minimap floutée ou découpée en biais) |
| Sections | titres numérotés (`01 DRAFT`), filet, sous-titre à droite en mono |
| Portraits | cercle à anneau de la couleur d'univers ; tons **pick** (halo vert), **ennemi** (halo rouge), **ban** (gris barré de rouge), **défaite** (anneau rouge) |
| Résultats | pastilles V/D, verdict « VICTORY / DEFEAT » en titrage, bilans `k–n` en grands chiffres |
| Barres | jauges fines (accent → accent clair ; rouges pour l'équipe adverse) |
| Mouvement | apparition décalée des sections, respect de `prefers-reduced-motion` |

**Règle de densité** : le titrage condensé ne va **jamais** dans les lignes de tableau. Les pages
denses (Matches, Heroes, Trends) gardent des tableaux lisibles (chiffres en mono, lignes nettes) ;
le nouveau langage les **encadre** (couverture, en-têtes, filtres), il ne les remplace pas.

## Les trois maquettes (données réelles de l'archive)

- **Session** — couverture « Tonight » : bilan de la soirée en très grand (4–0 le 2026-06-09),
  pastilles, dernière partie (portrait halo, MVP, lien vers le match), tuiles (winrate, K/D/A, MVP,
  héros). « Tonight's games » : une carte par partie (bandeau de carte, mode, héros, K/D/A, award ;
  motif rayé pour les cartes ARAM sans minimap). « Your form » : 30 derniers résultats, winrate
  global, 10 derniers, winrate par mode, héros les plus joués.
- **Match detail** (2368, Braxis Holdout, Storm League) — couverture avec minimap découpée, verdict,
  score takedowns / niveau / premier objectif / premier fort. Draft : deux panneaux (bans barrés
  avec phase, picks dans l'ordre, « FIRST PICK », « YOU »). Scoreboard : barres relatives au max de
  la partie, awards en étiquettes, ligne du joueur mise en évidence. Avance de niveau : courbe
  au-dessus/en dessous de la ligne + premier fort. Build du joueur par palier.
- **Hero page** (Gul'dan) — grand portrait à anneau d'univers, stats, meilleure carte (≥ 10
  parties). Cartes en tuiles minimap colorées (≥ 60 % vert, < 45 % rouge). « Builds that win » :
  chemins de talents par palier avec winrate. Historique de patchs avec étiquettes BUFF / NERF /
  BUGFIX.

Les maquettes du dépôt chargent les portraits depuis `../../.images/` (référentiel vendorisé
localement, non versionné) et les minimaps depuis `../../assets/minimaps/` ; sans portraits, repli
sur médaillons à initiales.

## Architecture technique

1. **Polices auto-hébergées** : les trois familles (licence SIL OFL) servies depuis le bundle
   (`web/public/fonts/*.woff2`, sous-ensemble latin), plus de Google Fonts — l'app tourne en LAN
   et dans OBS. Budget : ≤ 250 Ko de polices.
2. **Tokens** : `theme.css` gagne les tokens typographiques et de tons ; les tokens de couleur
   existants ne changent pas.
3. **Composants partagés** `web/src/components/ds/` : `Portrait` (tons), `Cover`, `SectionHead`,
   `StatTile`, `Pips`, `Chips`, `MapHeader`, `Bar`, `Verdict`, `EvidenceLine`. Les pages Scouting
   migrent de leurs copies locales (`Dossier.tsx`, `scouting-dossier.css` scopé `.sd`) vers ces
   composants — une seule source.
4. **Chrome** : barre du haut redessinée (marque en titrage, onglets en mono capitales,
   indicateur live) — voir maquettes.
5. L'export HTML de scouting reste autonome (il n'utilise pas le bundle) et n'est pas modifié.

## Déploiement par lots (chacun livrable et vérifiable seul)

| Lot | Contenu |
|---|---|
| 0 — fondations | polices auto-hébergées, tokens, composants `ds/`, chrome, migration des pages Scouting |
| 1 — cœur | Session (Dashboard), Matches (cadre + filtres, tableau dense), Match detail |
| 2 — référentiel | Heroes / Hero, Player, Maps |
| 3 — analyses | Synergies, Trends, Patch notes / Hero changes, Leagues |
| 4 — outils | console Draft, Admin (allégé) |
| Overlays OBS | **hors périmètre** par défaut (maquette d'exploration seulement). S'ils étaient retenus : habillage uniquement — tailles, ancrages et timings figés par leurs specs inchangés |

## Critères d'acceptation (par lot)

- `npm test` et `npm run build` verts ; aucune régression des mises à jour temps réel (WS).
- Vérification navigateur sur données réelles de chaque page du lot, en 1400 px et 390 px.
- Lot 0 : polices servies par l'app (aucune requête vers Google Fonts), ≤ 250 Ko ; pages Scouting
  identiques visuellement après migration ; sources OBS inchangées.
- Pages denses : lisibilité des tableaux conservée (chiffres en mono, pas de titrage dans les lignes).

## Impact documentaire

`08-frontend.md` (conventions de design, composants `ds/`, chaque page refaite) ;
`01-architecture.md` (la ligne « design Nexus Codex » précise « · Dossier » une fois validé) ;
`docs/STATUS.md`.

## Questions ouvertes pour l'opérateur

1. La direction des trois maquettes est-elle la bonne (et la barre du haut redessinée) ?
2. Overlays OBS : confirmer « hors périmètre » après avoir vu la maquette d'exploration ?
3. Ordre des lots : Session / Matches / Match detail d'abord, ou une autre priorité ?
