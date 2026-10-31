# Lecture OCR du rang en fin de partie + carte overlay « dernières parties » — design

**Statut** : validée par l'opérateur (2026-10-31). Décisions 1 à 5 retenues telles qu'écrites (lecture des chiffres par gabarits, pas OCR générique). Implémentation non commencée : les captures brutes du PC de jeu sont à fournir.
**Date** : 2026-10-31
**Domaines** : Aegis (`aegis/crates/aegis-uploader`, capture + OCR), storm-codex (`crates/storm-codex-server/src/rank/`, `web/src/pages/`), spec `docs/spec/05-api.md` et `docs/spec/07-evenements.md` à compléter à l'implémentation.

## Contexte

Le MMR n'est pas dans le replay. Le classement (rang, points de rang gagnés ou perdus, total, points requis pour la promotion) n'apparaît qu'à l'écran. Les captures d'une VOD Twitch (2026-10-31) montrent le parcours de fin de partie :

1. **Coffre de loot** (pas toujours présent) : aucun point de classement.
2. **Synthèse, avant chargement** : compteur « Rank Points » à `0`, rang affiché (Diamond 1). Valeur à ne jamais lire.
3. **Survol du rang** : infobulle « Rank Points Change » (*Match Points*, *Total Rank Points*). Le compteur est encore à `0`.
4. **Animation** : le compteur monte (`227` observé) et l'anneau se remplit. Interruptible.
5. **Page Play, Ranked** : rang, **total de points** (535 observé, affiché seulement au survol du rang), **points requis pour la promotion** (465 observé). 535 + 465 = 1000.

En partie de placement, aucun rang n'est affiché : on ne lit rien et on ne publie rien.

## Objectif

1. **Aegis** lit le rang après chaque partie classée et envoie un résultat **validé** ou **indisponible** à storm-codex.
2. **storm-codex** garde le résultat avec la partie et l'affiche dans un overlay OBS : une carte compacte persistante, qui s'agrandit à la fin de chaque partie. Elle montre les dix dernières parties du compte, puis le KDA et les points gagnés ou perdus quand ils arrivent.

## Hors périmètre

- Pas de contrôle de la souris ni de clic dans le jeu. Le survol du rang pour afficher l'infobulle n'est **pas** automatisé : on ne lit que ce qui est déjà affiché.
- Pas d'extension Twitch ni d'Azure (abandonnés, cf. `docs/STATUS.md`).
- Pas de lecture des parties de placement (aucun rang affiché).
- Pas de modification de l'uploader de Hots-Overlay (`client-rs`) : il est remplacé par Aegis.

## Architecture

### Aegis : le lecteur

- Déclencheur : apparition de `replay.details` dans `TempWriteReplayP1` (déjà détectée par `aegis-uploader/src/live/`). À vérifier : ce fichier arrive **avant** ou **après** la synthèse à l'écran. Si c'est après, l'OCR attend la synthèse ; sinon, il attend la fenêtre de jeu.
- Capture : fenêtre de Heroes of the Storm uniquement, via `Windows.Graphics.Capture` (l'API de capture de Windows, pas le bureau entier). Sur écran HDR, capturer l'image scRGB et la reconvertir : la capture 8 bits sort sur-saturée (constat de Heroes Profile). Fréquence : une image par seconde suffit pour l'écran de fin, qui reste affiché plusieurs secondes.
- Lecture des chiffres : **gabarits de chiffres**, pas OCR générique. Les chiffres du total et du requis sont dans une police fixe ; on les compare à des gabarits construits à partir d'échantillons étiquetés, versionnés dans le binaire, avec un test qui les reconstruit. `Windows.Media.Ocr` reste possible pour du texte (titre VICTOIRE/DÉFAITE), pas pour les chiffres.
- Machine d'état (pure, testable sur des images enregistrées) :
  - `Attente` → `Synthèse` (titre « VICTOIRE » ou « DÉFAITE » lu) → `Stable` (même valeur de total lue sur **N = 3 images consécutives**, N à calibrer) → `Publié`.
  - Sortie possible à tout moment : `Indisponible` après un délai de 60 s sans lecture stable.
- Contrôle de cohérence avant publication : **total + requis pour la promotion = 1000** lus sur la page Play. Si la vérification échoue, on publie `indisponible` avec la raison, jamais une valeur estimée.
- Gardé en mémoire jusqu'à la fin de la partie : les lectures (et les images d'échantillon) ne sont **pas** envoyées tant que le mode de la partie n'est pas connu. Seul le mode Storm League (ou classé) déclenche l'envoi ; les autres modes sont jetés (idée reprise de Heroes Profile).
- Envoi : `POST /api/rank/result` avec l'en-tête `X-Session` = SHA-256 du `battlelobby` (même identifiant que le feed live), donc storm-codex rattache le résultat à la partie même si l'OCR finit avant le parse.
- Échantillons (opt-in, désactivé par défaut) : Aegis peut envoyer quelques captures étiquetées pour calibrer les gabarits. Jamais automatique.

### storm-codex : le serveur

