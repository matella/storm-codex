// Regression for the import-flash flicker: layout.rs scheduled an independent 6s clear timer per
// match.parsed event with no cancellation, so a burst of events during a bulk import (each
// replacing the flash text) let an *earlier* event's timer wipe a *later*, still-fresh message --
// the toast kept disappearing and reappearing, reflowing <main> and scrolling the page. This test
// drives the real /ws channel with a rapid burst and samples flash visibility continuously.
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
    let server;
    await page.routeWebSocket('**/ws', ws => {
      server = ws;
      ws.onMessage(() => {});
    });
    await page.route('**/*', async route => {
      const url = new URL(route.request().url());
      if (url.origin !== origin) return route.abort();
      if (url.pathname.startsWith('/api/')) {
        if (url.pathname === '/api/settings') return route.fulfill({ json: { operator_names: [] } });
        return route.fulfill({ json: [], headers: { 'x-total-count': '0' } });
      }
      const asset = url.pathname.startsWith('/v2/app/') ? url.pathname.slice(8) : 'index.html';
      return route.fulfill({ path: join(dist, asset) });
    });
    await page.goto(origin + '/');
    await page.getByRole('heading').first().waitFor({ timeout: 20000 });
    await page.waitForTimeout(500);
    assert.ok(server, '/ws did not connect');

    // Burst: 4 match.parsed events, 1s apart (well inside the old 6s timer window). Sample flash
    // text right after each send and again at steady intervals through the old stale-timer
    // window so a premature clear from an earlier event's timer is visible either way.
    const maps = ['Cursed Hollow', 'Dragon Shire', 'Towers of Doom', 'Sky Temple'];
    const samples = [];
    for (const map of maps) {
      server.send(JSON.stringify({ type: 'match.parsed', map }));
      for (let i = 0; i < 4; i++) {
        await page.waitForTimeout(250);
        samples.push(await page.locator('.ds-flash').innerText().catch(() => null));
      }
    }
    // Keep sampling past the last send through the old per-event timer window.
    for (let i = 0; i < 20; i++) {
      await page.waitForTimeout(250);
      samples.push(await page.locator('.ds-flash').innerText().catch(() => null));
    }
    // Any sample from right after the 2nd send through steady-state (covers every one of the old
    // per-event 6s timers deriving from the earlier events) must show text, never go empty --
    // that gap is exactly the "disappears and reappears" flicker during a bulk import.
    const duringBurstWindow = samples.slice(4, 28);
    assert.ok(duringBurstWindow.every(s => !!s), `flash went empty mid-burst: ${JSON.stringify(samples)}`);
    // It must still clear eventually (not a permanent toast).
    assert.ok(samples.slice(28).some(s => !s), `flash never cleared: ${JSON.stringify(samples)}`);
    assert.deepEqual(errors, []);

    console.log('flash-flicker: burst of imports shows one continuous toast, clears once at the end');
  } finally {
    await browser.close().catch(() => {});
  }
}

main()
  .then(() => process.exit(0))
  .catch(e => { console.error(e); process.exit(1); });
