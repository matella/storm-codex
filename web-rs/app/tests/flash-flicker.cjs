// Usage: node web-rs/app/tests/flash-flicker.cjs APP_DIST
const assert = require('node:assert/strict');
const { join } = require('node:path');
const { chromium } = require('playwright');
const [dist] = process.argv.slice(2);
assert.ok(dist, 'Usage: node flash-flicker.cjs APP_DIST');
const origin = 'http://localhost:41724';

async function main() {
  const browser = await chromium.launch();
  const page = await browser.newPage({ viewport: { width: 1440, height: 960 } });
  const errors = [];
  try {
    page.on('pageerror', e => errors.push(e.message));
    await page.addInitScript(() => {
      localStorage.setItem('onboarding_done', '1');
      localStorage.setItem('whatsnew_seen', '9999-01-01');
    });
    let connected;
    const socket = new Promise(resolve => { connected = resolve; });
    await page.routeWebSocket('**/ws', connected);
    await page.route('**/*', async route => {
      const url = new URL(route.request().url());
      if (url.origin !== origin) return route.abort();
      if (url.pathname.startsWith('/api/')) {
        if (url.pathname === '/api/standalone/status') return route.fulfill({ status: 404 });
        if (url.pathname === '/api/sessions') return route.fulfill({ json: { sessions: [] } });
        if (url.pathname === '/api/settings') return route.fulfill({ json: { operator_names: [] } });
        return route.fulfill({ json: [], headers: { 'x-total-count': '0' } });
      }
      const asset = url.pathname.startsWith('/v2/app/') ? url.pathname.slice(8) : 'index.html';
      return route.fulfill({ path: join(dist, asset) });
    });
    const now = new Date('2026-01-01T00:00:00Z');
    await page.clock.install({ time: now });
    await page.goto(origin + '/');
    await page.getByRole('heading').first().waitFor({ timeout: 20000 });
    await page.clock.pauseAt(new Date(now.getTime() + 60_000));
    let socketTimeout;
    const ws = await Promise.race([socket, new Promise((_, reject) => {
      socketTimeout = setTimeout(() => reject(new Error('/ws did not connect')), 5000);
    })]).finally(() => clearTimeout(socketTimeout));
    const toast = page.locator('.ds-flash');
    async function parseMatch(map) {
      // The mounted session's refetch acknowledges each event even when the text is identical.
      const refetch = page.waitForRequest(/\/api\/sessions\?/, { timeout: 5000 });
      ws.send(JSON.stringify({ type: 'match.parsed', map }));
      await refetch;
      assert.equal(await toast.textContent(), `● new replay — ${map} added`);
    }

    await parseMatch('Dragon Shire');  // t = 0
    await page.clock.runFor(1000);
    await parseMatch('Cursed Hollow'); // t = 1s
    await page.clock.runFor(2000);
    await parseMatch('Cursed Hollow'); // t = 3s (identical text)
    await page.clock.runFor(3000);     // t = 6s (unrelated expiry)
    assert.equal(await toast.isVisible(), true, 'an unrelated timer cleared the newer toast');
    await page.clock.runFor(1000);     // t = 7s (first Cursed Hollow expiry)
    assert.equal(await toast.isVisible(), true, 'the first identical toast timer cleared the newer toast');
    await page.clock.runFor(1999);
    assert.equal(await toast.isVisible(), true, 'the repeat toast did not get a full 6 seconds');
    await page.clock.runFor(1);        // t = 9s (repeat expiry)
    assert.equal(await toast.count(), 0, 'the repeat toast never cleared');
    assert.deepEqual(errors, []);
    console.log('flash-flicker: distinct and identical replacements each get a full 6 seconds');
  } finally {
    await browser.close().catch(() => {});
  }
}

main().catch(error => { console.error(error); process.exitCode = 1; });
