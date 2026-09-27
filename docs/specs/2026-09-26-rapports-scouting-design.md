# Rapports de scouting

> Spec storm-codex. Besoin exprimé par l'opérateur le 2026-09-26.
> **Validée par l'opérateur le 2026-09-26** (« You can go »). Volet A passé (drafts confirmés).
> Plan : `docs/plans/2026-09-26-rapports-scouting.md`.

## But

Préparer un match contre une équipe adverse à partir de **ses replays** : l'opérateur dépose une
liste de replays, storm-codex en tire un jeu de faits chiffrés (draft, cartes, pool de héros,
déroulé des parties), les emballe dans un **pack** destiné à un LLM (ChatGPT, Claude ou un modèle
local), puis **réimporte l'analyse** du LLM dans un format imposé pour l'afficher proprement, chaque
affirmation adossée aux chiffres qui la justifient.

Chaque rapport est un objet durable : titre libre modifiable, horodatage, liste de rapports.

## Cadre fixé par l'opérateur (2026-09-26)

| Point | Décision |
|---|---|
| Usage | **scouting à 99 %** (analyse d'un adversaire, pas revue de sa propre équipe) |
| Type de parties | **toutes en partie personnalisée** (mode `Custom`, draft de tournoi) |
| Origine des replays | les parties de l'adversaire **contre d'autres équipes, et parfois contre nous** |
| Utilisateurs | **l'opérateur seul** — pas de partage, pas de lien public |
| Analyses | **une seule par rapport** ; réimporter **écrase** l'analyse en place |
| Contenu de l'analyse (2026-09-26, après la v1) | **la draft uniquement, carte par carte** : quelles cartes choisir ou éviter, et pour chaque carte quoi bannir, quoi prendre, ce qu'ils vont probablement jouer, quoi prévoir ; plus un plan valable sur toute carte. Plus de notes joueurs ni de plan de jeu général → `format_version: 2` |
| Règle de roster | le côté cible compte **toujours au moins 3 joueurs de l'équipe principale**, sauf exception ; pour les exceptions, l'opérateur peut désigner un **joueur ancre** |
| Étape LLM | **manuelle** (copier-coller ou fichier) : l'opérateur n'a pas d'API OpenAI/Anthropic — aucun appel de LLM depuis l'app |
| Noms des joueurs | **inclus** dans le pack, sans option pour les retirer (ce ne sont que des pseudos) |
| Langue de l'analyse | **anglais** (comme l'UI) |
| Lien avec `teams` | **non** — le roster d'un rapport vit dans le rapport |

## L'existant (état au 2026-09-26)

| Élément | Où | Ce qui sert ici |
|---|---|---|
| Draft des parties perso | `crates/storm-stats/src/process.rs:2239` | `match.bans`, `match.picks` (+ `first`), `players.*.turn` — le mode `Custom` est dans la liste des modes draftés (`process.rs:2246-2255`) |
| Upload + parse | `crates/storm-codex-server/src/upload.rs` | pipeline archive → parse → projection, `game_fingerprint` (`upload.rs:22`) |
| Auth d'écriture | `crates/storm-codex-server/src/auth.rs` | `is_admin` : Bearer `ADMIN_TOKEN`, mode ouvert s'il est absent |
| Simulateur de draft | `/draft`, `draft_live` | passerelle éventuelle (volet F) |
| Export CSV | `GET /api/matches.csv` | précédent d'export, **non réutilisé** (voir décision 1) |

## Décisions actées

1. **Les replays de scouting ne vont pas dans `matches`.** Ce sont majoritairement des parties
   d'autres joueurs : projetées dans `matches`/`match_players`, elles pollueraient toutes les stats
   personnelles (`/api/heroes`, `/api/players`, `/api/maps`, `/api/synergies`, dashboard, lobby…).
   Un drapeau `source` sur `matches` obligerait à filtrer la quarantaine de requêtes qui lisent
   ces tables (`read.rs` : 24, `lobby/enrich.rs` : 6, `project.rs` : 6…) — une seule oubliée et la
   pollution est silencieuse. On stocke donc dans des **tables dédiées** : zéro changement des
   requêtes existantes, isolation garantie par construction.
   Une partie adverse **contre nous** peut ainsi exister aux deux endroits (dans `matches` via
   `client-rs`, et dans le rapport) : c'est voulu, les deux usages sont indépendants.
