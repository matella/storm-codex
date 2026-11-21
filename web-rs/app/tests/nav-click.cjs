// Regression for the view-transition click interceptor in layout.rs: a raw capture-phase DOM
// listener calling use_navigate() *inside* the handler panics (no live Leptos Owner at click time),
// silently swallowing the click after prevent_default() -- the URL never changes. Run with
// NODE_PATH and PLAYWRIGHT_BROWSERS_PATH pointing to an isolated Playwright installation.
// Usage: node web-rs/app/tests/nav-click.cjs APP_DIST
const assert = require('node:assert/strict');
const { join } = require('node:path');
const { chromium } = require('playwright');
const [dist] = process.argv.slice(2);
assert.ok(dist, 'Usage: node nav-click.cjs APP_DIST');
const origin = 'http://localhost:41723';

async function run(browser, base) {
  const page = await browser.newPage({ viewport: { width: 1440, height: 960 } });
  const errors = [];
  try {
    page.on('pageerror', err => errors.push(err.message));
    await page.addInitScript(() => {
      localStorage.setItem('onboarding_done', '1');
      localStorage.setItem('whatsnew_seen', '9999-01-01');
    });
    await page.routeWebSocket('**/ws', () => {});
    await page.route('**/*', async route => {
      const request = route.request();
      const url = new URL(request.url());
      if (url.origin !== origin) return route.abort();
      if (url.pathname.startsWith('/api/')) {
        if (url.pathname === '/api/settings') return route.fulfill({ json: { operator_names: [] } });
        return route.fulfill({ json: [], headers: { 'x-total-count': '0' } });
      }
      const asset = url.pathname.startsWith('/v2/app/') ? url.pathname.slice(8) : 'index.html';
      return route.fulfill({ path: join(dist, asset) });
    });
    await page.goto(origin + base);
    await page.getByRole('heading').first().waitFor({ timeout: 20000 });
    const prefix = base === '/' ? '' : base.replace(/\/$/, '');
    // Players, Heroes and Records now live inside the "Stats" dropdown (grouped nav): open it
    // before each click instead of clicking a flat top-bar link directly.
    for (const [tab, heading] of [['players', 'PLAYERS'], ['heroes', 'HERO POOL'], ['records', 'THE BEST SINGLE GAMES']]) {
      const before = page.url();
      await page.locator('nav.ds-topnav .ds-navgroup button').filter({ hasText: 'Stats' }).first().click({ timeout: 5000 });
      await page.locator(`nav.ds-topnav .ds-navmenu a[href="${prefix}/${tab}"]`).first().click({ timeout: 5000 });
      await page.waitForFunction(u => location.href !== u, before, { timeout: 5000 });
      assert.equal(new URL(page.url()).pathname, `${prefix}/${tab}`);
      await page.locator('h1').filter({ hasText: heading }).first().waitFor({ timeout: 5000 });
    }
    await page.goBack();
    await page.waitForFunction(h => location.pathname === h, `${prefix}/heroes`, { timeout: 5000 });
    assert.deepEqual(errors, []);
  } finally {
    await page.close();
  }
}

async function main() {
  const browser = await chromium.launch();
  try {
    await run(browser, '/');
    await run(browser, '/v2/');
    console.log('nav-click: click-driven SPA navigation and back/forward work in both base paths, no pageerror');
  } finally {
    await browser.close();
  }
}

main().catch(e => { console.error(e); process.exitCode = 1; });
