// Exercise the release WASM, with only synthetic same-origin API/WS fixtures.
const assert = require('node:assert/strict');
const { join } = require('node:path');
const { chromium } = require('playwright');
const { origin, date, fixture, eventually } = require('./fixed-sources.cjs');
const dir = process.argv[2];
const layout = { id: '430691a0-1254-4f61-9ac5-b8d6b622e125', name: 'Composition', created_at: '', updated_at: '' };
const catalog = [['ticker', '3-game win streak'], ['widget', 'DEFEAT'], ['queue', 'GAME · IN QUEUE'],
  ['recap', 'Session recap'], ['stream-pool', 'Played this stream'],
  ['patch-banner', 'Fixture patch'], ['record-flash', 'NEW RECORD'], ['nemesis', 'NEMESIS'], ['goal', '2 / 3 wins today']];
const widgets = catalog.map(([widget_type], i) => ({ id: `430691a0-1254-4f61-9ac5-b8d6b622e1${String(i).padStart(2, '0')}`,
  widget_type, grid_x: 1 + (i % 3) * 20, grid_y: 1 + Math.floor(i / 3) * 10, grid_w: 20, grid_h: 10, z_order: i,
  settings: widget_type === 'widget' ? { me: 'A+B Player' } : {} }));
const byKind = (root, kind) => root.locator(`[data-widget-type="${kind}"]`).first();

