// Run with NODE_PATH and PLAYWRIGHT_BROWSERS_PATH pointing to an isolated Playwright installation.
// Usage: node web-rs/app/tests/overlay-builder.cjs APP_DIST [SCREENSHOT_DIR]
const assert = require('node:assert/strict');
const { join } = require('node:path');
const { chromium } = require('playwright');
const { randomUUID } = require('node:crypto');
const [dist, evidence] = process.argv.slice(2);
const origin = 'http://localhost:41721';
const fixtureCredential = 'fixture-only';
const api = '/api/admin/overlay-layouts';
const stamp = '2026-11-20T12:00:00Z';
const preview = `<!doctype html><script>
window.messages = [];
window.simulations = [];
addEventListener('message', event => {
  if (event.origin === location.origin && event.source === parent) {
    (event.data.type === 'storm-overlay-simulate' ? simulations : messages).push(event.data);
  }
});
window.ready = () => parent.postMessage({type:'storm-overlay-preview-ready',version:1}, location.origin);
</script>`;

async function run(browser, base) {
  const page = await browser.newPage({ viewport: { width: 1440, height: 960 } });
  const errors = [], fixtureErrors = [], saved = new Map();
  let rejectWrite = false;
  let writes = 0;
  const button = name => page.getByRole('button', { name, exact: true });
  const label = name => page.getByLabel(name, { exact: true });
  const status = text => page.getByText(text, { exact: true }).waitFor();
  const summary = ({ widgets, ...rest }) => rest;
  try {
    page.on('pageerror', err => errors.push(err.message));
    page.on('dialog', dialog => dialog.accept());
    await page.addInitScript(() => {
      if (window !== top) return;
      localStorage.setItem('onboarding_done', '1');
      localStorage.setItem('whatsnew_seen', '9999-01-01');
      Object.defineProperty(navigator, 'clipboard', { value: {
        writeText: async value => { window.copied = value; },
      }});
    });
    await page.routeWebSocket('**/ws', () => {});
    await page.route('**/*', async route => {
      try {
        const request = route.request();
        const url = new URL(request.url());
        assert.equal(url.origin, origin);
        const path = url.pathname;
        if (path === `${base}/overlay/custom`) {
          assert.equal(url.search, '?preview=1');
          assert.equal(request.headers().authorization, undefined);
          return await route.fulfill({ contentType: 'text/html', body: preview });
        }
        if (path === api || path.startsWith(`${api}/`)) {
          if (request.headers().authorization !== `Bearer ${fixtureCredential}`) return await route.fulfill({ status: 401 });
          const id = path.slice(api.length + 1);
          const method = request.method();
          if (method === 'GET') return await route.fulfill({ json: id ? saved.get(id) : [...saved.values()].map(summary) });
          if (method === 'DELETE') {
            assert.ok(saved.delete(id));
            return await route.fulfill({ status: 204 });
          }
          assert.equal(method, id ? 'PUT' : 'POST');
          if (rejectWrite) return await route.fulfill({ status: 500 });
          const body = request.postDataJSON();
          assert.deepEqual(Object.keys(body).sort(), ['name', 'widgets']);
          const layout = { id: id || randomUUID(), name: body.name, created_at: stamp, updated_at: stamp,
            widgets: body.widgets.map((widget, i) => {
              assert.equal(widget.id, undefined);
              assert.equal(widget.z_order, i);
              assert.ok(widget.grid_x >= 0 && widget.grid_x + widget.grid_w <= 80);
              assert.ok(widget.grid_y >= 0 && widget.grid_y + widget.grid_h <= 45);
              return { id: randomUUID(), ...widget };
            }) };
          writes++;
          saved.set(layout.id, layout);
          return await route.fulfill({ status: id ? 200 : 201, json: layout });
        }
        if (path.startsWith('/api/')) return await route.fulfill({ json: path === '/api/settings' ? { operator_names: [] } : {} });
        const asset = path.startsWith('/v2/app/') ? path.slice(8) : 'index.html';
        assert.match(asset, /^(?:fonts\/)?[\w.-]+\.(?:html|css|js|wasm|woff2)$/);
        return await route.fulfill({ path: join(dist, asset) });
      } catch (e) {
        fixtureErrors.push(e.message);
        await route.abort();
      }
    });
    await page.goto(origin + base + '/overlay-builder');
    await page.getByRole('heading', { name: 'Overlay Builder', exact: true }).waitFor({timeout: 8000});
    await page.getByRole('alert').filter({ hasText: 'Admin' }).waitFor();
    await page.evaluate(token => localStorage.setItem('admin_token', token), fixtureCredential);
    await button('Reload layouts').click();
    await button('New layout').click();
    await label('Layout name').fill('Fixture composition');
    assert.equal(await page.getByRole('group', {name: 'Widget palette'}).getByRole('button').count(), 9);
    const frame = page.frames().find(frame => frame.url().includes('?preview=1'));
    assert.ok(frame);
    await frame.waitForFunction(() => typeof window.ready === 'function');
    assert.deepEqual(await frame.evaluate(() => [innerWidth, innerHeight]), [1920, 1080]);
    await page.evaluate(() => postMessage({type: 'storm-overlay-preview-ready', version: 1}, location.origin));
    await frame.evaluate(() => parent.dispatchEvent(new parent.MessageEvent('message', {source: window, origin: 'https://wrong.invalid', data: {type:'storm-overlay-preview-ready', version:1}})));
    await frame.evaluate(() => parent.postMessage({type:'storm-overlay-preview-ready',version:2}, location.origin));
    await frame.evaluate(() => new Promise(resolve => setTimeout(resolve, 80)));
    assert.deepEqual(await frame.evaluate(() => messages), []);
    assert.deepEqual(await frame.evaluate(() => simulations), []);
    await frame.evaluate(() => ready());
    await frame.waitForFunction(() => messages.at(-1)?.widgets.length === 0);
    await frame.waitForFunction(() => simulations.length === 3);
    assert.deepEqual(await frame.evaluate(() => simulations), ['patch-banner', 'nemesis', 'record-flash'].map(kind => ({
      type: 'storm-overlay-simulate', kind, active: false,
    })));
    await button('Add Scorecard').press('Enter');
    await label('Player name (me)').fill('A+B Player');
    await button('Add Queue').click();
    await button('Add Ticker').click();
    const canvas = page.locator('.ob-canvas');
    const box = await canvas.boundingBox();
    const drop = await page.evaluateHandle(() => new DataTransfer());
    await button('Add Daily goal').dispatchEvent('dragstart', {dataTransfer: drop});
    await canvas.dispatchEvent('drop', {dataTransfer: drop, clientX: box.x + box.width / 2, clientY: box.y + box.height / 2});
    await frame.waitForFunction(() => messages.at(-1)?.widgets.length === 4);
    assert.deepEqual((await frame.evaluate(() => messages.at(-1).widgets[3])).grid_x, 40);
    const move = button('Move Daily goal');
    const handle = await move.boundingBox();
    await page.mouse.move(handle.x + handle.width / 2, handle.y + handle.height / 2);
    await page.mouse.down();
    await page.mouse.move(handle.x + handle.width / 2 - 3.3 * box.width / 80, handle.y + handle.height / 2, {steps: 4});
    await page.mouse.up();
    assert.equal(await label('X (cells)').inputValue(), '37');
    const resize = await button('Resize Daily goal').boundingBox();
    await page.mouse.move(resize.x + resize.width / 2, resize.y + resize.height / 2);
    await page.mouse.down();
    await page.mouse.move(box.x + box.width + 40, box.y + box.height + 40, {steps: 4});
    await page.mouse.up();
    assert.equal(await label('Width (cells)').inputValue(), '43');
    await label('X (cells)').fill('999');
    await label('X (cells)').press('Tab');
    assert.equal(await label('X (cells)').inputValue(), '37');
    await move.press('ArrowLeft');
    assert.equal(await label('X (cells)').inputValue(), '36');
    await button('Move layer back').click();
    await frame.waitForFunction(() => messages.at(-1)?.widgets[2].widget_type === 'goal');
    const before = await frame.evaluate(() => messages.at(-1));
    assert.deepEqual(Object.keys(before).sort(), ['type','version','widgets']);
    assert.equal(before.type, 'storm-overlay-preview');
    assert.equal(before.version, 1);
    assert.deepEqual(before.widgets[0].settings, { me: 'A+B Player' });
    assert.deepEqual(before.widgets[1].settings, {});
    assert.ok(!JSON.stringify(before).includes(fixtureCredential));
    await status('Unsaved changes');
    await button('Save layout').click();
    await status('Saved');
    const [firstId, first] = [...saved.entries()][0];
    const expectedUrl = `${origin}${base}/overlay/custom?id=${firstId}`;
    assert.equal(await label('OBS URL').inputValue(), expectedUrl);
    await button('Copy OBS URL').click();
    await status('OBS URL copied.');
    assert.equal(await page.evaluate(() => copied), expectedUrl);
    rejectWrite = true;
    await label('Layout name').fill('Renamed layout');
    await button('Save layout').click();
    await page.getByRole('alert').filter({hasText:'500'}).waitFor();
    await status('Unsaved changes');
    rejectWrite = false;
    await button('Save layout').click();
    await status('Saved');
    assert.equal(saved.get(firstId).name, 'Renamed layout');
    assert.notDeepEqual(first.widgets.map(w => w.id), saved.get(firstId).widgets.map(w => w.id));
    assert.deepEqual((await frame.evaluate(() => messages.at(-1).widgets)).map(w => w.id), before.widgets.map(w => w.id));
    assert.equal(await label('OBS URL').inputValue(), expectedUrl);
    if (evidence) await page.screenshot({path: join(evidence, base ? 'editor-v2.png' : 'editor-root.png'), fullPage: true});
    await button('New layout').click();
    await label('Saved layouts').selectOption(firstId);
    await status('Saved');
    assert.equal(await label('Layout name').inputValue(), 'Renamed layout');
    await button('Move Scorecard').press('Enter');
    assert.equal(await label('Player name (me)').inputValue(), 'A+B Player');
    // Backspace/Delete acts on the selected widget (Scorecard), not whichever handle merely
    // has keyboard focus (Queue, Tab-ed to without clicking) — Tab-ing must stay non-destructive.
    await button('Move Queue').focus();
    await page.keyboard.press('Delete');
    await frame.waitForFunction(() => messages.at(-1)?.widgets.length === 3);
    const afterDelete = await frame.evaluate(() => messages.at(-1).widgets);
    assert.ok(afterDelete.some(w => w.widget_type === 'queue'));
    assert.ok(!afterDelete.some(w => w.widget_type === 'widget'));
    await button('Add Scorecard').press('Enter');
    await label('Player name (me)').fill('A+B Player');
    await frame.waitForFunction(() => messages.at(-1)?.widgets.length === 4);
    await button('Remove widget').click();
    await frame.waitForFunction(() => messages.at(-1)?.widgets.length === 3);
    await button('Delete layout').click();
    await status('Layout deleted.');
    assert.equal(saved.size, 0);
    assert.equal(writes, 2);
    assert.deepEqual(errors, []);
    assert.deepEqual(fixtureErrors, []);
    console.log(`${base || '/'}: CRUD, draft geometry, options, clipboard, guarded preview messaging passed`);
  } finally { await page.close(); }
}

(async () => {
  const browser = await chromium.launch();
  try { await run(browser, ''); await run(browser, '/v2'); }
  finally { await browser.close(); }
})().catch(error => { console.error(error); process.exitCode = 1; });