- Nouvelle table `ranked_results` (migration suivante) : `session_id`, `match_id` (renseigné quand la partie est parsée), `rank_name`, `total_points`, `points_delta` (nullable), `promotion_required`, `status` (`valide` | `indisponible`, sans accent : valeur de la base), `reason`, `read_at`. Une seule lecture par session : la dernière remplace les précédentes.
- Routes :
  - `POST /api/rank/result` (Aegis, token du box) ;
  - `GET /api/rank/recent?player=<toon>&limit=10` : dix dernières parties du compte avec KDA et résultat de rang quand connu.
- Rattachement : si la partie n'est pas encore parsée, le résultat reste en attente sur `session_id`, puis est relié au `match_id` à l'arrivée du replay. Le rattachement se fait par **empreinte de partie** (`crate::upload::game_fingerprint`, déjà utilisée pour la déduplication) plutôt que par le seul `session_id`. En attente : expiration après 6 h, comme `PendingRanks` de Heroes Profile.
- Évènements (conformes à `docs/spec/07-evenements.md`, `schema_version`, `correlation_id`, `occurred_at` / `recorded_at`, forme `entité.verbe` au passé) :
  - `hots.match_finished` (dès le parse : KDA, résultat, mode) → la carte s'agrandit sans attendre l'OCR ;
  - `hots.rank_read` (validé ou indisponible) → l'anneau de la carte se remplit ou affiche « rang inconnu ».

### Overlay : la carte

- Même modèle que `/now-playing` (`?mini`, `?reveal`) : une source persistante, puis une annonce agrandie à chaque fin de partie. Fond transparent, ancrage haut-droite.
- Route proposée : `/ranked` (source OBS), paramètre `?me=<toon>` comme `/widget`.
- Contenu :
  - **Mini** : dix dernières parties (pastilles victoire/défaite) et rang courant ;
  - **Annonce** (fin de partie) : KDA et héros dès `hots.match_finished` ; points gagnés ou perdus et anneau de progression dès `hots.rank_read` ; sinon « points en attente » ou « rang inconnu », sans bloquer l'annonce.
- L'annonce ne dépend jamais de l'OCR : elle s'affiche avec les données du serveur, et l'OCR complète quand il arrive.

## Règles de validation

- Une valeur lue n'est publiée qu'après **stabilité** (N images identiques) **et** contrôle 1000.
- Un résultat indisponible est publié comme tel. Aucune valeur n'est déduite (par exemple du total précédent).
- Les nombres sont lus sur la page Play après l'animation, ou sur la synthèse une fois le compteur stable, jamais pendant l'animation.
- La reconnaissance doit distinguer VICTOIRE et DÉFAITE du titre de la synthèse pour contrôler le sens du gain.

## Tests

- **Machine d'état Aegis** : tests unitaires sur une séquence d'images décrites (pas d'image réelle) : stabilité, interruption, indisponible après 60 s, rejet si total + requis ≠ 1000, pas de publication en placement.
- **OCR** : jeu de captures brutes du PC de jeu (non committées si elles contiennent des pseudos), avec la lecture attendue annotée. Critère : 100 % de lectures correctes sur les captures validées, zéro publication fausse.
- **storm-codex** : rattachement d'un résultat arrivé avant le parse ; `GET /api/rank/recent` sans résultat rang ; évènements bien formés.
- **Overlay** : rendu sur `?mini` et annonce ; vérifié en vrai dans OBS sur une partie réelle.

## Référence : RankCapture de Heroes Profile

Le projet [HeroesProfile.Uploader](https://github.com/Heroes-Profile/HeroesProfile.Uploader) lit déjà les rangs
en jeu (C#, Windows/macOS/Linux). Ce qu'on en reprend, et ce qu'on n'en reprend pas :

- **Repris (idées)** : capture de la seule fenêtre de jeu, gestion HDR, mode de partie connu avant envoi,
  résultats gardés en attente le temps que le replay arrive, gabarits de glyphes plutôt qu'OCR, échantillons
  opt-in pour calibrer.
- **Différent** : eux lisent l'écran de **chargement** (ligue et division de chacun des dix joueurs, pas les
  points). Les points gagnés ou perdus n'apparaissent qu'à l'écran de fin, donc notre lecture reste à la fin
  de partie. Le chargement reste une option ultérieure (rang de départ des dix joueurs), pas une V1.
- **Licence** : le dépôt n'affiche pas de licence reconnue (`NOASSERTION`). On ne copie aucun code : on
  réimplémente les idées. Toute réutilisation de code demande d'abord de vérifier la licence.

## Données nécessaires (à fournir)

- Captures **brutes** (pas de VOD compressée) de la séquence complète, en partie classée, avec la fenêtre de jeu en plein écran et en fenêtré.
- Une partie de placement, pour vérifier le cas « aucun rang ».
- Une défaite et une victoire, pour vérifier le sens du gain.

## Décisions à valider

1. **Aegis** porte la capture et l'OCR (proposé), storm-codex ne fait que stocker et afficher.
2. **Lecture des chiffres** : gabarits construits à partir d'échantillons étiquetés, pas OCR générique (proposé, voir Architecture). `Windows.Media.Ocr` seulement pour le texte.
3. **Overlay** : nouvelle route `/ranked` sur le modèle de `/now-playing`, et non une extension de `/widget` (proposé). `/widget` reste la carte de la dernière partie.
4. **Stabilité** : N = 3 images consécutives identiques avant publication (proposé, à calibrer sur les captures).
5. **Parties sans lecture** : publiées comme `indisponible` après 60 s (proposé), jamais estimées.