async function main() {
  assert.ok(dir, 'Usage: node custom-overlay.cjs SOURCES_DIST');
  const browser = await chromium.launch();
  const page = await browser.newPage({ viewport: { width: 1920, height: 1080 }, timezoneId: 'UTC' });
  const requests = [], sockets = [], errors = [];
  let mode = 'saved', opponentPending, releaseOpponent, lobbyGone = false, holdOpponent = false;
  try {
    page.on('pageerror', e => errors.push(e.message));
    await page.routeWebSocket('**/ws', ws => sockets.push(ws));
    await page.route('**/*', async route => {
      const request = route.request();
      const url = new URL(request.url());
      if (url.origin !== origin) return route.abort();
      if (url.pathname.startsWith('/api/')) {
        assert.equal(request.method(), 'GET');
        assert.equal(request.headers().authorization, undefined);
        // gloo-net 0.6.0's Request::get always appends a trailing '&' to an already-present query
        // string when .query() isn't used (TryFrom<RequestBuilder> combines url.search() with an
        // empty value.query). Harmless server-side (axum ignores the empty trailing pair); strip
        // it here so assertions can match the logical path exactly.
        requests.push((url.pathname + url.search).replace(/&$/, ''));
        if (url.pathname.startsWith('/api/overlay-layouts/')) {
          assert.equal(url.pathname, `/api/overlay-layouts/${layout.id}`);
          if (mode === 'missing') return route.fulfill({ status: 404 });
          if (mode === 'transport') return route.abort();
          return route.fulfill({ json: { ...layout, widgets: mode === 'invalid' ? [{ ...widgets[0], grid_w: 81 }] : widgets } });
        }
        if (url.pathname === '/api/lobby' && lobbyGone) return route.fulfill({ status: 204 });
        const against = url.searchParams.has('against');
        if (against && holdOpponent && !url.searchParams.has('result')) {
          opponentPending = true;
          await new Promise(resolve => { releaseOpponent = resolve; });
        }
        const headers = against ? { 'x-total-count': url.searchParams.has('result') ? '1' : '5' } : {};
        return route.fulfill({ json: fixture(url, true), headers });
      }
      if (url.pathname === '/host') return route.fulfill({ contentType: 'text/html', body: `<script>
        window.events = []; addEventListener('message', e => events.push({data:e.data, origin:e.origin, peer:e.source===document.querySelector('iframe').contentWindow}));
        </script><iframe src="/v2/overlay/custom?preview=1" width="1920" height="1080"></iframe>` });
      if (url.pathname === '/sibling') return route.fulfill({ contentType: 'text/html', body: '<html></html>' });
      if (url.pathname === '/favicon.ico') return route.fulfill({ status: 204 });
      return route.fulfill({ path: join(dir, url.pathname.startsWith('/v2/sources/') ? url.pathname.slice(12) : 'index.html') });
    });
    await page.clock.install({ time: new Date(+date - 1000) });
    await page.clock.pauseAt(date);
    await page.goto(`${origin}/overlay/custom?id=${layout.id}`);
    for (const [kind, text] of catalog) await eventually(async () => (await byKind(page, kind).textContent({ timeout: 100 })).includes(text), kind);
    assert.equal(await page.locator('[data-widget-type]').count(), 9);
    assert.equal(sockets.length, 1);
    assert.equal(requests.filter(r => r === '/api/sessions?limit=1').length, 1);
    assert.equal(requests.filter(r => r === '/api/matches?limit=1').length, 1);
    const inspect = root => byKind(root, 'queue').evaluate(el => {
      const r = el.getBoundingClientRect(), scene = el.firstElementChild.firstElementChild.getBoundingClientRect();
      return [r.x, r.y, r.width, r.height, getComputedStyle(el).overflow, scene.width, scene.height];
    });
    const savedGeometry = await inspect(page);
    assert.deepEqual(savedGeometry.slice(0, 5), [984, 24, 480, 240, 'hidden']);
    assert.ok(Math.abs(savedGeometry[5] / savedGeometry[6] - 16 / 9) < .001 && savedGeometry[5] <= 480.01 && savedGeometry[6] <= 240.01);
    const png = await page.screenshot({ omitBackground: true, animations: 'allow' });
    const alpha = await page.evaluate(async b64 => {
      const img = new Image(); img.src = `data:image/png;base64,${b64}`; await img.decode();
      const c = document.createElement('canvas'); const ctx = c.getContext('2d'); ctx.drawImage(img, -1919, -1079);
      return ctx.getImageData(0, 0, 1, 1).data[3];
    }, png.toString('base64'));
    assert.equal(alpha, 0);
    const liveStorage = await page.evaluate(() => Object.entries(localStorage));
    assert.equal(liveStorage.length, 1);
    assert.notEqual(liveStorage[0][0], 'patch_banner_last_seen');
    await page.reload();
    await byKind(page, 'patch-banner').waitFor();
    await eventually(() => byKind(page, 'nemesis').getByText('⚠ NEMESIS').isVisible(), 'reload lobby');
    assert.equal(await byKind(page, 'patch-banner').textContent(), '');
    console.log('saved: 9 widgets, alpha, queue geometry, shared feeds, per-layout patch acknowledgement');

    for (const [label, text] of [['missing', 'Layout not found'], ['transport', 'Unable to load layout'], ['invalid', 'Unable to load layout']]) {
      mode = label; await page.reload(); await page.getByText(text, { exact: true }).waitFor();
      assert.equal(await page.locator('[data-widget-type]').count(), 0);
    }
    const count = requests.length;
    await page.goto(`${origin}/overlay/custom?preview=1&id=${layout.id}`);
    await page.getByText('Preview is only available inside the builder').waitFor();
    assert.equal(requests.length, count);

    const oldSocketCount = sockets.length;
    holdOpponent = true;
    await page.goto(`${origin}/host`);
    await eventually(() => page.evaluate(() => events.some(e => e.data.type === 'storm-overlay-preview-ready' && e.origin === location.origin && e.peer && e.data.version === 1)), 'preview ready');
    const root = page.frameLocator('iframe').first();
    const send = (ws, extra = {}) => page.evaluate(([w, e]) => document.querySelector('iframe').contentWindow.postMessage(
      { type: 'storm-overlay-preview', version: 1, widgets: w, ...e }, location.origin), [ws, extra]);
    await page.evaluate(w => {
      const child = document.querySelector('iframe'); const sibling = document.createElement('iframe'); sibling.src = '/sibling';
      sibling.onload = () => sibling.contentWindow.eval(`parent.frames[0].postMessage(${JSON.stringify({ type: 'storm-overlay-preview', version: 1, widgets: w })}, location.origin)`);
      document.body.append(sibling);
      child.contentWindow.dispatchEvent(new child.contentWindow.MessageEvent('message', {
        origin: 'https://other.test', source: window, data: { type: 'storm-overlay-preview', version: 1, widgets: w }
      }));
    }, widgets);
    await send(widgets, { admin_token: 'not-a-real-token' });
    await send(widgets, { version: 2 });
    await send([widgets[0], widgets[0]]);
    await send([{ ...widgets[0], grid_x: 80 }]);
    await send([{ ...widgets[0], settings: { surprise: true } }]);
    await page.waitForTimeout(100);
    assert.equal(await root.locator('[data-widget-type]').count(), 0);
    assert.equal(requests.length, count);
    await send(widgets);
    await eventually(() => root.locator('[data-widget-type]').count().then(n => n === 9), 'preview places nine widgets');
    assert.deepEqual(await inspect(root), savedGeometry);
    await eventually(() => opponentPending, 'held opponent read');
    assert.equal(sockets.length, oldSocketCount + 1);
    lobbyGone = true;
    const lobbyReads = () => requests.filter(r => r === '/api/lobby').length;
    const prior = lobbyReads();
    sockets.at(-1).send(JSON.stringify({ type: 'lobby.updated' }));
    await eventually(() => lobbyReads() > prior, 'new lobby read before old opponent lookup has settled');
    releaseOpponent();
    const ticker = byKind(root, 'ticker');
    await eventually(async () => (await ticker.textContent()).includes('3-game win streak'), 'preview ticker');
    await ticker.evaluate(el => { el.dataset.identity = 'keep'; });
    const rearranged = widgets.toReversed().map(w => w.widget_type === 'ticker' ? { ...w, grid_x: 2, z_order: 99 } : w);
    await send(rearranged);
    await eventually(() => ticker.evaluate(el => el.style.left === '48px' && el.style.zIndex === '99'), 'moves keyed widget');
    assert.equal(await ticker.getAttribute('data-identity'), 'keep');
    await eventually(async () => (await byKind(root, 'patch-banner').textContent()).includes('Fixture patch'), 'preview patch ignores live acknowledgement');
    assert.equal(await byKind(root, 'nemesis').textContent(), '');
    assert.deepEqual(await page.evaluate(() => Object.entries(localStorage)), liveStorage);
    await send([]);
    await eventually(async () => await root.locator('[data-widget-type]').count() === 0, 'clears snapshot');
    assert.deepEqual(errors, []);
    console.log('preview: peer/origin/version/schema guards, preserved identity, stale opponent rejected, no writes or storage changes');
  } finally { await browser.close(); }
}
main().catch(e => { console.error(e); process.exitCode = 1; });
