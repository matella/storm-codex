# Companion live (pré-game / post-game) — design

> Spec storm-codex. Brainstorming validé par l'opérateur le 2026-08-27.
> Cible : spike go/no-go, puis `writing-plans` et implémentation TDD.

## But

Une page **companion privée** (second écran / téléphone) qui s'ouvre toute seule quand une partie
se charge : qui est dans le lobby et ce qu'on sait d'eux, tes propres stats sur ce héros et cette
carte, et **le build à suivre**, tiré d'une bibliothèque que tu as constituée. Quand le replay
remonte, la même page bascule en **debrief** : ce que tu avais prévu contre ce que tu as pris.

Pas un overlay de stream : page dense, pour toi seul.

## Réouverture d'une décision verrouillée

La spec programme (`docs/specs/2026-06-12-storm-codex-design.md`) verrouille « pas de mode pré-game
(`.battlelobby`) en V1 » — décision n° 6 du tableau des décisions, reprise en « Hors scope V1 ».
**L'opérateur rouvre explicitement cette décision le 2026-08-27** et retient la détection
automatique. Trace ici, conformément à la règle du repo.

## Décisions actées (brainstorming)

1. **Audience** : toi, sur un second écran. Pas d'overlay OBS, donc pas de contrainte de lisibilité
   à distance ni de question de vie privée sur les stats des autres joueurs.
2. **Déclenchement : automatique d'emblée**, via `replay.server.battlelobby`. Le risque (format
   rétro-ingénieré, fragile aux builds Blizzard) a été posé et assumé par l'opérateur. Il est
   traité par un spike préalable, un harnais de parité chiffré, et des dégradations explicites.
3. **Contenu pré-game** : les quatre blocs — ton build suggéré, tes 4 coéquipiers, les 5
   adversaires, tes propres stats sur ce héros / cette carte.
4. **Builds** : saisie manuelle **plus** amorçage depuis une partie jouée. Un build marqué défaut
   par héros = celui proposé ; les autres listés dessous. Pas de moteur de règles contextuelles en
   V1 (YAGNI — à rouvrir si le besoin se manifeste à l'usage).
5. **Post-game** : uniquement le **debrief de build** (prévu vs pris). Le suivi de session existe
   déjà dans `/queue`, la fiche de match et la visionneuse 2D existent déjà — on ne duplique pas.
6. **Pipeline** : `client-rs` pousse les **octets bruts**, le serveur parse. Le parser fragile vit
   côté serveur, donc une casse de format se corrige par un redéploiement du box, sans jamais
   retoucher ni redéployer le binaire Windows.

## Ce qui existe déjà et qu'on réutilise

| Existant | Usage ici |
|---|---|
| `storm_replay::Replay::battlelobby_raw()` (`crates/storm-replay/src/lib.rs:258`) | fournit des blobs lobby réels pour tests et parité |
| `get_battletags` (`crates/storm-stats/src/process.rs:313`) | référence de comportement (regex battletags), mais insuffisant seul : il croise `details.m_playerList`, absent en live |
| `/api/upload-raw` (octets bruts + `Bearer` + `X-Filename`) | modèle exact de `POST /api/lobby` ; token `matella-pc` déjà émis |
| canal broadcast `/ws` (`match.parsed`, `draft.updated`) | on ajoute `lobby.detected` / `lobby.updated` |
| `draft_live` + `draft/store.rs` | calque pour le singleton `lobby_live` |
| `dim_heroes.id = name = match_players.hero` (`dim.rs:48`) | clé héros unique dans tout le système |
| `dim_talents.tree_id` (migration 0005) | `talentTreeId` → nom/tier/icône |
| `match_players.data.talents` = `{TierNChoice: talentTreeId}` | **même forme que `builds.picks`** → import et diff triviaux |
| `app_settings.operator_names` + `pickOperator`/`matchOperator` | identifie lequel des 10 joueurs c'est toi |
| `Avatar`, portraits et cartes vendorisés, tokens Nexus Codex | rendu |

## Spike go/no-go (préalable, sur le Mac)

Aucune ligne de serveur ni de front avant ça.

1. Extraire le blob lobby des 4 replays committés (`crates/*/tests/data/*.StormReplay`) et
   l'analyser à l'octet près.
2. **Trancher la question ouverte : le héros pické figure-t-il dans le battlelobby ?** Inconnu à ce
   jour ; ne pas le supposer. Si oui, déterminer la forme du nom (vraisemblablement un nom interne
   type `HeroTychus`) et la normalisation vers `dim_heroes.id`.
3. Vérifier si le blob porte la carte et le mode.
4. Vérifier si les octets du fichier temporaire et du stream archivé sont identiques (décide la
   méthode de liaison replay↔lobby, cf. plus bas).
5. Faire tourner le parser candidat sur l'archive du box (mesuré : 3 322 replays, 25 builds
   2024→2026) et diffuser contre le parse complet, qui connaît la vérité.

