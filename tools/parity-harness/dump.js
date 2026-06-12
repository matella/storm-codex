// Étalon de parité du jalon 2 : exécute hots-parser (processReplay) sur un replay et écrit
// {match, players, status} en JSON dans un fichier (pino pollue stdout — pas de pipe possible).
// Usage : node dump.js <replay> <sortie.json>
const fs = require('fs');
const parser = require('hots-parser');

const [file, out] = process.argv.slice(2);
if (!file || !out) {
  console.error('usage : node dump.js <replay> <sortie.json>');
  process.exit(2);
}
const result = parser.processReplay(file, {
  overrideVerifiedBuild: true, // builds > 87774 (notre overlay fait pareil depuis 2026)
  useAttributeName: false,
});
if (result.status !== parser.ReplayStatus.OK) {
  console.error(`statut ${result.status} (${parser.StatusString[result.status]})`);
  process.exit(1);
}
fs.writeFileSync(out, JSON.stringify(result, (k, v) => (v === undefined ? null : v)));
