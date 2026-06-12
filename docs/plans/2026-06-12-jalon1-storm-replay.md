# Jalon 1 — crate `storm-replay` : décodage complet des 7 streams

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal :** un crate Rust publiable (`crates/storm-replay`) qui décode les **7 streams** d'un
`.StormReplay` (header, details, initdata, attributes, messages, game events, tracker events),
avec tables de protocole générées (`protocol-gen`), fallback builds inconnus, erreurs typées.
**Accept (spec) : 100 % du corpus décodé** (= l'archive locale entière, 2 652 replays) **et
bench < 150 ms/replay hors stats** (7 streams compris).

**Architecture :** le spike (jalon 0) a validé nom-mpq + le décodeur versioned interprétant des
typeinfos JSON (12 ms/replay sur 3 streams). Le jalon 1 industrialise : workspace cargo racine,
crate `storm-replay` (thiserror, pas d'`unwrap()` hors tests, clippy strict), port du
**BitPackedDecoder** (bit-à-bit big-endian — requis par initdata/game/message events, absent du
spike), décodeur attributes (format little-endian à part), tables **dédupliquées committées**
(390 builds → 32 contenus distincts ≈ 0,5 MB, embarquées par `include_str!`), parité prouvée
stream par stream contre heroprotocol.

**Tech stack :** Rust 2021 (deps lib : `serde_json`, `thiserror` ; dev : rien de plus),
Python 3.13 pour protocol-gen/cross-check (clone GitHub Blizzard/heroprotocol — PyPI interdit,
cf. rapport jalon 0).

---

## Contexte pour l'exécutant

- Référence exacte des décodeurs : `decoders.py` du package pip heroprotocol
  (`python -c "import heroprotocol, pathlib; print(pathlib.Path(heroprotocol.__file__).parent)"`).
  Le spike a déjà porté `VersionedDecoder` (`spike/storm-decode/src/versioned.rs`) — le
  **BitPackedDecoder** (classe au-dessus dans le même fichier) reste à porter : lecture de bits
  big-endian (`read_bits` : bits de poids faible de `_next` placés en poids fort du résultat),
  `_int` = `bounds[0] + read_bits(bounds[1])` — **les bounds comptent ici** (le versioned les
  ignore), `_struct` séquentiel sans tags, `_choice` sans skip (tag inconnu = corrompu),
  `_bitarray` → (longueur, entier), `_blob` → byte_align puis octets alignés.
- Attributes : `decode_replay_attributes_events` dans tout module protocole (ex.
  protocol91756.py:453) — BitPackedBuffer **little-endian**, scopes/attrid, valeur 4 octets
  inversés et dépouillés des `\x00`.
- Boucle d'événements : `_decode_event_stream` (protocolXXXXX.py:366) — delta svaruint32,
  userid **seulement** pour game/message events, eventid, struct, `byte_align()` entre
  événements. Champs ajoutés : `_event`, `_eventid`, `_gameloop`, `_userid` (pas `_bits` :
  on ne l'émet pas, le cross-check le strippe côté Python).
- Spike réutilisable : `spike/export_protocols.py` (base de protocol-gen),
  `spike/storm-decode/src/{versioned,protocol}.rs` (à adapter en lib propre),
  corpus `corpus/spike50` + archive complète locale (~2 652 replays, builds ≥ 92665 → tous
  couverts par les protocoles GitHub, fallback validé pour 97039).
- Conventions repo : thiserror dans la lib (anyhow toléré uniquement dans les binaires
  outils/tests), commits conventionnels, clippy `-D warnings`.

---

### Task 1 : workspace racine + squelette du crate

- Racine : `Cargo.toml` workspace `members = ["crates/storm-replay"]`,
  `exclude = ["spike/storm-decode"]` (le spike garde son lockfile, il est figé).
- `crates/storm-replay` : lib `src/lib.rs` + `src/error.rs` (`thiserror` :
  `Error::{Io, Mpq, Truncated, Corrupted, UnknownType, MissingStream, Protocol}` — classes
  d'erreur stables, elles serviront aux stats d'échec du serveur au jalon 3).
- `cargo clippy --workspace -- -D warnings` vert. Commit.

### Task 2 : protocol-gen (tables dédupliquées committées)

- `tools/protocol_gen.py` : depuis un clone GitHub (clone automatique en `%TEMP%/heroprotocol`
  si absent, option `--clone-dir`), exporte par build le JSON **complet** (typeinfos + les 6
  constantes typeid + `tracker_event_types` + `game_event_types` + `message_event_types` +
  `game_eventid_typeid`, `message_eventid_typeid`), déduplique par hash du contenu
  (hors `base_build`), écrit :
  - `crates/storm-replay/protocols/tables/<hash12>.json` (~32 fichiers),
  - `crates/storm-replay/protocols/index.json` (`{"builds": {"92665": "<hash12>", …},
    "latest": 96477}`),
  - `crates/storm-replay/protocols/embed.rs` généré : `static TABLES: &[(&str, &str)] =
    &[("<hash12>", include_str!("tables/<hash12>.json")), …];` + index builds.
- Vérif : 390 builds dans l'index, ~32 tables, taille totale < 1 MB. Commit (tables incluses —
  relançable à chaque patch HotS).

### Task 3 : décodeurs (versioned porté + bitpacked nouveau)

- `src/value.rs` : `Value` du spike + `BitArrayInt(u64 bits, u64 valeur)` (variante bitpacked).
- `src/typeinfo.rs` : `TypeInfo` avec **bounds conservés** (`Int{lo, bits}`, `Choice{lo, bits,
  fields}`, `Array{lo, bits, typeid}`, `Blob{lo, bits}`, `BitArray{lo, bits}`) — parse depuis le
  JSON ; le versioned les ignore, le bitpacked les utilise.
- `src/versioned.rs` : port du spike adapté (`Error` typée au lieu d'anyhow).
- `src/bitpacked.rs` : `BitReader` big-endian fidèle à `BitPackedBuffer.read_bits` + endian
  little pour attributes ; `BitPackedDecoder::instance`. Tests unitaires : vint (0, 1, -1,
  0x3F, gros nombres), read_bits big-endian sur un motif connu (comparer à un calcul Python
  fait à la main dans le commentaire du test).
- Commit.

### Task 4 : attributes + tables embarquées + boucles d'événements

- `src/attributes.rs` : port de `decode_replay_attributes_events` (struct typée
  `Attributes{source, map_namespace, scopes: HashMap<u8, HashMap<u32, Vec<AttributeValue>>>}`).
- `src/protocol.rs` : `Protocol` chargé depuis les tables embarquées (`OnceLock`, parse JSON
  une fois par table et par process) ; `for_build(build) -> (protocole, fallback: bool)` ;
  `decode_header/details/initdata` ; `decode_tracker_events` (versioned, sans userid) ;
  `decode_game_events`/`decode_message_events` (bitpacked, avec userid, `byte_align` entre
  événements).
- Commit.

### Task 5 : API publique `Replay` + mini-corpus + tests d'intégration

- `src/lib.rs` : `Replay::open(path)` / `Replay::from_bytes` → MPQ parsé + header décodé à
  l'ouverture (build, durée, version typées : `ReplayHeader`) ; streams **paresseux** :
  `details()` (vue typée `ReplayDetails{title, players: Vec<PlayerDetails{name, hero, team,
  result, toon…}>, timestamp}` + `details_raw()` Value), `initdata_raw()`, `attributes()`,
  `message_events()`, `game_events()`, `tracker_events()` (Value + nom d'événement) ;
  `protocol_fallback() -> Option<(demandé, utilisé)>` (warning à la charge de l'appelant).
- Mini-corpus CI : copier 2 replays récents + 1 de 2024 depuis l'archive locale vers
  `crates/storm-replay/tests/data/` (~3 MB committés — replays de l'opérateur ; signalé dans le
  résumé de session pour veto éventuel). Tests d'intégration : les 7 streams de chaque replay
  du mini-corpus se décodent, invariants (10 joueurs, 5 vainqueurs, > 1 000 tracker events,
  attributes non vides, game events > tracker events).
- `cargo test --workspace` + clippy verts. Commit.

### Task 6 : parité stream par stream vs heroprotocol

- Bin `src/bin/storm-replay-dump.rs` : `storm-replay-dump <replay> --stream <nom>` → JSON
  lines normalisé « comme heroprotocol » (blobs en latin-1, structs = objets, reals = `[f]`,
  bitarrays selon le décodeur, `_event/_eventid/_gameloop/_userid`).
- `tools/crosscheck_streams.py` : sur les **3 replays les plus récents** du corpus spike50 +
  le replay 2024 du mini-corpus, pour chacun des 7 streams : décode via heroprotocol (loader
  importlib du jalon 0) et via storm-replay-dump, normalise (strip `_bits`, tuples→listes,
  bytes→latin-1), **deep-compare**. Attendu : égalité exacte partout (tolérance documentée
  uniquement si un cas réel l'exige — alors la documenter dans le rapport).
- Commit.

### Task 7 : sweep d'archive (critère 100 %) + bench (critère < 150 ms)

- Bin `src/bin/storm-replay-verify.rs` : parcourt un répertoire récursivement, décode les
  7 streams de chaque `.StormReplay`, classe les échecs par `Error` ; sortie CSV + résumé.
- Run sur l'archive locale complète (`Documents/Heroes of the Storm/.../Multiplayer`).
  **Accept : 100 % décodés** (sinon : corriger le décodeur — les builds locaux sont tous
  ≥ 92665, aucune excuse « builds alpha »).
- Bench : `storm-replay-dump --bench corpus/spike50` (7 streams, mono-thread, warm-up exclu,
  médiane/p95/max → `spike/bench-results/rust-jalon1.csv`). **Accept : médiane et p95 < 150 ms.**
- Commit.

### Task 8 : finitions + STATUS + push

- README du crate (usage, protocol-gen, fallback, perfs mesurées), doc rustdoc sur l'API
  publique, `cargo fmt` + clippy + tests une dernière fois.
- `docs/STATUS.md` : jalon 1 fini (chiffres sweep + bench), prochaine étape jalon 2.
- Commit + **git push** (autorisation opérateur du 2026-06-12).

---

## Pièges connus

- `read_bits` big-endian : on consomme les bits de **poids faible** de l'octet courant mais on
  les place en **poids fort** du résultat — porter la boucle telle quelle, ne pas « simplifier ».
- `_struct` bitpacked n'a **pas** de tags ni de skip : tout champ se décode séquentiellement ;
  un protocole inadapté ⇒ `Corrupted`, c'est le comportement attendu (pas de tolérance).
- `_bitarray` versioned = octets alignés ; bitpacked = entier de `length` bits. Deux variantes
  de `Value`, ne pas fusionner.
- Attributes : buffer **little-endian** (l'unique usage), valeurs 4 octets inversées
  (`[::-1]`) puis `strip(b'\x00')` — strip des deux côtés, comme Python.
- game events : certains types contiennent des bitarrays/fourcc exotiques — si un eventid ou
  un champ casse sur un vieux replay du sweep, comparer d'abord avec
  `python -m`-style dump heroprotocol du même fichier avant de toucher au décodeur.
- Réutiliser le clone heroprotocol de `%TEMP%` s'il existe (sinon re-cloner --depth 1).
