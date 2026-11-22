// Regression for the butler brief full-fledged expansion
// (docs/specs/2026-11-21-butler-brief-full-fledged.md): the `/queue` OBS source renders the
// French line by default, the English line once `brief_voice` is switched to `en`, and no line
// at all when it is `off`. API responses are mocked (same pattern as
// web-rs/app/tests/confirmed-identity.cjs); no real server/DB involved.
// Usage: node web-rs/sources/tests/butler-brief.cjs SOURCES_DIST
const assert = require('node:assert/strict');
const { join } = require('node:path');
const { chromium } = require('playwright');
const [dist] = process.argv.slice(2);
assert.ok(dist, 'Usage: node butler-brief.cjs SOURCES_DIST');
const origin = 'http://localhost:41729';

// One MVP win today: top priority bucket, deterministic ("MVP" appears in both the Fr and En
// phrase banks), so a single fixture exercises the full `/queue` -> jarvis_phrase wiring.
function matches() {
  const now = new Date().toISOString();
  return [{
    id: 1,
    map: 'Cursed Hollow',
    mode: 50091,
    played_at: now,
    length: 600,
    winner: 0,
    players: [
      { toon: 'me-1', name: 'matella', hero: 'Valla', team: 0, win: true, kills: 10, deaths: 0, takedowns: 20, award: 'EndOfMatchAwardMVPBoolean' },
      { toon: 'foe-1', name: 'foe', hero: 'Zeratul', team: 1, win: false, kills: 2, deaths: 10, takedowns: 4, award: null },
    ],
    perspective_toon: null,
  }];
}

async function open(page, voice) {
  page.setDefaultTimeout(10000);
  await page.routeWebSocket('**/ws', () => {});
  await page.route('**/*', async route => {
    const request = route.request();
    const url = new URL(request.url());
    if (url.origin !== origin) return route.abort();
    if (url.pathname === '/api/settings') {
      return route.fulfill({ json: { operator_names: ['matella'], brief_voice: voice, admin_open: true } });
    }
    if (url.pathname === '/api/matches') return route.fulfill({ json: matches() });
    if (url.pathname === '/api/dim/heroes') return route.fulfill({ json: {} });
    if (url.pathname.startsWith('/api/')) return route.fulfill({ json: [], headers: { 'x-total-count': '0' } });
    const asset = url.pathname.startsWith('/v2/sources/') ? url.pathname.slice(12) : 'index.html';
    return route.fulfill({ path: join(dist, asset) });
  });
  await page.goto(origin + '/queue');
}

async function main() {
  const browser = await chromium.launch();
  try {
    const errors = [];

    // Fr (default)
    {
      const page = await browser.newPage();
      page.on('pageerror', e => errors.push(String(e)));
      await open(page, 'fr');
      const line = page.locator('text=/« .* » — Jarvis/');
      await line.waitFor();
      const text = await line.innerText();
      assert.match(text, /MVP/, 'MVP is the top-priority bucket in both phrase banks');
      assert.match(text, /monsieur|soirée|réglé/i, 'Fr line reads as French');
      await page.close();
    }

    // En (switched)
    {
      const page = await browser.newPage();
      page.on('pageerror', e => errors.push(String(e)));
      await open(page, 'en');
      const line = page.locator('text=/« .* » — Jarvis/');
      await line.waitFor();
      const text = await line.innerText();
      assert.match(text, /MVP/);
      assert.doesNotMatch(text, /monsieur/i, 'En line must not leak French words');
      await page.close();
    }

    // Off (no line at all)
    {
      const page = await browser.newPage();
      page.on('pageerror', e => errors.push(String(e)));
      await open(page, 'off');
      // the session panel itself (record, recent games) must still render
      await page.getByText("TODAY'S SESSION").waitFor();
      await page.waitForTimeout(300); // settle the memo before asserting absence
      assert.equal(await page.locator('text=/— Jarvis/').count(), 0, 'Off must omit the line entirely');
      await page.close();
    }

    assert.deepEqual(errors, []);
    console.log('butler-brief: OK (Fr default, En switch, Off omits the line)');
  } finally {
    await browser.close();
  }
}

main().then(() => process.exit(0)).catch(err => {
  console.error(err);
  process.exit(1);
});
