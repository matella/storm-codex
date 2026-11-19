'use strict';
// Replaces download-github-repo for heroprotocol's postinstall (see ../../README.md).
// The upstream package downloads <repo>/archive/master.zip and extracts it with
// decompress@4 (unpatched path-traversal advisories) through got@7. This shim checks out
// the same repository at a pinned commit with git, into `dest` (like `strip: 1`).
const { execFileSync } = require('child_process');
const fs = require('fs');

const PINNED = {
  'Blizzard/heroprotocol': '9af3ea7150f1a8acb53464c92519a9bbcc7a3594', // 2.55.15.96477
};

module.exports = function download(repo, dest, fn) {
  const sha = PINNED[repo];
  if (!sha) {
    fn(new Error(`download-github-repo shim: no pinned commit for ${repo}`));
    return;
  }
  try {
    fs.mkdirSync(dest, { recursive: true });
    const git = (...args) => execFileSync('git', ['-C', dest, ...args], { stdio: 'inherit' });
    git('init', '--quiet');
    git('fetch', '--quiet', '--depth', '1', `https://github.com/${repo}.git`, sha);
    git('checkout', '--quiet', '--force', '--detach', sha); // overwrites the fork's bundled snapshot
    fn();
  } catch (err) {
    fn(err);
  }
};