2. **Aucun changement de storm-stats ni de `PARSER_VERSION`.** Tous les faits sont calculés à partir
   de l'objet `match`/`players` que storm-stats produit déjà. Pas de diff de parité à refaire.
3. **Les faits sont calculés en Rust, pas par le LLM.** Un LLM calcule mal des taux sur des
   centaines de lignes et invente volontiers des chiffres ; il reçoit donc des agrégats déjà
   calculés, chacun avec son **effectif `n`**, et il **interprète**. Le calcul est un module
   **pur** (zéro I/O) `crates/storm-codex-server/src/scouting/facts.rs`, testé unitairement, comme
   le moteur de draft.
4. **Chaque fait a un identifiant stable** (`map.cursed_hollow.record`, `p2.hero.johanna`…) et
   **chaque affirmation de l'analyse doit citer les identifiants** qui la fondent. À l'import, les
   identifiants sont résolus : l'UI affiche la vraie valeur et son `n` à côté de l'affirmation, et
   **signale** toute affirmation sans preuve ou citant un identifiant inexistant. C'est le garde-fou
   contre l'hallucination sur des échantillons de 10–20 parties.
5. **Les faits sont un instantané stocké**, recalculé à chaque changement du rapport (ajout/retrait
   de replay, roster modifié) et versionné (`facts_version`). Les lectures ne re-décodent rien et
   ne rechargent pas 30 objets `match` : elles servent l'instantané.
6. **Les joueurs sont désignés `p1`…`pN` dans le pack**, pas par leur BattleTag : identifiants
   courts que le LLM recopie sans erreur, et que l'UI retraduit. Les noms figurent toujours dans
   une table de correspondance du pack (décision opérateur : ce ne sont que des pseudos).
7. **Routes d'écriture protégées** comme la gestion existante (`is_admin`, 🔒 si `ADMIN_TOKEN`).

## Volet A — Vérification préalable : le draft des parties perso (porte d'entrée)

Le code déclare `Custom` comme mode drafté, mais rien ne prouve que les bans et l'ordre de pick
sortent justes sur **de vraies parties de tournoi**. Première tâche, avant tout le reste : décoder
3 replays de parties perso avec draft et comparer `bans`, `picks.first` et l'ordre des picks à ce
qu'on voit en rejouant la partie dans le client HotS (confirmation opérateur).

Si c'est faux ou incomplet : on documente l'écart ici, et le volet « Draft » des faits est retiré
de la v1 (cartes, pool de héros et déroulé restent livrables) — une correction de storm-stats
serait une divergence de parité à instruire dans `03-storm-stats.md`, spec séparée.

### Résultat du décodage (2026-09-26) — **confirmé par l'opérateur** (volet A passé)

3 replays fournis par l'opérateur (une série du 2026-07-22, build 97605, mêmes 10 joueurs ; ancre
`Razhag`) : Tomb of the Spider Queen, Alterac Pass, Braxis Holdout. Décodage sans erreur ;
`mode = -1` = `Custom` dans `constants.json` (valeur attendue, et la draft est bien extraite).
Cohérence interne vérifiée : 3 bans par équipe (2 + 1), 5 picks par équipe, aucun héros banni
n'est pické dans la même partie, aucun doublon.

Ce qui est **lu** dans le replay et ce qui est **déduit** — à garder en tête pour les faits :

| Donnée | Origine |
|---|---|
| Héros bannis par chaque équipe, et phase (2 premiers / 1 du milieu) | lu (attributs 4023/4025/4043 et 4028/4030/4045) |
| Codes de ban → nom de héros | `data/attr.json` (`Crus` → Johanna…) — **le pack doit résoudre les codes** |
| Équipe au first pick, ordre des picks au sein d'une équipe | lu |
| Entrelacement global (ban A, ban B, ban A, ban B, pick A, B B, A A, ban B, ban A, B B, A A, B) | **déduit** : ordre standard codé en dur dans le parser (`process.rs:2343-2378`), pas lu |

Conséquence : les faits de draft parlent de « bans de 1re phase / de milieu de draft » et d'« ordre
de pick dans l'équipe », jamais d'un rang global qui ne serait qu'une hypothèse.

## Volet B — Stockage, dépôt des replays, détection de l'équipe cible

