# Inspection du format `replay.server.battlelobby` (companion live, tâche 1)

**Date :** 2026-08-27 · **Statut : investigation, pas de décision d'architecture** — ce document
répond aux 6 questions de la spec companion-live avec preuve à l'appui ; il conditionne l'écriture
du parser en tâche 3.

**Outil :** `crates/storm-replay/examples/dump_lobby.rs` — extrait le stream
`replay.server.battlelobby` brut (non décodé) d'un `.StormReplay` vers un fichier `.bin`, via
`Replay::open()` + `battlelobby_raw()`. C'est le même blob que celui écrit en live par le jeu dans
`%TEMP%\Heroes of the Storm\TempWriteReplayP1\replay.server.battlelobby`.

```
cargo run -p storm-replay --example dump_lobby -- <replay> <sortie.bin>
```

## Corpus dumpé

| Fichier source | Taille du blob | Joueurs (via `storm-stats-dump`) |
|---|---|---|
| `crates/storm-stats/tests/data/silver-city-aram.StormReplay` | 279 694 octets | 10 (ARAM, Silver City) |
| `crates/storm-replay/tests/data/2024-10-07 22.29.55 Industrial District.StormReplay` | 280 388 octets | 10 |
| `crates/storm-replay/tests/data/2026-05-27 22.37.45 Industrial District.StormReplay` | 281 735 octets | 10 |
| `crates/storm-replay/tests/data/2026-06-09 20.35.02 Industrial District.StormReplay` | 281 642 octets | 10 |

4 replays uniques → 5 fichiers `.bin` produits par l'étape 3 (`silver-city.bin` et
`silver-city-aram.bin` sont deux dumps du même replay, tailles identiques 279 694 octets — attendu).

Pour disposer d'une vérité de référence (noms de joueurs, héros *tels que décodés par le pipeline
existant*, équipes), chaque replay a aussi été passé dans le binaire déjà existant
`storm-stats-dump` (`crates/storm-stats/src/bin/storm-stats-dump.rs`) — **cette référence vient de
`replay.details()`, pas du lobby** ; elle sert uniquement de point de comparaison pour chercher les
mêmes chaînes dans le blob lobby.

Extrait pour `silver-city-aram` :

```
2-Hero-1-10724235 | Sando         | Tassadar        | team 0
2-Hero-1-1770353  | Kain          | Qhira           | team 0
2-Hero-1-14148348 | ShieldedSlug  | The Lost Vikings| team 0
2-Hero-1-14148822 | TheBlackKing  | Li Li           | team 0
2-Hero-1-7293976  | Cascon        | Chromie         | team 0
2-Hero-1-6130044  | slashbored    | Varian          | team 1
2-Hero-1-13579289 | SVKpermaBan   | Mephisto        | team 1
2-Hero-1-13495235 | LenaOxton     | Jaina           | team 1
2-Hero-1-1863125  | Hyldii        | Zarya           | team 1
2-Hero-1-2138731  | Shrek         | Rehgar          | team 1
```

(clé = `toon_handle`, format confirmé par le code existant : `region-Hero-realm-id`.)

---

## Réponses aux 6 questions

### 1. Les BattleTags sont-ils présents, et dans l'ordre du lobby ?

**Oui**, présents en clair, et dans un ordre qui correspond au regroupement par équipe (5+5) tel
que retourné par `storm-stats` (lui-même dérivé de `replay.details()`, pas du lobby).

Preuve (`strings -n 3 silver-city.bin`, lignes 2343–2368) :

```
Sando#2475
Kain#2164
ShieldedSlug#2429
TheBlackKing#21829
Cascon#11441
slashbored#2867
SVKpermaBan#2161
LenaOxton#21672
Hyldii#21343
Shrek#2533
```

Ordre identique à la table ci-dessus (équipe 0 : Sando, Kain, ShieldedSlug, TheBlackKing, Cascon ;
équipe 1 : slashbored, SVKpermaBan, LenaOxton, Hyldii, Shrek). Confirmé sur un 2ᵉ replay
(`2026-06-09 … Industrial District`, 9 des 10 tags visibles via `strings`, cf. Q3/limite ci-dessous)
avec le même schéma d'ordre.