**Critère d'acceptation du spike : ≥ 99 % des lobbies avec noms, BattleTags et équipes exacts,
sur les modes matchmakés** (Storm League, ARAM, Quick Match). Les parties personnalisées sont hors
critère : des observateurs y siègent dans le lobby et l'ordre n'y porte pas l'équipe.

Le spike ne peut pas faire échouer la feature — il détermine si elle coûte **zéro clic** (héros
présent) ou **un tap** (héros absent, sélecteur manuel).

## Architecture

```
PC de jeu (client-rs)                    Box (storm-codex-server)            Toi (2e écran)
watcher %TEMP%\…\TempWriteReplayP1\
  replay.server.battlelobby
      │ écriture détectée, debounce,
      │ dédup par hash de contenu
      └─ POST /api/lobby (octets bruts) ─► storm_lobby::parse(&[u8])
                                           ├─ enrichissement Postgres
                                           ├─ UPSERT lobby_live (singleton)
                                           └─ WS lobby.detected ────────────► /companion

  … partie jouée …
  replay écrit → upload existant ────────► parse → projection → match.parsed ─► debrief
```

### Crate `crates/storm-lobby` (pur, zéro I/O)

Ne dépend pas de `storm-replay` : il reçoit des octets. Cohérent avec `storm-replay-viewer`
(géométrie du problème isolée, testable seule, publiable au jalon 6).

> **Mis à jour le 2026-08-27 après l'exécution du spike.** Constats mesurés
> (`docs/research/2026-08-27-lobby-format.md`) : les BattleTags sont **présents en clair**, préfixés
> par une longueur en octets UTF-8 ; le **toon handle, le héros pické, la carte et le mode sont
> absents** ; l'équipe n'a **aucun champ explicite** et se déduit de l'ordre (5+5). Le type public
> ne porte donc que ce qui est réellement décodable — un champ toujours `None` serait du poids mort.

```rust
pub fn parse(bytes: &[u8]) -> Result<Lobby, LobbyError>;

pub struct Lobby {
    pub players: Vec<LobbyPlayer>,   // ordre d'apparition dans le blob
}
pub struct LobbyPlayer {
    pub name: String,                // peut contenir de l'UTF-8 non-ASCII
    pub discriminant: String,        // partie après '#'
    pub team: Option<u8>,            // déduit de l'ordre, uniquement si 10 joueurs pile
}
impl LobbyPlayer {
    pub fn battletag(&self) -> String;  // "nom#1234" — la clé d'identité
}
```

Erreurs typées (`thiserror`), pas d'`unwrap()` hors tests.

**Résolution de l'identité.** Le blob ne donne pas le toon handle : le serveur le retrouve en
rapprochant `nom#discriminant` de `match_players.name` + `match_players.data->>'tag'`, c'est-à-dire
**contre l'archive elle-même**. Un joueur absent de l'archive reste non résolu — sans conséquence,
puisqu'il n'a de toute façon aucun historique à afficher.

### Module serveur `lobby.rs`

Endpoint, enrichissement, singleton. Calque `draft/store.rs`. Le lobby courant écrase le
précédent ; aucun historique de lobbies (le replay archivé reste la source de vérité, étage 1).

## Modèle de données — migration `0009_companion.sql`

```sql
-- Bibliothèque de builds. `picks` a EXACTEMENT la forme écrite par le parser dans
-- match_players.data.talents : {"Tier1Choice": "<talentTreeId>", ...}.
CREATE TABLE builds (
    id              BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    hero_id         TEXT NOT NULL,          -- = dim_heroes.id = match_players.hero
    name            TEXT NOT NULL,
    picks           JSONB NOT NULL,
    notes           TEXT,
    is_default      BOOLEAN NOT NULL DEFAULT false,
    source_match_id BIGINT REFERENCES matches(id) ON DELETE SET NULL,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX builds_hero_idx ON builds(hero_id);
-- Invariant tenu par la base : au plus un build par défaut par héros.
CREATE UNIQUE INDEX builds_one_default_per_hero ON builds(hero_id) WHERE is_default;

-- Lobby courant. Singleton, calque de draft_live.
CREATE TABLE lobby_live (
    id         INT PRIMARY KEY DEFAULT 1 CHECK (id = 1),
    state      JSONB NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
```

