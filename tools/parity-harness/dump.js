// Milestone 2 parity reference: runs hots-parser (processReplay) on a replay and writes
// {match, players, status} as JSON to a file (pino pollutes stdout — no pipe possible).
// Usage: node dump.js <replay> <output.json>
const fs = require('fs');
const parser = require('hots-parser');

const [file, out] = process.argv.slice(2);
if (!file || !out) {
  console.error('usage: node dump.js <replay> <output.json>');
  process.exit(2);
}
const result = parser.processReplay(file, {
  overrideVerifiedBuild: true, // builds > 87774 (our overlay has done the same since 2026)
  useAttributeName: false,
});
if (result.status !== parser.ReplayStatus.OK) {
  console.error(`statut ${result.status} (${parser.StatusString[result.status]})`);
  process.exit(1);
}
fs.writeFileSync(out, JSON.stringify(result, (k, v) => (v === undefined ? null : v)));