**Nuance importante pour le parser (tâche 3) :** un BattleTag non-ASCII (joueur russe
`ЛовкийЭльф#215346` dans le replay Industrial District du 2026-06-09) **n'apparaît pas** dans la
sortie de `strings` (qui ne détecte que des runs ASCII imprimables) — il est bien présent dans le
blob mais en UTF-8 multi-octets. Vérifié en cherchant les octets bruts :

```
offset 0x43e3e : 07 ce 86 4b 21 1b d0 9b d0 be d0 b2 d0 ba d0 b8 d0 b9 d0 ad d0 bb d1 8c d1 84
                 23 32 31 35 33 34 36 00
                 [len=0x1b=27] Л  о  в  к  и  й  Э  л  ь  ф  #  2  1  5  3  4  6  \0
```

Le préfixe `0x1b` = 27 correspond exactement à la longueur **en octets UTF-8** de
`"ЛовкийЭльф#215346"` (10 caractères cyrilliques × 2 octets = 20, + `"#215346"` = 7 octets → 27).
**Le parser de tâche 3 doit donc lire un compte d'octets puis décoder en UTF-8, pas supposer de
l'ASCII pur ni un compte de caractères.**

### 2. Les composantes du toon handle (`m_region`, `m_programId`, `m_realm`, `m_id`) sont-elles présentes ?

**Inconnu / probablement non sous forme directement exploitable.** Chaque BattleTag est précédé
d'un champ texte de la forme `T:<nombre>#<nombre>`, par exemple juste avant `Sando#2475` :

