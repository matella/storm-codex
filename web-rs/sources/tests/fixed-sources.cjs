// Compare two release builds in Chromium without a server, live data or OBS.
// NODE_PATH must point to an isolated Playwright install (see the overlay-builder spec).
const assert = require('node:assert/strict');
const { join } = require('node:path');
const { chromium } = require('playwright');

const origin = 'http://sources.test';
const date = new Date('2026-11-20T12:00:00Z');
const operator = { name: 'Operator', hero: 'Jaina', team: 0, kills: 4, takedowns: 7, deaths: 2 };
const opponent = { name: 'A+B Player', hero: 'Thrall', team: 1, toon: '1-Hero-1-42' };
const match = (id, playedAt) => ({ id, played_at: playedAt, winner: 0, length: 900, mode: 50001,
  map: 'Synthetic map', players: [operator, opponent] });
const matches = [3, 2, 1].map(id => match(id, `2026-11-20T0${id}:00:00Z`));
const track = { authenticated: true, current: { isPlaying: true, progressMs: 5000,
  track: { id: 'song', name: 'Fixture track', artist: 'Fixture artist', durationMs: 180000 } } };
const patch = { internalId: 'fixture-patch', patchName: 'Fixture patch', liveDate: '2026-11-19' };
const lobby = { content_hash: 'lobby', me: 0, players: [{ team: 0 }, { team: 1, toon_handle: opponent.toon }] };

const routes = [
  ['/ticker', '3-game win streak'], ['/widget', 'VICTORY'],
  ['/widget?me=A%2BB+Player', 'DEFEAT'], ['/widget?me=Unknown', 'VICTORY'],
  ['/queue', 'GAME · IN QUEUE'], ['/now-playing', 'ORPHEUS'], ['/now-playing?mini=0', 'Fixture track'],
  ['/now-playing?reveal', 'Fixture track'], ['/now-playing?mini&reveal', 'Fixture track'],
  ['/recap', 'Session recap'], ['/stream-pool', 'Played this stream'], ['/patch-banner', 'Fixture patch'],
  ['/record-flash', 'NEW RECORD'], ['/nemesis', 'NEMESIS'], ['/goal', '2 / 3 wins today'],
];

function fixture(url, populated) {
  const q = url.searchParams;
  switch (url.pathname) {
    case '/api/dim/heroes': return {};
    case '/api/settings': return { operator_names: [operator.name] };
    case '/api/matches': return populated ? matches.slice(0, Number(q.get('limit'))) : [];
    case '/api/now-playing': return populated ? track : { authenticated: false };
    case '/api/sessions': return { sessions: populated ? [{ id: 3 }] : [] };
    case '/api/sessions/3': return { record: { wins: 3 }, streaks: { current: 3 },
      heroes: [{ name: 'Jaina', games: 3, wins: 3 }] };
    case '/api/patches': return { items: populated ? [patch] : [] };
    case '/api/records': return { stats: [{ key: 'kills', label: 'Kills',
      rows: [{ value: 12, toon: 'operator', match_id: 3, mine: true }] }] };
    case '/api/lobby': return populated ? lobby : null;
    case '/api/goal/progress': return populated ? { kind: 'wins_today', target: 3, done: 2 } : { done: 0 };
    default: throw new Error(`Unexpected API: ${url}`);
  }
}

async function eventually(test, description) {
  for (let i = 0; i < 100; i++) {
    if (await test()) return;
    await new Promise(resolve => setTimeout(resolve, 25));
  }
  throw new Error(`Timed out: ${description}`);
}