### État enrichi (`lobby_live.state`, porte un `schema_version`)

Le serveur ne renvoie pas le lobby brut mais le lobby joint à l'archive :

- **par joueur** : `known` ; parties avec toi / contre toi et winrates ; dernière rencontre ; ses 3
  héros les plus joués ; ses stats sur le héros du moment.
- **pour toi** (via `operator_names`) : winrate sur ce héros, sur cette carte, sur le couple
  héros×carte, tes dernières parties sur ce héros.
- **build** : le défaut du héros + alternatives, talents résolus via `dim_talents.tree_id`.
- `match_id` : rempli à la liaison → bascule en debrief.

Agrégats SQL sur `match_players` ⋈ `matches` (~20 000 lignes, index existants sur `toon_handle`,
`hero`, `match_id`). Budget : voir le critère 3 ci-dessous, amendé après mesure.

### Liaison replay ↔ lobby

**Méthode retenue : l'ensemble des 10 BattleTags**, plus une fenêtre temporelle de quelques
heures. Le parse complet reconstruit les mêmes BattleTags depuis le blob embarqué dans le replay
(`get_battletags`, `process.rs:313`), donc les deux côtés portent la même clé.

Alternative plus exacte — hasher les octets du blob et comparer à celui extrait du replay — retenue
**seulement si le point 4 du spike confirme** l'identité bit-à-bit. Sinon on garde les handles.

## API

| Route | Rôle |
|---|---|
| `POST /api/lobby` | octets bruts, `Bearer` upload — parse, enrichit, upsert, broadcast. Idempotent. |
| `GET /api/lobby` | état enrichi courant ; `204` si aucun |
| `POST /api/lobby/hero` | `{hero}` — repli 1 tap si le blob ne porte pas le héros |
| `DELETE /api/lobby` | ferme le companion |
| `GET/POST/PUT/DELETE /api/builds` | CRUD bibliothèque (même garde admin que teams/collections) |
| `POST /api/builds/from-match` | `{match_id, toon_handle, name}` → amorçage |

WS : `lobby.detected` à la réception d'un nouveau lobby ; `lobby.updated` sur toute mutation
ultérieure du même lobby (héros saisi à la main, liaison au replay, fermeture). Canal existant,
consommés comme `match.parsed` — invalidation TanStack Query, pas de push d'état.

## Front

### `/companion` — trois états sur une route, pilotés par le WS

- **Au repos** : « en attente d'un lobby », dernier debrief, accès à la bibliothèque. Jamais vide.
- **Pré-game** : bandeau carte · mode · ton héros. **Le build domine la page** (7 tiers, gros) —
  c'est ce qui se consomme pendant les ~45 s de chargement. Les 10 joueurs en deux colonnes
  d'équipe (badge connu/inconnu, V-D avec ou contre toi, héros du moment). Tes stats héros / carte /
  héros×carte en dessous du build.
- **Debrief** : diff tier par tier, prévu vs pris, marqué ✓/✗, plus le résultat. Liens vers la
  fiche de match et la visionneuse 2D — qui existent, et qu'on ne réimplémente pas.

### `/builds` — bibliothèque

Liste par héros, éditeur 7 tiers alimenté par `dim_talents`, marquage du défaut. Le bouton
d'amorçage « enregistrer comme build » vit **dans la fiche de match**, là où tu es quand tu
constates qu'une partie s'est bien passée.

## Dégradations (comportements exigés)

| Ce qui casse | Comportement |
|---|---|
| Blizzard change le format | `POST /api/lobby` répond 200 avec `parse_failed` classé, build loggé en clair. La page affiche « lobby illisible (build X) » **et** le sélecteur de héros → le build suggéré s'affiche quand même. Perte des 9 joueurs, pas de la fonction principale. |
| Héros absent du blob (**confirmé** par le spike) | sélecteur, un tap |
| Joueur jamais croisé | « jamais croisé » écrit tel quel — jamais un 50 % fabriqué sur zéro partie |
| Carte absente du blob (**confirmé** par le spike) | sélecteur, un tap ; à défaut, stats carte au debrief |
| Aucun build pour ce héros | « aucun build » + raccourci « importer depuis ta meilleure partie sur ce héros » (données déjà en base) |
| Box injoignable | état précédent affiché et marqué périmé — pas de spinner infini |