### Modèle de données — migration `NNNN_scouting.sql`

Numéro = prochain libre au moment de l'implémentation (`0010` est réservé par la spec « chat de
match » si elle passe avant).

```sql
CREATE TABLE scouting_reports (
    id                   BIGSERIAL PRIMARY KEY,
    title                TEXT        NOT NULL,
    target_name          TEXT,                       -- nom libre de l'équipe scoutée
    roster               JSONB       NOT NULL DEFAULT '[]',  -- toon_handles confirmés par l'opérateur
    anchors              JSONB       NOT NULL DEFAULT '[]',  -- toon_handles ancres (exceptions)
    facts                JSONB,                      -- instantané des faits (volet C)
    facts_version        INT         NOT NULL DEFAULT 0,
    analysis             JSONB,                      -- analyse importée validée (volet E), écrasable
    analysis_facts_version INT,                      -- facts_version au moment de l'import
    analysis_model       TEXT,                       -- déclaré par le LLM ou saisi, informatif
    analysis_imported_at TIMESTAMPTZ,
    created_at           TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at           TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE scouting_games (
    id             BIGSERIAL PRIMARY KEY,
    report_id      BIGINT      NOT NULL REFERENCES scouting_reports(id) ON DELETE CASCADE,
    fingerprint    TEXT        NOT NULL,             -- game_fingerprint (upload.rs:22)
    file_sha256    TEXT        NOT NULL,
    archived_path  TEXT        NOT NULL,
    filename       TEXT,
    played_at      TIMESTAMPTZ,
    map            TEXT,
    build          INT,
    target_team    INT,                              -- 0/1 ; NULL = équipe cible non déterminée
    target_source  TEXT,                             -- 'roster' | 'anchor' | 'manual' ; NULL si non déterminée
    data           JSONB       NOT NULL,             -- sortie storm-stats complète {match, players}
    parser_version INT         NOT NULL,
    UNIQUE (report_id, fingerprint)
);
CREATE INDEX scouting_games_report_idx ON scouting_games (report_id);
```

- **Écart d'implémentation assumé (2026-09-26)** : `scouting_games` porte aussi `summary JSONB`
  (résumé compact : équipes, héros, bans, picks, stats, niveaux, objectifs), et `scouting_reports`
  des ids `GENERATED ALWAYS AS IDENTITY` (convention des migrations récentes). Le recalcul des
  faits ne lit que les résumés, pas N objets `match` complets ; `data` reste la projection
  complète. `facts` stocke l'instantané `{facts, detection, roster, roster_auto}`.
- **Replay brut archivé** sous `ARCHIVE_DIR/scouting/<sha256>.StormReplay` (règle des 3 étages :
  le brut reste la source de vérité).
- **Re-process** : `POST /api/admin/reprocess` couvre aussi `scouting_games` (même critère
  `parser_version`), puis recalcule les faits des rapports touchés. Idempotent.
- Le même replay peut appartenir à deux rapports (unicité par `(report_id, fingerprint)`).

### Détection de l'équipe cible

Les parties sont celles de l'adversaire contre des équipes variées, parfois contre nous :

Règle de l'opérateur : le côté cible compte **toujours au moins 3 joueurs de l'équipe
principale**, sauf exception — et pour les exceptions, un **joueur ancre**.

1. **Candidats** : les `toon_handle` présents dans au moins 50 % des parties du rapport. Si une
   ancre est déjà définie, la détection part d'elle : candidats = joueurs du même côté que l'ancre
   dans au moins 50 % des parties de l'ancre (plus robuste quand le lot contient beaucoup de
   parties contre une même équipe, nous compris).
2. **Côté cible d'une partie**, dans cet ordre :
   1. le côté qui contient **au moins 3 joueurs du roster confirmé** → `target_source = 'roster'` ;
   2. sinon, le côté qui contient **un joueur ancre** → `'anchor'` (l'exception : line-up remaniée) ;
   3. sinon, **choix manuel** de l'opérateur dans l'onglet Games → `'manual'`.

   Tant qu'aucune règle ne tranche : `target_team = NULL`, partie affichée « team not found » et
   **exclue des faits**. Une ancre présente des deux côtés (cas impossible en jeu, donc donnée
   corrompue) → non déterminée.
3. L'opérateur **confirme ou corrige** le roster (coche/décoche, ajoute un joueur) et désigne
   **0, 1 ou plusieurs ancres** ; toute modification relance l'étape 2 (les choix manuels sont
   conservés) puis le calcul des faits.