async function snapshot(browser, dir, path, expected, populated) {
  const page = await browser.newPage({ viewport: { width: 1920, height: 1080 }, timezoneId: 'UTC' });
  const errors = [];
  const requests = [];
  let socket;
  let pending = 0;
  const state = async () => page.evaluate(() => ({
    html: document.body.innerHTML.replace(/<!--.*?-->/gs, ''),
    bg: [document.documentElement, document.body].map(el => getComputedStyle(el).backgroundColor),
    text: document.body.textContent.trim(),
  }));
  try {
    page.on('pageerror', error => errors.push(error.message));
    page.on('console', message => { if (message.type() === 'error') errors.push(message.text()); });
    await page.routeWebSocket('**/ws', ws => { socket = ws; });
    await page.route('**/*', async route => {
      const url = new URL(route.request().url());
      assert.equal(url.origin, origin);
      pending++;
      try {
        if (url.pathname.startsWith('/api/')) {
          requests.push(url.pathname + url.search);
          const headers = url.searchParams.has('against')
            ? { 'x-total-count': url.searchParams.has('result') ? '1' : '5' } : {};
          await route.fulfill({ json: fixture(url, populated), headers });
        } else {
          const asset = url.pathname.startsWith('/v2/sources/')
            ? url.pathname.slice('/v2/sources/'.length) : 'index.html';
          if (url.pathname === '/favicon.ico') {
            await route.fulfill({ status: 204 });
          } else {
            assert.ok(asset === 'index.html' || /^(?:fonts\/)?[\w.-]+\.(?:css|js|wasm|woff2)$/.test(asset),
              `Unexpected bundle asset: ${asset}`);
            await route.fulfill({ path: join(dir, asset) });
          }
        }
      } finally {
        pending--;
      }
    });
    await page.clock.install({ time: new Date(+date - 1000) });
    await page.clock.pauseAt(date);
    await page.goto(origin + path, { waitUntil: 'load' });
    await eventually(async () => pending === 0 && requests.length > 0 &&
      (populated ? (await state()).text.includes(expected) : await page.locator('body > div').count()), path);
    const before = await state();
    assert.deepEqual(before.bg, ['rgba(0, 0, 0, 0)', 'rgba(0, 0, 0, 0)'], path);
    assert.match(before.html, /width:1920px;height:1080px/, path);
    if (!populated && !path.endsWith('/queue')) assert.equal(before.text.trim() === '' ||
      (path.includes('reveal') && /class="npr [^"]*hidden/.test(before.html)), true, path);
    const firstRequests = [...requests].sort();
    let after = null;

    if (populated && path === '/ticker') {
      await page.clock.runFor(3900);
      assert.match((await state()).text, /3-game win streak/);
      await page.clock.runFor(100);
      after = await state();
      assert.doesNotMatch(after.text, /3-game win streak/);
      assert.ok(socket, 'ticker connects to /ws');
      socket.send(JSON.stringify({ type: 'match.parsed' }));
      await eventually(() => pending === 0 && requests.filter(p => p === '/api/matches?limit=200&').length === 2, 'ticker refetch');
    } else if (populated && path === '/record-flash') {
      await page.clock.runFor(7900);
      assert.match((await state()).text, /NEW RECORD/);
      await page.clock.runFor(100);
      after = await state();
      assert.doesNotMatch(after.text, /NEW RECORD/);
    } else if (populated && path === '/nemesis') {
      assert.ok(socket, 'nemesis connects to /ws');
      socket.send(JSON.stringify({ type: 'lobby.updated' }));
      await eventually(() => pending === 0 && requests.filter(p => p === '/api/lobby').length === 2, 'lobby refetch');
      assert.equal(requests.filter(p => p.includes('against=')).length, 2, 'once per resolved lobby');
    } else if (populated && path.startsWith('/now-playing')) {
      const reveal = path.includes('reveal');
      const period = reveal ? 2000 : 5000;
      await page.clock.runFor(period - 1);
      assert.equal(requests.filter(p => p === '/api/now-playing').length, 1);
      await page.clock.runFor(1);
      await eventually(() => pending === 0 && requests.filter(p => p === '/api/now-playing').length === 2, 'music poll');
      if (reveal) {
        await page.clock.runFor(600);
        after = await state();
        assert.notEqual(after.html, before.html, 'reveal folds into mini');
      }
    } else if (populated && path === '/goal') {
      await page.clock.runFor(9999);
      assert.equal(requests.filter(p => p === '/api/goal/progress').length, 1);
      await page.clock.runFor(1);
      await eventually(() => pending === 0 && requests.length === 2, 'goal poll');
    } else if (populated && path === '/patch-banner') {
      await eventually(() => page.evaluate(() => localStorage.getItem('patch_banner_last_seen') === 'fixture-patch'), 'patch acknowledgement');
      await page.reload({ waitUntil: 'load' });
      await eventually(() => pending === 0 && requests.length === 2, 'patch reload');
      await eventually(async () => (await state()).text === '', 'acknowledged patch stays hidden');
      after = await state();
    }

    if (!populated && path === '/goal') {
      const png = await page.screenshot({ omitBackground: true, timeout: 4000 });
      const cornerAlpha = await page.evaluate(async data => {
        const image = new Image();
        image.src = `data:image/png;base64,${data}`;
        await image.decode();
        const canvas = document.createElement('canvas');
        const ctx = canvas.getContext('2d');
        ctx.drawImage(image, 0, 0);
        return ctx.getImageData(0, 0, 1, 1).data[3];
      }, png.toString('base64'));
      assert.equal(cornerAlpha, 0);
    }
    assert.deepEqual(errors, [], path);
    return { before, after, firstRequests, requests: requests.sort() };
  } finally {
    await page.close();
  }
}

async function main() {
  const [base, candidate] = process.argv.slice(2);
  assert.ok(base && candidate, 'Usage: node fixed-sources.cjs BASE_DIST CANDIDATE_DIST');
  const browser = await chromium.launch();
  try {
    for (const [path, expected] of routes) {
      for (const [url, populated] of [[path, true], [`/v2${path}`, true], [path, false]]) {
        const [before, after] = await Promise.all([base, candidate].map(dir => snapshot(browser, dir, url, expected, populated)));
        assert.deepEqual(after, before, `${url} (${populated ? 'populated' : 'empty'})`);
      }
      console.log(`${path}: populated, /v2 alias and empty parity`);
    }
  } finally {
    await browser.close();
  }
}

main().catch(error => { console.error(error); process.exitCode = 1; });
