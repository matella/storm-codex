# Jalon 0 — Spike go/no-go : décodage Rust de `.StormReplay`

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal :** prouver (ou infirmer) qu'un décodage Rust complet de replays HotS réels tient sous
**500 ms/replay**, en le comparant aux moteurs .NET (Heroes.StormReplayParser) et Python
(heroprotocol) sur un corpus de 50 replays locaux — verdict go/no-go documenté.

**Architecture :** code jetable-mais-propre dans `spike/` (le repo est autosuffisant, on est sur
le PC de jeu : 2 652 replays dans `C:\Users\matth\Documents\Heroes of the Storm\...\Replays\Multiplayer`).
Le décodeur Rust n'invente rien : lecteur MPQ = crate `nom-mpq` (celui de s2protocol-rs), décodeur
« versioned » porté depuis `decoders.py` du package pip `heroprotocol` (référence exacte Blizzard),
tables de protocole exportées en JSON depuis les modules `protocolXXXXX.py` installés par pip et
**interprétées à l'exécution** (la génération de code Rust, c'est le jalon 1 — pas le spike).

**Tech stack :** Rust 1.94 (`nom-mpq` 2, `serde_json`, `anyhow` toléré dans le spike), .NET 10
(`Heroes.StormReplayParser` 2.2.1, NuGet), Python 3.13 (`heroprotocol` 2.55.15.96477, pip).

**Critère d'acceptation (spec §Jalons) :** 3 replays réels (builds ≥ 2024) décodés en Rust
(header + details + trackerevents, champs nécessaires présents et exacts vs heroprotocol) **et**
bench 50 replays : décodage complet Rust < 500 ms/replay. Sinon → repli .NET acté dans la spec.

---

## Contexte pour l'exécutant (zéro connaissance du domaine supposée)