4. Joueurs du côté cible hors roster confirmé : comptés comme **remplaçants** (apparaissent dans
   les faits joueurs avec leur nombre de parties).

Identifiant de joueur = `toon_handle`, jamais le nom affiché (qui change et se ressemble d'un joueur
à l'autre).

### Dépôt

Le front envoie les fichiers **un par un** (barre de progression, statut par fichier : ajouté /
doublon / échec de parse / équipe non trouvée) via `POST /api/scouting/{id}/replays` — corps =
octets bruts, header `X-Filename`, même contrat que `/api/upload`. Parse sur le même pool que
l'upload ; budget parse existant (< 150 ms/replay).

## Volet C — Faits calculés

Catalogue v1. **La liste exacte des champs sources est fixée au plan**, après vérification de leur
présence dans l'objet `match` des parties perso ; un fait dont la source manque est retiré, pas
approximé.

| Famille | Faits (id indicatif) |
|---|---|
| Vue d'ensemble | parties, bilan V/D, période couverte, builds/patchs, durée moyenne (`overview.*`) |
| Cartes | par carte : parties, V/D, winrate + **intervalle de Wilson 95 %** (`map.<carte>.record`) |
| Joueurs | par joueur `pN` : parties, héros joués (parties, victoires), répartition des rôles, KDA moyen, dégâts héros/min, siège/min, soins/min, morts/partie, part des dégâts de l'équipe (`pN.*`) |
| Héros × carte | héros pris par la cible sur chaque carte (`map.<carte>.picks`) |
| Draft | taux de first pick ; bans faits par la cible (global, par carte, par phase) ; bans subis ; héros pris en first pick ; rôle pris en premier ; derniers picks ; **héros joués contre eux** avec leur bilan dans ces parties (`draft.faced.*`) (`draft.*`) |
| Picks par carte | héros pris par la cible sur chaque carte **avec le joueur `pN` qui l'a joué** (`map.<carte>.pick.*`) |
| Déroulé | winrate selon premier à 10, premier fort, premier objectif ; écart de niveau moyen à 10/15/20 min ; winrate par tranche de durée ; remontées et parties perdues en tête (`flow.*`) |

Règles :
- **Chaque fait porte son `n`.** Un fait avec `n < 3` est marqué « faible échantillon » dans le
  pack (le prompt interdit d'en tirer une conclusion ferme) et grisé dans l'UI.
- Taux présentés en `k/n` **et** en pourcentage, jamais en pourcentage seul.
- Module pur `scouting/facts.rs` : entrée = parties + roster, sortie = `Facts` sérialisable.

## Volet D — Le pack LLM

Deux sorties, même contenu :

1. **`GET /api/scouting/{id}/pack.md`** (et bouton « Copier le pack ») — **format principal**,
   lisible par tous les LLM y compris locaux :
   - le **prompt** d'analyse de scouting, **en anglais**, réponse exigée en anglais (rôle, objectif : préparer draft et plan de jeu contre
     cette équipe ; règles : citer les ids, ne rien affirmer sans fait, signaler les faibles
     échantillons) ;
   - la **définition** de chaque famille de faits ;
   - la table `pN` → nom ;
   - les **faits**, en tableaux Markdown, chaque ligne avec son id ;
   - le **format de réponse imposé** (JSON Schema, volet E) et un exemple minimal ;
   - `report_id` et `facts_version`, à recopier dans la réponse.
2. **`GET /api/scouting/{id}/pack.xlsx`** — un onglet par famille de faits + un onglet Parties
   (1 ligne par partie : date, carte, adversaire du jour, résultat, durée, bans, compositions)
   + un onglet README. Pour la lecture humaine et pour les LLM qui lisent l'Excel.
   Bibliothèque d'écriture choisie au plan, **doc vérifiée via Context7**.

Le prompt vit dans un fichier versionné du dépôt (`crates/storm-codex-server/src/scouting/prompt.md`,
embarqué), pas en dur dans le code.

## Volet E — Import de l'analyse

### Format imposé (`format_version: 2` — plan de draft carte par carte)

La v1 (notes joueurs, tendances, plan de jeu général) est remplacée le 2026-09-26 à la demande de
l'opérateur : il ne veut que la draft, carte par carte. Une réponse v1 est refusée à l'import
(« copy the pack again »).

```json
{
  "format_version": 2,
  "report_id": 12,
  "facts_version": 3,
  "model": "claude-opus-5-5",
  "summary": "… identité de draft de l'équipe (3-5 phrases)",
  "map_choice": {
    "pick":  [{ "map": "…", "why": "…", "evidence": ["map.x.record"] }],
    "avoid": [{ "map": "…", "why": "…", "evidence": ["…"] }]
  },
  "maps": [{
    "map": "Braxis Holdout", "confidence": "low|medium|high",
    "overview": "ce qu'on attend d'eux sur cette carte", "evidence": ["…"],
    "bans":        [{ "hero": "…", "phase": "first|mid", "why": "…", "evidence": ["…"] }],
    "picks":       [{ "hero": "…", "why": "…", "evidence": ["…"] }],
    "their_picks": [{ "hero": "…", "player": "p3", "why": "…", "evidence": ["…"] }],
    "considerations": [{ "point": "…", "evidence": ["…"] }]
  }],
  "general": { "bans": [], "picks": [], "considerations": [] }
}
```

Une entrée `maps` par carte présente dans les faits ; `general` vaut pour toute carte, y compris
celles absentes des replays. Chaque élément porte ses `evidence` (vérifiées à l'import).

Le JSON Schema est un fichier du dépôt, embarqué dans le pack et utilisé à la validation.

### Import — `PUT /api/scouting/{id}/analysis`

Corps = **texte brut** de la réponse du LLM (collé ou fichier déposé). Le serveur :

1. **Extrait** le JSON : bloc ```` ```json ```` s'il existe, sinon premier objet JSON équilibré du
   texte (les modèles locaux l'entourent souvent de prose).
2. **Valide** contre le schéma v1 — tolérant : champs inconnus ignorés, sections absentes
   acceptées. JSON cassé ou `format_version` inconnu → refus avec l'erreur exacte, rien n'est
   écrasé.
3. **Résout les preuves** : chaque id cité est cherché dans les faits courants ; affirmation sans
   preuve ou avec id inconnu → marquée `unsupported` (conservée, signalée).
4. **Contrôle de cohérence** : `report_id` différent → refus ; `facts_version` antérieur → accepté
   avec avertissement « analyse faite sur un état antérieur du rapport ».
5. **Écrase** l'analyse existante (décision opérateur) — l'UI demande confirmation avant l'envoi
   quand une analyse existe déjà.

Réponse : l'analyse validée + un rapport d'import (nb d'affirmations, nb `unsupported`,
avertissements). `DELETE /api/scouting/{id}/analysis` retire l'analyse.

### Statut d'un rapport (dérivé, pas stocké)

`empty` (aucune partie) → `ready` (faits calculés, pas d'analyse) → `analyzed` → `stale` (analyse
importée sur un `facts_version` antérieur : des replays ont été ajoutés/retirés depuis).

## Interface

Langue de l'UI : anglais (convention `08-frontend.md`).

### `/scouting` — liste des rapports

Cartes triées par date de création décroissante : **titre**, équipe cible, date de création,
nombre de parties, période/patchs couverts, **badge de statut**, date d'import de l'analyse.
Bouton « New report » (titre + nom d'équipe → ouvre le rapport).

### `/scouting/:id` — un rapport

- **En-tête** : titre **éditable en place**, équipe cible, dates de création / mise à jour,
  statut, actions : *Copy pack*, *Download .md*, *Download .xlsx*, *Import analysis*.
- **Onglets** :
  - **Overview** — synthèse de draft + **choix de carte** (à prendre / à éviter) ;
    faits d'ensemble.
  - **Players** — une carte par joueur (`pN` + nom + avatar) : pool de héros, rôles, stats clés,
    niveau de menace et notes de l'analyse.
  - **Draft** — plan « toute carte » de l'analyse, puis bans, first/last picks, héros joués contre
    eux avec leur bilan.
  - **Maps** — **le plan de draft** : une carte par map (bilan + intervalle, ce qu'ils ont pris et
    par qui, leurs bans, les bans subis), puis « We ban » (avec phase), « We pick », « They will
    likely pick » (avec le joueur), « Consider », et le niveau de confiance. Les cartes que
    l'analyse traite sans qu'elles soient dans les replays apparaissent aussi (« not in these
    replays »).
  - **Games** — la liste des replays du rapport, zone de dépôt, statut par fichier, côté cible
    détecté et **par quelle règle** (roster / ancre / manuel), choix manuel du côté pour une
    partie non déterminée, retrait d'un replay.
  - **Roster** — candidats détectés, cases à cocher, remplaçants, **désignation des ancres**.
- **Preuves** : chaque affirmation de l'analyse porte des **puces** avec la valeur réelle et `k/n`
  (survol = détail du fait) ; une affirmation `unsupported` est visiblement marquée.
- **Import** : fenêtre avec zone de collage + dépôt de fichier, aperçu du rapport d'import avant
  confirmation, confirmation d'écrasement si une analyse existe.
- Le rapport est **utile sans analyse** : tous les onglets affichent les faits seuls.

## API (récapitulatif)

| Route | Rôle |
|---|---|
| `GET /api/scouting` | liste (sans `facts` ni `analysis`) |
| `POST /api/scouting` 🔒 | créer `{title, target_name}` |
| `GET /api/scouting/{id}` | rapport complet : méta, parties (sans `data`), candidats roster, faits, analyse avec preuves résolues, statut |
| `PATCH /api/scouting/{id}` 🔒 | `title`, `target_name`, `roster` (→ recalcul) |
| `DELETE /api/scouting/{id}` 🔒 | supprimer (parties en cascade ; les bruts archivés restent) |
| `POST /api/scouting/{id}/replays` 🔒 | ajouter un replay (octets bruts, `X-Filename`) |
| `DELETE /api/scouting/{id}/replays/{gid}` 🔒 | retirer un replay (→ recalcul) |
| `GET /api/scouting/{id}/pack.md` · `/pack.xlsx` | pack LLM |
| `PUT /api/scouting/{id}/analysis` 🔒 | importer (écrase) |
| `DELETE /api/scouting/{id}/analysis` 🔒 | retirer l'analyse |

**Budget** : `GET /api/scouting/{id}` p95 < 100 ms (contrat d'API) sur un rapport de 30 parties —
tenu par l'instantané de faits (décision 5), mesuré.

## Volet G — Export HTML à partager (demande opérateur, 2026-09-27)

Bouton **Export HTML** sur la page d'un rapport : télécharge **un seul fichier `.html` autonome**
que l'opérateur envoie à ses coéquipiers (Discord, mail). Exigence : « highly visual ».

- **Autonome** : portraits des héros et minimaps embarqués en `data:` URI (réduits dans le
  navigateur : portraits 128 px, minimaps 900 px, WebP) ; aucune dépendance à l'app ni au box.
  Seules les polices viennent de Google Fonts, avec repli système si hors ligne.
- **Généré côté navigateur** (`web/src/reportExport.ts` charge les images ; `web/src/reportHtml.ts`
  rend — fonction **pure**, testée en vitest). Aucune route serveur nouvelle.
- **Sécurité** : tout texte venu du LLM ou des replays est échappé (le fichier circule hors de l'app).
- **Direction artistique** : « dossier de scouting » dans le langage de l'écran de draft HotS.
  Couverture (nom de l'équipe en grand, bilan avec pastilles V/D, first pick, avertissement de
  faible échantillon, médaillons du roster), identité de draft (synthèse + bans/ouvertures en
  portraits), choix de carte (tuiles sur minimap), **un tableau de draft par carte** (en-tête
  minimap, jauge de confiance, colonnes « We ban » — portraits barrés de rouge avec la phase —,
  « We pick » — halo vert —, « They will likely pick » — halo orange + joueur —, « Consider », puis
  ce qu'ils ont joué/banni sur la carte), plan « toute carte », cartes joueurs (pool en barres),
  héros affrontés (ceux qui les ont battus en évidence), journal des parties. Chaque recommandation
  garde ses preuves (libellé + valeur) ; « ⚠ unverified » si aucune. Navigation par cartes en tête,
  responsive, impression propre, animations respectant `prefers-reduced-motion`.
- Fonctionne sans analyse importée (faits seuls) et sans portraits (médaillons à initiales sur
  l'anneau de couleur de l'univers).

### Page de l'app au même style (demande opérateur, 2026-09-27)

« Les mêmes visuels dans le site, sans toucher à l'export » : les pages `/scouting` et
`/scouting/:id` adoptent le langage de l'export (couverture, tableaux de draft par carte, portraits
barrés / lumineux, preuves compactes). L'export n'est **pas modifié** : la page réutilise seulement
ses helpers purs (`compactEvidence`, `humanize`) et sa table de couleurs d'univers. Les styles de
l'export stylisent `body`/`h1`/`main` : ils sont **portés** dans une feuille scopée `.sd`, pas
injectés. Les onglets disparaissent au profit d'une page unique avec barre de sections ; dépôt,
côté manuel et roster deviennent les deux dernières sections.

## Volet F — Plus tard, hors v1

- **Fiche de partie pour les replays de scouting** (réutiliser `MatchDetail` et la visionneuse 2D
  sur une source `/api/scouting/{id}/games/{gid}` de même forme que `/api/matches/{id}`).
- **Passerelle vers le simulateur de draft** : charger les tendances et bans recommandés du rapport
  dans `/draft` pendant une vraie draft.

**Non-but** : aucun appel de LLM depuis l'app (l'opérateur n'a pas d'API OpenAI/Anthropic).
L'étape LLM est manuelle : copier le pack, le coller dans ChatGPT/Claude/modèle local, recoller la
réponse. D'où l'importance du bouton « Copy pack » et d'un import tolérant.

## Ordre de livraison

A (porte d'entrée) → B → C → D (`.md` d'abord, `.xlsx` ensuite) → E + UI. Chaque volet est
vérifiable seul ; B–C sont utiles sans LLM.

## Critères d'acceptation

| Volet | Critère mesurable |
|---|---|
| A | 3 replays réels de parties perso avec draft : bans, first pick et ordre des picks identiques à ce que montre le client HotS (confirmation opérateur) — sinon l'écart est écrit ici et le volet Draft retiré |
| B | Migration appliquée sur le Postgres dev ; sur un lot réel de replays d'une équipe, le roster détecté correspond à l'équipe (confirmation opérateur) ; **isolation prouvée** : réponses de `/api/heroes`, `/api/maps`, `/api/players/{toon}` identiques avant/après l'ajout de 20 replays de scouting (test d'intégration) ; reprocess idempotent sur `scouting_games` |
| C | Tests unitaires de `scouting/facts.rs` : bilans, Wilson, exclusion des parties sans côté cible, remplaçants, `n` sur chaque fait ; tests de la détection de côté : ≥ 3 du roster, repli sur l'ancre, choix manuel conservé après changement de roster, ancre des deux côtés → non déterminée |
| D | Pack `.md` d'un rapport réel donné à Claude **et** à ChatGPT : les deux renvoient une réponse que l'import accepte ; `.xlsx` ouvert sans erreur dans Excel |
| E | Tests de l'extraction (bloc fenced, prose autour, JSON cassé → refus sans écrasement, `report_id` faux → refus) et de la résolution des preuves (id inconnu → `unsupported`) ; `GET /api/scouting/{id}` mesuré p95 < 100 ms sur 30 parties ; vérif navigateur sur un rapport réel, analyse importée puis réimportée (écrasement) |

## Impact documentaire (règle n° 0)

| Fichier | Ce qui change |
|---|---|
| `docs/spec/05-api.md` | section « Scouting » (routes ci-dessus) |
| `docs/spec/06-modele-donnees.md` | tables `scouting_reports`, `scouting_games`, migration |
| `docs/spec/08-frontend.md` | routes `/scouting`, `/scouting/:id` |
| `docs/spec/04-serveur.md` | archivage `ARCHIVE_DIR/scouting/`, reprocess étendu |
| `docs/spec/01-architecture.md` | p95 mesuré de `GET /api/scouting/{id}` |

## Questions tranchées (2026-09-26)

1. **Roster** : au moins 3 joueurs de l'équipe principale, sinon joueur ancre (et choix manuel en
   dernier recours) — intégré au volet B.
2. **Lien avec `teams`** : non.
3. **Noms dans le pack** : toujours inclus.
4. **Langue de l'analyse** : anglais.