```
00043394: 03 00 00 00 19 01 54 3a 31 30 35 36 31 37 35 37   ......T:1056175
000433a4: 36 23 32 32 37 00 01 01 ...                        6#227...
```
soit la chaîne `"T:105617576#227\0"` immédiatement suivie, plus loin, de `"Sando#2475"` (avec son
propre octet de longueur `0x0a`=10 bien vérifié, cf. plus bas). **Contrairement au champ
BattleTag**, le mécanisme de longueur-préfixe de ce champ `T:…#…` n'a pas été identifié avec
certitude (l'octet immédiatement précédent, `0x01`, ne correspond pas à la longueur de la chaîne
`15` ; il s'agit possiblement d'un tag de type ou d'un varint sur plusieurs octets) — noté ici
comme observation brute, pas comme mécanisme confirmé.

Ce nombre (`105617576`) **ne correspond à aucun des `m_id` réels** des 10 joueurs
(`10724235, 1770353, 14148348, 14148822, 7293976, 6130044, 13579289, 13495235, 1863125,
2138731`) — vérifié par recherche exhaustive des 10 `m_id` en ASCII et en entier 32 bits
little/big-endian dans le blob : **aucune occurrence, sous aucune des trois formes**. Le champ
`T:…#…` a donc une sémantique **inconnue** (piste : un identifiant de session/lobby côté
Battle.net, pas le `toon_handle` du joueur) et ne doit pas être confondu avec `m_id`.

Aucune chaîne `m_region`, `m_programId`, `m_realm`, `toonHandle`, ni un pattern texte du type
`"2-Hero-1-…"` (format confirmé du toon handle applicatif, cf. `ReplayHeader.toon_handle`) n'a été
trouvée dans le blob. **Conclusion : les composantes du toon handle ne sont pas présentes sous
forme lisible évidente ; il faudra reconstituer la liaison par ensemble de BattleTags (nom#tag)
plutôt que par toon handle** — cohérent avec la résolution par défaut prévue en cas de doute.

### 3. L'appartenance à une équipe est-elle déductible (champ explicite, ou position/ordre) ?

**Par ordre uniquement, à confirmer — aucun champ « équipe » explicite identifié à proximité des
enregistrements joueurs.** L'ordre d'apparition des 10 BattleTags correspond, sur les 2 replays
inspectés en détail, exactement au découpage 5 premiers = équipe 0 / 5 derniers = équipe 1 tel que
retourné par `storm-stats` (qui lui dérive l'équipe de `replay.details()`, pas du lobby).

En xxd, la structure autour de chaque enregistrement (motif répété `42 21 95 c9 2f` avant/après
chaque BattleTag, taille de bloc variable ~90-110 octets à cause de champs à longueur variable) ne
laisse voir aucun octet différenciant nettement le 5ᵉ du 6ᵉ enregistrement (frontière d'équipe) du
1ᵉʳ au 2ᵉ (même équipe) — le motif de séparation est identique. Il est donc possible que le champ
équipe existe ailleurs dans le blob (zone bit-packée non explorée byte par byte) sans qu'on l'ait
localisé. **Réponse : ordre observé cohérent avec un regroupement par équipe sur 2 échantillons,
mais aucun champ explicite trouvé → à traiter avec prudence en tâche 3 (ne pas parier uniquement
sur la position sans un signal de secours).**

### 4. Le héros pické est-il présent ? — question la plus importante

**Non trouvé sous forme de chaîne lisible, ni canonique ni interne, associée aux BattleTags.**
Recherche active mais négative :

- Aucun des 10 noms de héros réellement joués en `silver-city-aram` (`Tassadar`, `Qhira`,
  `Lost Vikings`, `Li Li`, `Chromie`, `Varian`, `Mephisto`, `Jaina`, `Zarya`, `Rehgar`) n'apparaît
  dans `strings -n 3 silver-city.bin` (recherche insensible à la casse, formes canoniques testées).
- Aucune forme interne `HeroXxx` correspondant à ces héros n'apparaît non plus.
- Des fragments à consonance de héros existent bien dans le blob, mais **ne correspondent à
  aucun des héros réellement joués** dans cette partie — ce qui suggère un catalogue générique
  (portraits/récompenses), pas le pick :
  - `DVA4`, `DVAJ`, `DVA9` (offsets ~659–666 dans `strings`) : aucun joueur de cette partie ne
    joue D.Va.
  - Une liste distincte, en fin de fichier (offset 0x43ddd–0x4444f), de tokens 4 octets de type
    `Arts`, `Crus`, `FENX`, `Guld`, `HL--`, `Junk`, `Luci`, `Monk`, `Ragn`, `Sylv`, `Uthe`, `Zaga`
    — dans une zone à enregistrements de taille fixe (~64 octets chacun, cf. xxd). Aucun ne
    correspond à un abrégé plausible des 10 héros joués (`TASS`, `QHIR`, `LILI`, `CHRO`, `VARI`,
    `MEPH`, `JAIN`, `ZARY`, `REHG` : zéro occurrence). Sémantique de cette zone : **inconnue**
    (hypothèse non vérifiée : catalogue de portraits/loot/annonceur générique au client, pas
    lié aux picks de la partie).
- Une zone contenant de nombreux jetons `Hero…` (`HeroPOVT8`, `HeroGOLD`, `HeroLOOT`, `HeroICON`,
  `HeroLcns`, `HeroPORT`) a été trouvée (offset ~ligne 2385 dans `strings`) mais ce sont des
  suffixes de type d'actif (`ICON`, `PORT`ait, `LOOT`, `Lcns`=license) accolés au mot générique
  `Hero`, **pas des noms de héros** — probablement des clés de la structure de récompenses/lobby
  UI, sans rapport avec le pick.

**Conclusion, honnête : le héros pické n'est pas trouvable en clair dans ce blob par une recherche
de chaînes ou de motifs connus. Réponse à la sous-question ARAM (correspond-il au héros réellement
joué, vu le bug de shuffle sur l'attribut 4002 documenté dans `docs/STATUS.md`) : sans objet,
puisqu'aucun candidat « héros » n'a été identifié dans le blob pour commencer — inconnu.**
Il est possible que le pick soit encodé uniquement sous forme d'ID numérique bit-packé (non
byte-aligné) nécessitant un décodage structurel complet du blob (hors périmètre de cette tâche
d'investigation) — **si c'est le cas, l'ergonomie « héros visible en direct dans le lobby » n'est
pas acquise avec une simple extraction de chaînes, et la tâche 3 devra soit décoder le format bit
par bit, soit renoncer à afficher le héros avant la fin de partie.**

### 5. La carte et le mode sont-ils présents ?

**Carte : indirectement, via des chemins de cache `.s2ma`, mais pas de nom en clair.**
Le tout début du blob contient 9 chemins de cache Battle.net pointant vers des fichiers `.s2ma`
(cartes) identifiés par hash de contenu, par exemple :

```
C:\ProgramData\Blizzard Entertainment\Battle.net\Cache\1f\1b\1f1b228ddb1f72205cbfd44405528710
0b0f39959be816548162e4081ea85511.s2ma
```

Aucun de ces hashes ne se résout en nom de carte sans table de correspondance externe (absente de
ce blob et du crate) — **inconnu** si l'un de ces 9 fichiers correspond spécifiquement à Silver
City sans decoder le hash. Recherche complémentaire négative : ni `"Silver City"`, ni `"ARAM"`, ni
aucun nom de carte/mode canonique n'apparaît en ASCII (`strings`) ni en UTF-16LE (recherche
programmatique dédiée) dans le blob.

**Mode : non trouvé.** Recherche des codes de mode connus et documentés dans `docs/STATUS.md`
(`50091`=Storm League, `50101`=ARAM, `-1`=Custom) en ASCII et en entier 32 bits little/big-endian :
aucune occurrence dans le blob. **Réponse : carte et mode ne sont pas présents sous forme
directement lisible ; la carte est peut-être déductible via le hash `.s2ma` (nécessite une table
hash→carte externe, à vérifier — inconnu à ce stade) ; le mode n'a aucune piste identifiée.**

### 6. Le blob live est-il bit-à-bit identique au blob archivé ?

**Non testé — exige le PC de jeu.** Cette comparaison nécessite de copier
`%TEMP%\Heroes of the Storm\TempWriteReplayP1\replay.server.battlelobby` pendant une partie en
cours sur la machine de jeu Windows, indisponible pour cette tâche. À reporter en tâche 5 (ou dès
qu'un accès au PC de jeu est possible). Par défaut, en l'absence de cette preuve, **la liaison
replay↔lobby devra se faire par ensemble de BattleTags**, comme prévu par la résolution par défaut
de la spec — pas par hash d'octets.

---

## Difficultés rencontrées

- La structure du blob n'est **pas un format à enregistrements de taille fixe** dans la zone des
  BattleTags : la distance entre deux BattleTags consécutifs varie (92, 101… octets), à cause de
  champs numériques précédents encodés en longueur variable (vraisemblablement des varints du
  protocole versionné de Blizzard). Une extraction fiable en tâche 3 devra donc marcher par
  recherche de motif (longueur-préfixe + `#` + chiffres) plutôt que par offsets fixes.
- `strings` (BSD, macOS) n'a pas d'option `-e`/`-el` pour forcer une lecture UTF-16 ou UTF-8
  multi-octets — la détection du BattleTag cyrillique a nécessité une recherche programmatique
  directe des octets UTF-8 dans le fichier (Python), pas l'outil `strings` seul. À documenter pour
  la tâche 3 : ne pas se fier uniquement à `strings`/grep ASCII pour l'inventaire des BattleTags.
- Aucun exemple `storm-stats` dédié à l'affichage des héros n'existait ; le binaire déjà présent
  `crates/storm-stats/src/bin/storm-stats-dump.rs` (`storm-stats-dump <replay> <sortie.json>`) a
  été réutilisé tel quel (aucune modification) pour obtenir la vérité de référence
  joueurs/héros/équipes utilisée dans ce document — cette référence vient de `replay.details()`
  (l'attribut 4002, potentiellement biaisé par le shuffle ARAM selon `docs/STATUS.md`), pas du
  lobby ; elle ne sert qu'à savoir *quelles chaînes chercher* dans le blob, pas de vérité absolue
  sur le héros réellement joué.

## Ce que la tâche 3 peut retenir de sûr

1. Les BattleTags (`nom#tag`) sont extractibles du blob par un motif
   `[octet de longueur en octets][bytes UTF-8][séparateur '#'][chiffres][\0]`, dans l'ordre du
   lobby (empiriquement = ordre par équipe sur 2 échantillons).
2. Il faut décoder en UTF-8 sur le nombre d'octets donné, pas supposer de l'ASCII.
3. Le toon handle complet n'est pas présent en clair → liaison par BattleTag, pas par
   toon handle, et pas par team explicite (pas de champ confirmé) tant qu'un champ équipe n'a pas
   été localisé plus précisément.
4. Le héros pické n'a pas été retrouvé dans ce blob par cette méthode — **point bloquant potentiel
   pour l'ergonomie visée**, à traiter explicitement en tâche 3 (soit décodage bit-packé complet,
   soit produit livré sans héros visible avant fin de partie, à trancher par l'opérateur).
5. Carte et mode ne sont pas exploitables sans décodage plus profond ou table externe.
