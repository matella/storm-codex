// Build-time only; uses built-in Node codecs (no npm dependencies).
import { readdirSync, readFileSync, writeFileSync } from 'node:fs';
import { extname, join } from 'node:path';
import { brotliCompressSync, constants, gzipSync } from 'node:zlib';

const dist = process.argv[2] ?? 'web-rs/dist';
for (const bundle of ['app', 'overlay', 'sources']) {
  const dir = join(dist, bundle);
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    if (!entry.isFile() || !['.css', '.js', '.wasm'].includes(extname(entry.name))) continue;
    const path = join(dir, entry.name);
    const bytes = readFileSync(path);
    writeFileSync(`${path}.br`, brotliCompressSync(bytes, {
      params: { [constants.BROTLI_PARAM_QUALITY]: constants.BROTLI_MAX_QUALITY },
    }));
    writeFileSync(`${path}.gz`, gzipSync(bytes, { level: constants.Z_BEST_COMPRESSION }));
  }
}
