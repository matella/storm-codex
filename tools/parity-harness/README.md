# Stats parity harness (storm-stats vs hots-parser)

Diffs `storm-stats-dump` (Rust) field by field against hots-parser 7.55.7 (Node, the reference)
on a replay corpus. Required evidence for any change to `crates/storm-stats/src/process/`
(see `docs/spec/03-storm-stats.md` and `docs/spec/10-developpement.md`). Not run in CI: the
corpus is private.

- `dump.js`: hots-parser reference dump of one replay.
- `diff.py`: runs both sides on every `*.StormReplay` of the corpus and compares them; the Node
  dumps are cached in `<corpus>/.ref/`.
- `tolerances.json`: the accepted, documented divergences.

## Run

```sh
cd tools/parity-harness
npm ci                      # needs git and network access to github.com (see below)
cd ../..
cargo build --release -p storm-stats --bin storm-stats-dump
python3 tools/parity-harness/diff.py --corpus /abs/path/to/corpus
```

`npm ci` must run install scripts: heroprotocol's `postinstall` generates the protocol tables
(`lib/protocol*.js`) that hots-parser loads; with `--ignore-scripts`, `require('hots-parser')`
fails with `Cannot find module './lib/protocol29406'`. If you install with `--ignore-scripts`,
run `npm rebuild heroprotocol` afterwards.

## Pinned heroprotocol source

heroprotocol's `postinstall` used `download-github-repo`, which downloads
`Blizzard/heroprotocol/archive/master.zip` and extracts it with `decompress@4` through `got@7`
(path-traversal and redirect advisories with no fixed `decompress` release). `package.json`
overrides it with `vendor/download-github-repo`, a drop-in that fetches `Blizzard/heroprotocol`
at a pinned commit with `git` (no archive extraction, no extra dependencies). To pick up new
game builds, bump the commit in `vendor/download-github-repo/index.js` and reinstall.