- Un `.StormReplay` est une **archive MPQ** (format Blizzard). Elle contient :
  - une **section "user data"** en tête d'archive (hors fichiers) : le *header* du replay,
    qui donne `m_version.m_baseBuild` (numéro de build → choix de la table de protocole) ;
  - des fichiers embarqués, dont `replay.details` (joueurs, carte, héros, vainqueur) et
    `replay.tracker.events` (flux d'événements de stats).
- Tous ces blobs sont encodés au format **"versioned"** de Blizzard : un bytestream taggé
  (struct/blob/array/int zigzag-varint…). La **référence exacte** est le fichier `decoders.py`
  (classes `BitPackedBuffer` + `VersionedDecoder`) du package pip `heroprotocol` — il sera sur
  disque après le Task 1, le localiser avec :
  `python -c "import heroprotocol, pathlib; print(pathlib.Path(heroprotocol.__file__).parent)"`
- Chaque build a un module `versions/protocolXXXXX.py` contenant `typeinfos` (la grammaire des
  types) et des constantes (`replay_header_typeid`, `game_details_typeid`,
  `tracker_eventid_typeid`, `tracker_event_types`…). La fonction `main()` de
  `heroprotocol.py` (même package) montre l'enchaînement canonique
  (header décodé avec le protocole *latest* → `baseBuild` → protocole du build → streams).
- **Règle Context7/doc :** Context7 n'indexe pas ces libs de niche (vérifié le 2026-06-12).
  Les références sont : le code pip local (heroprotocol), https://docs.rs/nom-mpq (API du crate),
  https://github.com/sebosp/s2protocol-rs (exemple d'usage de nom-mpq),
  https://github.com/HeroesToolChest/Heroes.StormReplayParser (API .NET).
- Conventions repo : commits conventionnels ; pas d'`unwrap()` hors tests dans du code durable —
  dans `spike/`, `anyhow` + `?` suffisent (le spike ne sera pas publié).

**Méthodologie bench (identique pour les 3 moteurs, sinon le comparatif ne vaut rien) :**
mono-thread, mesure *in-process* (pas de coût de démarrage de process dans la mesure), décodage
header + details + trackerevents au minimum, 1 replay de warm-up exclu, builds optimisés
(`cargo run --release`, `dotnet run -c Release`), sortie CSV `fichier,ms` + résumé
médiane / p95 / max sur les mêmes 50 fichiers.

---

### Task 0 : corpus de 50 replays + hygiène repo

**Files :**
- Create : `.gitignore`
- Create : `spike/sample_corpus.ps1`
- Output (non commité) : `corpus/spike50/*.StormReplay`, `corpus/spike50/manifest.csv`

**Step 1 — `.gitignore` à la racine :**

```gitignore
corpus/
target/
spike/protocols/
spike/bench-results/
**/bin/
**/obj/
__pycache__/
.venv/
```

**Step 2 — `spike/sample_corpus.ps1` :** échantillon stratifié par année (les builds suivent le
temps ; mtime = proxy de build suffisant pour le spike) : jusqu'à 13 replays/année 2023→2026
tirés au hasard + **les 3 plus récents garantis** (critère « 3 replays réels builds ≥ 2024 »),
complété avec les plus récents si une année est creuse, total exactement 50, copie vers
`corpus/spike50/`, manifeste CSV `name,bytes,mtime`.

```powershell
$src = "$env:USERPROFILE\Documents\Heroes of the Storm\Accounts\*\*\Replays\Multiplayer"
$dst = Join-Path $PSScriptRoot "..\corpus\spike50"
New-Item -ItemType Directory -Force $dst | Out-Null
$all = Get-ChildItem "$src\*.StormReplay" | Sort-Object LastWriteTime
$pick = [System.Collections.Generic.List[object]]::new()
$all | Select-Object -Last 3 | ForEach-Object { $pick.Add($_) }            # 3 plus récents
foreach ($y in 2023..2026) {
  $year = $all | Where-Object { $_.LastWriteTime.Year -eq $y -and $_ -notin $pick }
  $year | Get-Random -Count ([Math]::Min(13, $year.Count)) | ForEach-Object { $pick.Add($_) }
}
$rest = $all | Where-Object { $_ -notin $pick } | Sort-Object LastWriteTime -Descending
$rest | Select-Object -First ([Math]::Max(0, 50 - $pick.Count)) | ForEach-Object { $pick.Add($_) }
$pick = $pick | Select-Object -First 50
$pick | ForEach-Object { Copy-Item $_.FullName $dst }
$pick | Sort-Object LastWriteTime |
  Select-Object @{n='name';e={$_.Name}}, @{n='bytes';e={$_.Length}},
                @{n='mtime';e={$_.LastWriteTime.ToString('s')}} |
  Export-Csv (Join-Path $dst 'manifest.csv') -NoTypeInformation
"$((Get-ChildItem $dst -Filter *.StormReplay).Count) replays copiés"
```

**Step 3 — exécuter et vérifier :**

Run : `pwsh -File spike/sample_corpus.ps1`
Attendu : `50 replays copiés`, manifest.csv listant des mtimes étalés 2023→2026
(vérifier qu'au moins ~4 années distinctes apparaissent ; sinon ajuster et relancer).

**Step 4 — commit :**

```bash
git add .gitignore spike/sample_corpus.ps1
git commit -m "feat(jalon0): script d'échantillonnage du corpus spike (50 replays stratifiés)"
```

---

### Task 1 : baseline Python (heroprotocol)

**Files :**
- Create : `spike/bench_python.py`
- Output (non commité) : `spike/bench-results/python.csv`

**Step 1 — installer :** `python -m pip install --upgrade heroprotocol mpyq`
(heroprotocol dépend de mpyq ; vérifier avec `python -m heroprotocol --header <un replay du corpus>`
que ça décode).

**Step 2 — lire la référence :** ouvrir `heroprotocol.py` et `decoders.py` dans le dossier donné
par `python -c "import heroprotocol, pathlib; print(pathlib.Path(heroprotocol.__file__).parent)"`.
Confirmer l'enchaînement canonique (noms exacts à reprendre tels quels du code lu) :

```python
archive = mpyq.MPQArchive(path)
contents = archive.header['user_data_header']['content']
header = <protocole latest>.decode_replay_header(contents)
base_build = header['m_version']['m_baseBuild']
protocol = <module du build base_build>
details = protocol.decode_replay_details(archive.read_file('replay.details'))
tracker = list(protocol.decode_replay_tracker_events(archive.read_file('replay.tracker.events')))
```

**Step 3 — `spike/bench_python.py` :** pour chaque `.StormReplay` de `corpus/spike50/` (ordre
trié), chrono `time.perf_counter()` autour de l'enchaînement ci-dessus, 1er fichier rejoué en
warm-up (mesure jetée), écrit `spike/bench-results/python.csv` (`name,ms,base_build,events,ok`)
et imprime `médiane / p95 / max ms` + nb d'échecs (un échec n'arrête pas le bench : `ok=0` +
message). p95 = `sorted(ms)[ceil(0.95*n)-1]`.

**Step 4 — exécuter :**

Run : `python spike/bench_python.py`
Attendu : 50 lignes, ~0 échec sur les builds récents, médiane attendue entre ~300 ms et ~2 s
(c'est la baseline lente). Noter les `base_build` distincts du corpus (servira au Task 3).

**Step 5 — commit :**

```bash
git add spike/bench_python.py
git commit -m "feat(jalon0): bench baseline heroprotocol (Python)"
```

---

### Task 2 : baseline .NET (Heroes.StormReplayParser)

**Files :**
- Create : `spike/bench-dotnet/` (projet console)
- Output (non commité) : `spike/bench-results/dotnet.csv`

**Step 1 — créer le projet :**

```bash
dotnet new console -o spike/bench-dotnet
dotnet add spike/bench-dotnet package Heroes.StormReplayParser
```

**Step 2 — `Program.cs` :** mêmes règles que le bench Python. API (vérifiée sur le README du
repo, v2.2.1) :

```csharp
StormReplayResult result = StormReplay.Parse(path); // options par défaut : tracker events inclus
// result.Status == StormReplayParseStatus.Success ; result.Replay.StormPlayers ;
// result.Replay.ReplayVersion (build)
```

Stopwatch par fichier, warm-up exclu, CSV `name,ms,build,ok` vers
`spike/bench-results/dotnet.csv`, résumé médiane/p95/max. Échec = `ok=0`, on continue.

**Step 3 — exécuter :**

Run : `dotnet run -c Release --project spike/bench-dotnet`
Attendu : médiane de l'ordre de 50–300 ms/replay. C'est la **cible à battre** et le repli si no-go.

**Step 4 — commit :**

```bash
git add spike/bench-dotnet
git commit -m "feat(jalon0): bench baseline Heroes.StormReplayParser (.NET)"
```

---

### Task 3 : export des tables de protocole en JSON

**Files :**
- Create : `spike/export_protocols.py`
- Output (non commité) : `spike/protocols/<base_build>.json` + `spike/protocols/latest.txt`

**Step 1 — `spike/export_protocols.py` :** énumérer les modules `protocol*.py` du dossier
`versions/` du package heroprotocol installé, les importer un à un, et sérialiser pour chacun
un JSON `spike/protocols/{base_build}.json` :

```json
{
  "base_build": 92264,
  "typeinfos": [["_int", [[0, 7]]], ["_struct", [[["m_flags", 12, 0]]]], ...],
  "replay_header_typeid": ..., "game_details_typeid": ..., "svaruint32_typeid": ...,
  "replay_userid_typeid": ..., "tracker_eventid_typeid": ...,
  "tracker_event_types": {"0": [..., "NNet.Replay.Tracker.SPlayerStatsEvent"], ...}
}
```

`typeinfos` et les constantes existent sous ces noms dans chaque module (vérifier sur un
module ouvert dans l'éditeur ; `json.dumps` convertit les tuples en listes tout seul). Écrire
aussi `latest.txt` contenant le plus grand base_build exporté (= protocole « latest » pour
décoder le header). Les anciens protocoles n'ont pas de tracker events (< 2014) : si un attribut
manque, l'omettre du JSON plutôt qu'échouer.

**Step 2 — exécuter :**

Run : `python spike/export_protocols.py`
Attendu : « N protocoles exportés » (N ≈ 80–120), et les `base_build` relevés au Task 1
tous présents dans `spike/protocols/`.

**Step 3 — commit :**

```bash
git add spike/export_protocols.py
git commit -m "feat(jalon0): export JSON des typeinfos heroprotocol par build"
```

---

### Task 4 : spike Rust `storm-decode` — MPQ + header

**Files :**
- Create : `spike/storm-decode/` (binaire cargo)
- Test : `spike/storm-decode/tests/decode.rs` (chemins corpus via env `SPIKE_CORPUS`)

**Step 1 — créer le crate :**

```bash
cargo new spike/storm-decode --name storm-decode
cargo add -p storm-decode nom-mpq serde_json anyhow
```

(Si le workspace racine n'existe pas, le crate vit seul — ne pas créer de workspace pour le spike.)

**Step 2 — lire les références avant d'écrire :** (a) `decoders.py` du package pip —
`BitPackedBuffer` (lecture de bits big-endian) et `VersionedDecoder` (un tag-byte par nœud :
struct / blob / array / choice / optional / bool / fourcc / bitarray / real / **vint zigzag**) ;
recopier la sémantique méthode par méthode, y compris `_skip_instance`. (b) docs.rs/nom-mpq +
le repo s2protocol-rs pour l'API exacte : parser l'archive depuis les bytes, lire la **user data
section** (header) et `read_mpq_file_sector("replay.details", …)` pour les fichiers embarqués.

**Step 3 — module `versioned.rs` (test d'abord) :** type `Value` (Int(i64), Blob(Vec<u8>),
Bool, Array(Vec<Value>), Struct(Vec<(String, Value)>), Optional(Option<Box<Value>>), Fourcc,
BitArray, Null) + `VersionedDecoder::instance(typeinfos_json, typeid) -> Result<Value>`.
Écrire d'abord `tests/decode.rs::header_smoke` : ouvrir le replay le plus récent de
`$env:SPIKE_CORPUS` (= `corpus/spike50`), décoder le header avec le protocole `latest.txt`,
asserter `m_signature == "Heroes of the Storm replay 11"` et `m_version.m_baseBuild > 70000`.

Run : `cargo test -p storm-decode` → Attendu : FAIL (compile error puis assertions) tant que
versioned.rs + mpq.rs ne sont pas finis ; itérer jusqu'à PASS.

**Step 4 — binaire :** `storm-decode <replay>` imprime un JSON résumé :
`{base_build, signature, elapsed_game_loops}` (header seul à ce stade).

Run : `cargo run --release -p storm-decode -- <replay récent>` → Attendu : JSON cohérent avec
`python -m heroprotocol --header <même replay>`.

**Step 5 — commit :**

```bash
git add spike/storm-decode
git commit -m "feat(jalon0): storm-decode — MPQ + décodeur versioned + header"
```

---

### Task 5 : spike Rust — details + tracker events

**Files :**
- Modify : `spike/storm-decode/src/main.rs` (+ `details.rs`, `tracker.rs`)
- Test : `spike/storm-decode/tests/decode.rs` (cas supplémentaires)

**Step 1 — details (test d'abord) :** test `details_smoke` : décoder `replay.details` du même
replay avec le protocole du `base_build` lu dans le header (`game_details_typeid`), asserter
10 joueurs, chacun avec `m_name` et `m_hero` non vides, `m_title` (carte) non vide.
Implémenter : lecture du fichier embarqué + `instance(...)` + extraction typée des champs.

**Step 2 — tracker events (test d'abord) :** test `tracker_smoke` : décoder
`replay.tracker.events`, asserter > 1 000 événements et qu'au moins un `SPlayerStatsEvent` et
un `SScoreResultEvent` apparaissent. Implémenter le décodage du flux en portant
`decode_replay_tracker_events` / `_decode_event_stream` du protocole Python lu au Task 4
(boucle : delta `svaruint32` → eventid (`tracker_eventid_typeid`) → struct de
`tracker_event_types[eventid]` ; **pas de userid pour les tracker events** — vérifier ce détail
dans le code Python, ne pas le déduire).

**Step 3 — sortie complète + cross-check :** `storm-decode <replay>` imprime désormais
`{base_build, map, players:[{name,hero,result}], tracker_events: N, elapsed_game_loops}`.
Créer `spike/crosscheck.py` : pour les **3 replays les plus récents** du corpus, comparer cette
sortie aux mêmes champs extraits via heroprotocol (subprocess `--json` ou import) ; imprimer
`OK`/diff par champ.

Run : `python spike/crosscheck.py` → Attendu : `3/3 OK` (champs identiques). C'est la moitié
« champs nécessaires présents » du critère d'acceptation.

**Step 4 — commit :**

```bash
git add spike/storm-decode spike/crosscheck.py
git commit -m "feat(jalon0): storm-decode — details + tracker events + cross-check heroprotocol"
```

---

### Task 6 : bench Rust 50 replays + rapport go/no-go

**Files :**
- Modify : `spike/storm-decode/src/main.rs` (mode `--bench <dir>`)
- Create : `docs/research/2026-06-12-jalon0-bench.md`
- Modify : `docs/STATUS.md` (et `docs/specs/2026-06-12-storm-codex-design.md` **seulement si no-go**)

**Step 1 — mode bench :** `storm-decode --bench corpus/spike50` — mêmes règles que les deux
autres moteurs (mono-thread, warm-up exclu, header+details+tracker, échec → `ok=0` et on
continue), CSV `spike/bench-results/rust.csv`, résumé médiane/p95/max.

Run : `cargo run --release -p storm-decode -- --bench corpus/spike50`

**Step 2 — rapport `docs/research/2026-06-12-jalon0-bench.md` :** machine (CPU), versions des
3 toolchains, tableau médiane/p95/max/échecs par moteur sur les 50 mêmes fichiers, couverture
builds du corpus, résultat du cross-check 3 replays, **verdict explicite** :
- médiane et p95 Rust < 500 ms + cross-check 3/3 → **GO jalon 1** ;
- sinon → **NO-GO** : acter le repli .NET dans la spec (§Décisions et §Risques), mêmes jalons.

**Step 3 — mettre à jour `docs/STATUS.md` :** section « Où on en est » (spike fait, verdict,
lien rapport), « Prochaine étape » (jalon 1 storm-replay si go — plan à écrire ; sinon jalon 1
en .NET), retirer le bloquant « échantillon de replays » (résolu : on est sur le PC de jeu).

**Step 4 — commit :**

```bash
git add docs/research/2026-06-12-jalon0-bench.md docs/STATUS.md spike/storm-decode
git commit -m "feat(jalon0): bench 3 moteurs + rapport go/no-go"
```

---

## Pièges connus (à relire avant Task 4–5)

- **BitPackedBuffer lit les bits en big-endian** et le `vint` versioned est un **zigzag varint
  par octets entiers** (bit 7 = continuation, bit 6 du 1er octet = signe) — ne pas confondre les
  deux chemins ; tout est dans `decoders.py`, le porter, ne pas le réinventer.
- Les structs versioned sont **taggés par champ** (tag = 3e élément des entrées typeinfos) et
  tolèrent les champs inconnus (`_skip_instance`) — indispensable pour la compat multi-builds.
- `replay.details` ↔ protocole du **base_build du header**, pas le latest ; le header, lui,
  se décode avec le **latest** (comportement heroprotocol, y compris le fallback builds inconnus).
- Certains très vieux replays (< 2015, builds alpha) peuvent échouer partout : un échec commun
  aux 3 moteurs n'est pas un no-go Rust, c'est une donnée du rapport.
- mpyq/nom-mpq : les fichiers embarqués sont compressés par secteurs — laisser la lib gérer ;
  ne jamais réimplémenter la décompression.
