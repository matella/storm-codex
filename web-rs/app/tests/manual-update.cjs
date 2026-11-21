// Usage: node web-rs/app/tests/manual-update.cjs APP_DIST
const assert = require('node:assert/strict');
const { join } = require('node:path');
const { chromium } = require('playwright');
const [dist] = process.argv.slice(2);
assert.ok(dist, 'Usage: node manual-update.cjs APP_DIST');
const origin = 'http://localhost:41725';

async function open(page, shell) {
  page.setDefaultTimeout(10000);
  await page.addInitScript(shell => {
    localStorage.setItem('onboarding_done', '1');
    localStorage.setItem('whatsnew_seen', '9999-01-01');
    if (!shell) return;
    const calls = [];
    const pending = {};
    const fixture = { calls, pending, status: { state: 'none' } };
    window.updateFixture = fixture;
    window.__TAURI__ = { core: { invoke(command, ...args) {
      calls.push([command, ...args]);
      switch (command) {
        case 'app_version': return Promise.resolve('0.1.4');
        case 'update_status': return Promise.resolve(fixture.status);
        case 'check_for_update_now':
        case 'install_update':
          return new Promise((resolve, reject) => { pending[command] = { resolve, reject }; });
        default: throw new Error(`Unexpected Tauri command: ${command}`);
      }
    } } };
  }, shell);
  await page.routeWebSocket('**/ws', () => {});
  await page.route('**/*', route => {
    const url = new URL(route.request().url());
    if (url.origin !== origin) return route.abort();
    if (url.pathname === '/api/sessions') return route.fulfill({ json: { sessions: [] } });
    if (url.pathname === '/api/settings') return route.fulfill({ json: { operator_names: [] } });
    if (url.pathname === '/api/standalone/status') return route.fulfill({ status: 404 });
    if (url.pathname.startsWith('/api/')) return route.fulfill({ json: [], headers: { 'x-total-count': '0' } });
    const asset = url.pathname.startsWith('/v2/app/') ? url.pathname.slice(8) : 'index.html';
    return route.fulfill({ path: join(dist, asset) });
  });
  await page.goto(origin + '/');
  await page.getByRole('heading').first().waitFor();
}

async function checkDesktop(page) {
  const button = name => page.getByRole('button', { name, exact: true });
  const text = value => page.getByText(value, { exact: true });
  const check = button('Check for updates');
  const restart = button('Restart to update');
  const settle = (command, method, result) => page.waitForFunction(command =>
    !!window.updateFixture.pending[command], command).then(async handle => {
    await handle.dispose();
    await page.evaluate(({ command, method, result }) => {
      const fixture = window.updateFixture;
      const { pending } = fixture;
      if (command === 'check_for_update_now' && method === 'resolve') fixture.status = result;
      pending[command][method](result);
      delete pending[command];
    }, { command, method, result });
  });
  const command = 'check_for_update_now';

  await text('Storm Codex v0.1.4').waitFor();
  await check.click();
  const checking = button('Checking…');
  await checking.waitFor();
  assert.equal(await checking.isDisabled(), true);
  await settle(command, 'resolve', { state: 'none' });
  const current = text("You're up to date.");
  await current.waitFor();

  await check.click();
  await checking.waitFor();
  assert.equal(await current.count(), 0);
  await settle(command, 'reject', 'synthetic offline error');
  const failure = text('Update failed: synthetic offline error');
  await failure.waitFor();
  assert.equal(await check.isEnabled(), true);

  await check.click();
  await checking.waitFor();
  assert.equal(await failure.count(), 0);
  await settle(command, 'resolve', { state: 'ready', version: '0.1.5', notes: 'Fixture release notes' });
  await text('Storm Codex 0.1.5 is ready to install.').waitFor();
  assert.equal(await check.count(), 0);
  await restart.click();
  const restarting = button('Restarting…');
  await restarting.waitFor();
  assert.equal(await restarting.isDisabled(), true);
  await settle('install_update', 'reject', 'synthetic install error');
  await text('Update failed: synthetic install error').waitFor();
  assert.equal(await restart.isEnabled(), true);
  await restart.click();
  await restarting.waitFor();
  await settle('install_update', 'resolve', undefined);
  assert.equal(await restarting.isDisabled(), true);
  assert.equal(await text('Update failed: synthetic install error').count(), 0);

  const calls = await page.evaluate(() => window.updateFixture.calls);
  assert.deepEqual(calls.filter(([name]) => name !== 'update_status'), [
    ['app_version'], [command], [command], [command], ['install_update'], ['install_update'],
  ]);
  assert.deepEqual(calls[0], ['update_status']);
}

async function main() {
  const browser = await chromium.launch();
  try {
    for (const shell of [true, false]) {
      const page = await browser.newPage();
      const errors = [];
      page.on('pageerror', error => errors.push(error.message));
      await open(page, shell);
      if (shell) {
        await checkDesktop(page);
      } else {
        assert.equal(await page.locator('.sa-version, .sa-update').count(), 0);
      }
      assert.deepEqual(errors, []);
      await page.close();
    }
    console.log('manual-update: version, check busy/current/error, ready, install retry, and web-only hiding');
  } finally {
    await browser.close();
  }
}

main().catch(error => { console.error(error); process.exitCode = 1; });
