import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { test } from 'node:test';
import { fileURLToPath } from 'node:url';
import { brotliDecompressSync, gunzipSync } from 'node:zlib';

const compressor = new URL('./precompress-web.mjs', import.meta.url);

test('all three bundles keep originals and gain valid Brotli and gzip siblings', () => {
  const dist = mkdtempSync(join(tmpdir(), 'storm-precompress-'));
  try {
    const original = Buffer.from('bundle fixture\n'.repeat(500));
    const assets = ['bundle-deadbeef.js', 'bundle-deadbeef_bg.wasm', 'theme-deadbeef.css'];
    for (const bundle of ['app', 'overlay', 'sources']) {
      mkdirSync(join(dist, bundle));
      for (const name of [...assets, 'index.html', 'font.woff2']) {
        writeFileSync(join(dist, bundle, name), original);
      }
    }
    const build = () => execFileSync(process.execPath, [fileURLToPath(compressor), dist]);
    build();
    build(); // Idempotent: siblings aren't themselves compressed.
    for (const bundle of ['app', 'overlay', 'sources']) {
      for (const name of assets) {
        const file = join(dist, bundle, name);
        assert.deepEqual(readFileSync(file), original);
        assert.deepEqual(brotliDecompressSync(readFileSync(`${file}.br`)), original);
        assert.deepEqual(gunzipSync(readFileSync(`${file}.gz`)), original);
        assert(!existsSync(`${file}.br.br`));
      }
      assert(!existsSync(join(dist, bundle, 'index.html.br')));
      assert(!existsSync(join(dist, bundle, 'font.woff2.gz')));
    }
  } finally {
    rmSync(dist, { recursive: true });
  }
});