## Tests

- **`storm-lobby`** : aucun golden — l'oracle (`tests/oracle.rs`) diffe contre le parse complet sur
  les 4 replays committés du workspace ; **ne panique jamais** sur entrée tronquée, vide ou
  aléatoire (`Err`, pas `panic`, `tests/robustness.rs`).
- **`crates/storm-lobby/examples/parity.rs`** : parser autonome sur l'archive du box, diff contre le
  parse complet, ventilé par mode de jeu. **Critère : ≥ 99 % (noms, BattleTags, équipes) sur les
  modes matchmakés** — voir le verdict mesuré dans `docs/research/2026-08-27-lobby-parity.md`.
- **Serveur** : invariant du build par défaut (violation refusée par la base) ; idempotence de
  l'upsert ; agrégats d'enrichissement sur base semée ; liaison par ensemble de handles.
- **Front** : vitest sur les parties pures — diff de build, classification connu/inconnu. Même
  approche que `replay2d.ts`, pas de test DOM lourd.
- **E2E sur le Mac** : Postgres local + serveur + replay committé → extraire son blob → `POST
  /api/lobby` → page remplie → uploader le replay → debrief. Reproductible sans PC de jeu ni partie
  en cours.

## Critères d'acceptation

1. Spike : parité lobby **≥ 99 % sur les modes matchmakés** (personnalisées hors critère) ;
   présence du héros tranchée par oui ou non. → **atteint : 100 %** sur 2 710 parties.
2. Détection lobby → page remplie **< 2 s**.
3. `/api/lobby` : **`GET` p95 < 100 ms** (il sert l'état depuis la mémoire — mesuré à ~1 ms) ;
   **`POST` p95 < 500 ms**.

   **Amendement du 2026-08-28, décision opérateur, mesure à l'appui.** Le critère initial était
   « `/api/lobby` p95 < 100 ms », hérité du contrat d'API général de la spec programme. Mesuré par
   `crates/storm-codex-server/backfill_bench.py` : le `POST` tient **18 ms en régime chaud**, mais
   **165 ms à froid** — reproductible sur 3 redémarrages (164,6 / 167,6 / 169,3 ms). Or le régime
   chaud n'existe pas à l'usage : il y a **un `POST` par partie**, serveur inactif entre deux, donc
   le chemin à froid **est** le chemin nominal.

   Le contrat de 100 ms n'est donc pas tenu, et prétendre le contraire serait faux. Il est amendé
   plutôt que contourné, parce que le besoin réel ne le justifie pas : ce `POST` s'exécute pendant
   un écran de chargement de **45 secondes**, où 165 ms sont imperceptibles. Le seuil de 500 ms
   laisse une marge de 3× sur la mesure et resterait invisible à l'usage.

   Ce qui n'est **pas** décidé : la cause des ~150 ms de froid (établissement de connexion du pool,
   planification SQL, autre) n'a pas été investiguée. Si ce seuil venait à sauter, c'est le premier
   endroit où regarder — un `min_connections` sur le pool sqlx est l'hypothèse la plus probable.
4. E2E scripté et rejouable sur le Mac.
5. Chaque dégradation du tableau ci-dessus exercée par un test ou une vérification manuelle tracée.

## Livrable hors de ce repo

Le watcher vit dans **`client-rs` (repo Hots-Overlay)** : surveiller
`%TEMP%\Heroes of the Storm\TempWriteReplayP1\replay.server.battlelobby`, gérer le fichier
verrouillé pendant l'écriture, **dédupliquer par hash de contenu** (le fichier survit entre deux
parties — sans ça, le lobby de la partie précédente se rouvrirait à chaque lancement).

C'est le seul morceau qui exige le PC de jeu, et le dernier de la séquence : tout le reste se
développe et se prouve depuis le Mac.

## Hors scope V1

- Overlay OBS du companion (décision : page privée uniquement).
- Moteur de règles contextuelles pour choisir le build (carte, matchup, composition).
- Source externe de stats pour les joueurs inconnus (API HeroesProfile) — les inconnus restent
  affichés comme inconnus.
- Debrief élargi aux performances des coéquipiers.
- Historique des lobbies.
