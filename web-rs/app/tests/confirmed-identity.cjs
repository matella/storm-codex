// Regression for the standalone confirmed-identity toggle
// (docs/specs/2026-11-21-standalone-confirmed-identity.md): the Welcome page's
// "Add to My Identity" / "Remove" button, its persistence across a reload, and the
// admin-gating revert on a non-2xx response.
// Usage: node web-rs/app/tests/confirmed-identity.cjs APP_DIST
const assert = require('node:assert/strict');
const { join } = require('node:path');
const { chromium } = require('playwright');
const [dist] = process.argv.slice(2);
assert.ok(dist, 'Usage: node confirmed-identity.cjs APP_DIST');
const origin = 'http://localhost:41727';

const TOON = '2-Hero-1-10';
const FOLDER = 'C:\\\\Users\\\\me\\\\Documents\\\\Heroes of the Storm\\\\Accounts\\\\1\\\\' + TOON + '\\\\Replays\\\\Multiplayer';

function statusJson() {
  return {
    phase: 'ready', folders: [FOLDER], extraFolders: [], total: 1, done: 1, duplicates: 0,
    rejected: [], failed: [], freeBytes: null, neededBytes: null, dataDir: 'C:\\data', version: '1',
  };
}

async function open(page, fixture) {
  page.setDefaultTimeout(10000);
  await page.addInitScript(() => {
    localStorage.setItem('onboarding_done', '1');
    localStorage.setItem('whatsnew_seen', '9999-01-01');
  });
  await page.routeWebSocket('**/ws', () => {});
  await page.route('**/*', async route => {
    const request = route.request();
    const url = new URL(request.url());
    if (url.origin !== origin) return route.abort();
    if (url.pathname === '/api/standalone/status') return route.fulfill({ json: statusJson() });
    if (url.pathname === '/api/standalone/identity') {
      return route.fulfill({
        json: [{ folder: FOLDER, toon: TOON, name: 'Player', aliases: ['Player'], confirmed: fixture.confirmed }],
      });
    }
    if (url.pathname === '/api/settings') return route.fulfill({ json: { operator_names: [] } });
    if (url.pathname === '/api/admin/identity/confirm' && request.method() === 'POST') {
      fixture.calls.push(['confirm', request.headers()['authorization'] || null]);
      if (fixture.rejectNext) {
        fixture.rejectNext = false;
        return route.fulfill({ status: 401, json: { error: 'admin token requis' } });
      }
      fixture.confirmed = true;
      return route.fulfill({ status: 200, json: {} });
    }
    if (url.pathname === `/api/admin/identity/confirm/${TOON}` && request.method() === 'DELETE') {
      fixture.calls.push(['unconfirm', request.headers()['authorization'] || null]);
      fixture.confirmed = false;
      return route.fulfill({ status: 200, json: {} });
    }
    if (url.pathname.startsWith('/api/')) return route.fulfill({ json: [], headers: { 'x-total-count': '0' } });
    const asset = url.pathname.startsWith('/v2/app/') ? url.pathname.slice(8) : 'index.html';
    return route.fulfill({ path: join(dist, asset) });
  });
  await page.goto(origin + '/welcome');
  await page.getByRole('heading').first().waitFor();
}

async function main() {
  const browser = await chromium.launch();
  try {
    const page = await browser.newPage();
    const errors = [];
    page.on('pageerror', error => errors.push(error.message));
    const fixture = { confirmed: false, calls: [], rejectNext: false };
    await open(page, fixture);

    const add = page.getByRole('button', { name: 'Add to My Identity', exact: true });
    const remove = page.getByRole('button', { name: 'Remove', exact: true });
    const added = page.getByText('Added to My Identity ✓', { exact: true });

    await add.waitFor();
    assert.equal(await added.count(), 0, 'not confirmed yet');
    await add.click();
    await added.waitFor();
    await remove.waitFor();
    assert.deepEqual(fixture.calls, [['confirm', null]]);

    // persists across a reload
    await page.reload();
    await page.getByRole('heading').first().waitFor();
    await added.waitFor();
    await remove.waitFor();

    // unconfirm removes it
    await remove.click();
    await add.waitFor();
    assert.equal(await added.count(), 0);
    assert.deepEqual(fixture.calls, [['confirm', null], ['unconfirm', null]]);

    // admin-gating: a 401 reverts the optimistic toggle and shows an inline error
    fixture.rejectNext = true;
    await add.click();
    await page.getByText('Could not save (HTTP 401).', { exact: true }).waitFor();
    await add.waitFor();
    assert.equal(await added.count(), 0, 'reverted after the 401');
    assert.equal(fixture.confirmed, false, 'the server never recorded the rejected confirm');

    assert.deepEqual(errors, []);
    console.log('confirmed-identity: OK');
  } finally {
    await Promise.race([browser.close().catch(() => {}), new Promise(r => setTimeout(r, 3000))]);
  }
}

main().then(() => process.exit(0)).catch(err => {
  console.error(err);
  process.exit(1);
});
