// Regressions for two release-build failures:
// 1. Same-origin links outside the SPA's route space (`/api/...`) were taken over as in-app
//    navigations by the view-transition click interceptor (layout.rs): the "CSV ↓" export changed
//    the URL to /api/matches.csv but never downloaded anything and blanked the page.
// 2. Async work finishing after its page was left read a disposed signal or ran a disposed
//    callback: in Leptos 0.8 that panics, and the release build aborts the whole WASM module, so
//    no navigation works afterwards. Each scenario starts the work, leaves the page before it
//    completes, lets it complete, then checks that router navigation still works.
// Every scenario runs in its own page and reports PASS/FAIL, so a baseline build shows each failure.
// Usage: NODE_PATH=<playwright>/node_modules node web-rs/app/tests/navigate-away.cjs APP_DIST
const assert = require('node:assert/strict');
const { join } = require('node:path');
const { chromium } = require('playwright');
const [dist] = process.argv.slice(2);
assert.ok(dist, 'Usage: node navigate-away.cjs APP_DIST');
const origin = 'http://localhost:41731';
const sleep = ms => new Promise(r => setTimeout(r, ms));

const TEAM = { name: '', players: [] };
const CAST = {
  active: false, blue: TEAM, red: TEAM, bo: 3, format: '', score_override: null, pool: [], games: [],
  scene: 'draft', slide: { game: null, index: 0, auto: false, interval_ms: 0, since_ms: 0 },
};
const DRAFT = {
  format: '', first_pick: 'blue', map: '', blue: TEAM, red: TEAM, score: [0, 0], bo: 3, steps: [], cursor: 0,
  assignments: [],
};

const STATUS = {
  phase: 'ready', folders: [], extraFolders: [], total: 1, done: 1, duplicates: 0,
  rejected: [], failed: [], freeBytes: null, neededBytes: null, dataDir: 'C:\\data', version: '1',
};

// `api(request, url)` returns a fulfill option object, a promise of one, or undefined (generic fallback).
async function open(browser, path, api = () => undefined, ready = 'h1') {
  const page = await browser.newPage({ viewport: { width: 1440, height: 960 } });
  page.setDefaultTimeout(10000);
  const problems = [];
  page.on('pageerror', err => problems.push(`pageerror: ${err.message}`));
  page.on('console', msg => {
    if (/panic|RuntimeError|unreachable/i.test(msg.text())) problems.push(`console: ${msg.text()}`);
  });
  const requests = [];
  page.on('request', req => requests.push(new URL(req.url()).pathname));
  await page.addInitScript(() => {
    localStorage.setItem('onboarding_done', '1');
    localStorage.setItem('whatsnew_seen', '9999-01-01');
    sessionStorage.setItem('standalone_welcome_decided', '1');
  });
  await page.routeWebSocket('**/ws', () => {});
  await page.route('**/*', async route => {
    const request = route.request();
    const url = new URL(request.url());
    if (url.origin !== origin) return route.abort();
    if (url.pathname.startsWith('/api/')) {
      const custom = await api(request, url);
      if (custom) return route.fulfill(custom);
      if (url.pathname === '/api/settings') return route.fulfill({ json: { operator_names: [] } });
      return route.fulfill({ json: [], headers: { 'x-total-count': '0' } });
    }
    const asset = url.pathname.startsWith('/v2/app/') ? url.pathname.slice(8) : 'index.html';
    return route.fulfill({ path: join(dist, asset) });
  });
  await page.goto(origin + path);
  await page.locator(ready).first().waitFor({ timeout: 20000 });
  return { page, problems, requests };
}

// Leaves the current page through a router link, without waiting for anything.
async function leave(page) {
  await page.locator('a.ds-brand').click({ timeout: 2000 });
}

// The app is still alive: two more client-side navigations through the grouped nav render their pages.
async function assertNavigationWorks(page, problems, base = '') {
  try {
    for (const [tab, heading] of [['heroes', 'HERO POOL'], ['records', 'THE BEST SINGLE GAMES']]) {
      await page.locator('nav.ds-topnav .ds-navgroup button').filter({ hasText: 'Stats' }).first().click({ timeout: 3000 });
      await page.locator(`nav.ds-topnav .ds-navmenu a[href="${base}/${tab}"]`).first().click({ timeout: 3000 });
      await page.waitForFunction(p => location.pathname === p, `${base}/${tab}`, { timeout: 3000 });
      await page.locator('h1').filter({ hasText: heading }).first().waitFor({ timeout: 3000 });
    }
  } catch (e) {
    throw new Error(`navigation broken (${e.message.split('\n')[0]}); problems: ${JSON.stringify(problems)}`);
  }
  assert.deepEqual(problems, [], `WASM panic/abort: ${JSON.stringify(problems)}`);
}

const scenarios = {};
// The export links are absolute `/api/...` URLs under both app bases (served at `/` and at `/v2/`).
for (const base of ['', '/v2']) {
  const at = base || '/';
  // H1: the CSV link really downloads, and the page stays on /matches.
  scenarios[`matches (${at}): CSV export link downloads a file`] = async browser => {
    const { page, problems } = await open(browser, `${base}/matches`, (_req, url) => {
      if (url.pathname === '/api/matches.csv') {
        return { body: 'id,map\n1,Cursed Hollow\n', headers: { 'content-type': 'text/csv', 'content-disposition': 'attachment; filename="matches.csv"' } };
      }
    });
    try {
      const download = page.waitForEvent('download', { timeout: 5000 });
      await page.locator('a.ds-btn').filter({ hasText: 'CSV' }).click();
      const file = await download;
      assert.equal(file.suggestedFilename(), 'matches.csv');
      assert.equal(new URL(page.url()).pathname, `${base}/matches`);
      await page.locator('h1').filter({ hasText: 'Matches' }).first().waitFor({ timeout: 3000 });
      await assertNavigationWorks(page, problems, base);
    } finally {
      await page.close();
    }
  };

  // H1, the class: any same-origin /api/ link (even without `download`) is left to the browser.
  scenarios[`layout (${at}): a bare /api/ link is not taken over by the SPA`] = async browser => {
    const { page, problems, requests } = await open(browser, `${base}/matches`, (_req, url) => {
      if (url.pathname === '/api/probe.csv') {
        return { body: 'a\n1\n', headers: { 'content-type': 'text/csv', 'content-disposition': 'attachment; filename="probe.csv"' } };
      }
    });
    try {
      await page.evaluate(() => {
        const a = document.createElement('a');
        a.id = 'api-probe';
        a.href = '/api/probe.csv?x=1';
        a.textContent = 'probe';
        document.querySelector('main, .ds-page').prepend(a);
      });
      const download = page.waitForEvent('download', { timeout: 5000 });
      await page.locator('#api-probe').click();
      await download;
      assert.ok(requests.includes('/api/probe.csv'), 'the browser requested the API URL');
      assert.equal(new URL(page.url()).pathname, `${base}/matches`);
      await assertNavigationWorks(page, problems, base);
    } finally {
      await page.close();
    }
  };
}

Object.assign(scenarios, {

  // H2: Players debounce timer (250 ms) firing after the page was left.
  async 'players: leaving during the search debounce'(browser) {
    const { page, problems } = await open(browser, '/players');
    try {
      await page.locator('input[placeholder="Search a player…"]').fill('matella');
      await leave(page);
      await sleep(600);
      await assertNavigationWorks(page, problems);
    } finally {
      await page.close();
    }
  },

  // H2: scouting replay upload completing after the report page was left.
  async 'scouting report: leaving during a replay upload'(browser) {
    let uploads = 0;
    const { page, problems } = await open(browser, '/scouting/1', async (req, url) => {
      if (url.pathname === '/api/scouting/1' && req.method() === 'GET') {
        return { json: { id: 1, title: 'Rivals', target_name: 'Rivals', created_at: '2026-11-01', updated_at: '2026-11-01', status: 'empty', games: [] } };
      }
      if (url.pathname === '/api/scouting/1/replays' && req.method() === 'POST') {
        uploads += 1;
        await sleep(1200);
        return { json: { status: 'added', target_team: 0 } };
      }
    });
    try {
      await page.locator('#games input[type=file]').setInputFiles([
        { name: 'a.StormReplay', mimeType: 'application/octet-stream', buffer: Buffer.from('MPQ1') },
        { name: 'b.StormReplay', mimeType: 'application/octet-stream', buffer: Buffer.from('MPQ2') },
      ]);
      await page.waitForFunction(() => document.querySelector('.ds-queue')?.textContent.includes('uploading'));
      await leave(page);
      await sleep(3000); // both uploads finish (sequentially) after the page is gone
      assert.equal(uploads, 2, 'the upload loop keeps sending the files the user dropped');
      await assertNavigationWorks(page, problems);
    } finally {
      await page.close();
    }
  },

  // H2: Welcome "Save" (manual names) answering after the page was left.
  async 'welcome: leaving during a manual-names save'(browser) {
    const { page, problems } = await open(browser, '/welcome', async (req, url) => {
      if (url.pathname === '/api/standalone/status') return { json: STATUS };
      if (url.pathname === '/api/standalone/identity') return { json: [] };
      if (url.pathname === '/api/admin/settings' && req.method() === 'PUT') {
        await sleep(800);
        return { json: {} };
      }
    });
    try {
      await page.locator('input[placeholder="Name, other account"]').fill('matella');
      await page.getByRole('button', { name: 'Save', exact: true }).click();
      await leave(page);
      await sleep(1500);
      await assertNavigationWorks(page, problems);
    } finally {
      await page.close();
    }
  },

  // Same class, synchronous: the cast draft "Replay" panel's Space/Backspace window listener outlived the panel
  // and read its disposed signals on the next key press after the page was left.
  async 'cast: a key press after leaving the draft replay panel'(browser) {
    const { page, problems } = await open(browser, '/cast?scene=draft', (_req, url) => {
      if (url.pathname === '/api/cast') return { json: CAST };
      if (url.pathname === '/api/draft') return { json: DRAFT };
      if (url.pathname === '/api/draft/replay') return { json: { queue: [], current: 0 } };
    }, '.cr-subtabs');
    try {
      await page.locator('.cr-subtabs button').filter({ hasText: 'Replay' }).click();
      await page.locator('.drp').first().waitFor();
      await leave(page);
      await page.locator('h1').first().waitFor();
      await sleep(500);
      await page.locator('body').press('Space');
      await sleep(300);
      await assertNavigationWorks(page, problems);
    } finally {
      await page.close();
    }
  },
});

async function main() {
  const browser = await chromium.launch();
  const failed = [];
  try {
    for (const [name, run] of Object.entries(scenarios)) {
      try {
        await run(browser);
        console.log(`PASS ${name}`);
      } catch (e) {
        failed.push(name);
        console.log(`FAIL ${name}\n  ${String(e.message || e).split('\n').slice(0, 6).join('\n  ')}`);
      }
    }
  } finally {
    await Promise.race([browser.close().catch(() => {}), sleep(3000)]);
  }
  console.log(failed.length ? `navigate-away: ${failed.length} scenario(s) failed` : 'navigate-away: all scenarios passed');
  process.exit(failed.length ? 1 : 0);
}

main().catch(e => { console.error(e); process.exit(1); });
