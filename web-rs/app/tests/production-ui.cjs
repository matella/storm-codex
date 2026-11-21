// Run against a disposable standalone server with the four distinct repo replay fixtures
// (dedupe the two copies of Silver City), qa-invalid.StormReplay, and operator_names=["matella"].
// Usage: node production-ui.cjs ORIGIN [SCREENSHOT_DIR]
const assert = require('node:assert/strict');
const { readFileSync } = require('node:fs');
const { join } = require('node:path');
const { chromium } = require('playwright');
const [origin, evidence] = process.argv.slice(2);
assert.ok(origin, 'Usage: node production-ui.cjs ORIGIN [SCREENSHOT_DIR]');

async function point(locator) {
  return locator.evaluate(el => {
    const r = el.getBoundingClientRect();
    const x = r.left + r.width / 2;
    const y = r.top + r.height / 2;
    const hit = document.elementFromPoint(x, y);
    return { x, y, hit: el.contains(hit), hitTag: hit?.tagName };
  });
}

async function pointer(page, locator, label) {
  const at = await point(locator);
  await page.mouse.click(at.x, at.y);
  assert.ok(at.hit, `${label}: pointer at ${at.x},${at.y} hits ${at.hitTag}`);
}

async function active(locator) {
  return locator.evaluate(el => el === document.activeElement);
}

async function tabTo(page, locator) {
  for (let i = 0; i < 45; i++) {
    await page.keyboard.press('Tab');
    if (await locator.count() && await active(locator)) return;
  }
  assert.fail(`Not in keyboard Tab order: ${locator}`);
}

async function archive(page, path = '/matches') {
  await page.goto(origin + path);
  await page.getByRole('heading', { name: 'Matches', exact: true }).waitFor();
  await page.locator('main a[href^="/match/"]').first().waitFor();
}

async function snapshot(page, name) {
  if (evidence) await page.screenshot({ path: join(evidence, `${name}.png`), animations: 'disabled' });
}

const checks = {};
for (const width of [1440, 390]) {
  for (const [group, item, path] of [['Stats', 'Players', '/players'], ['Tools', 'Cast', '/cast']]) {
    checks[`pointer ${width} ${group}`] = async page => {
      await page.setViewportSize({ width, height: 844 });
      await archive(page);
      const nav = page.getByRole('navigation', { name: 'Main' });
      const trigger = nav.getByRole('button', { name: group, exact: true });
      // Expose the trigger in the compact horizontal nav, but never scroll the flyout entry into view.
      await trigger.scrollIntoViewIfNeeded();
      await pointer(page, trigger, group);
      assert.equal(await trigger.getAttribute('aria-expanded'), 'true');
      await snapshot(page, `nav-${width}-${group}`);
      await pointer(page, nav.getByRole('link', { name: item, exact: true }), `${group} → ${item}`);
      await page.waitForURL(origin + path);
      assert.equal(await trigger.getAttribute('aria-expanded'), 'false');
    };
    checks[`escape ${width} ${group}`] = async page => {
      await page.setViewportSize({ width, height: 844 });
      await archive(page);
      const trigger = page.getByRole('button', { name: group, exact: true });
      const other = page.getByRole('button', { name: group === 'Stats' ? 'Tools' : 'Stats', exact: true });
      await trigger.focus();
      await page.keyboard.press('Space');
      assert.equal(await trigger.getAttribute('aria-expanded'), 'true');
      await page.keyboard.press('Tab');
      assert.ok(await page.evaluate(() => document.activeElement.matches('.ds-navmenu a')));
      await page.keyboard.press('Escape');
      assert.equal(await trigger.getAttribute('aria-expanded'), 'false');
      assert.ok(await active(trigger), `Escape should return focus to ${group}`);
      assert.equal(await trigger.getAttribute('aria-haspopup'), null);
      await page.keyboard.press('Enter');
      await other.focus();
      await page.keyboard.press('Escape');
      assert.equal(await trigger.getAttribute('aria-expanded'), 'false');
      assert.ok(await active(other), 'Escape must not steal outside focus');
    };
  }
}

for (const [key, destination] of [['Tab', 'Tools'], ['Shift+Tab', 'Live']]) {
  checks[`390px Stats focus out with ${key}`] = async page => {
    await page.setViewportSize({ width: 390, height: 844 });
    await archive(page);
    const nav = page.getByRole('navigation', { name: 'Main' });
    const trigger = nav.getByRole('button', { name: 'Stats', exact: true });
    const next = key === 'Tab'
      ? nav.getByRole('button', { name: destination, exact: true })
      : nav.getByRole('link', { name: destination, exact: true });
    await tabTo(page, trigger);
    await page.keyboard.press('Enter');
    assert.equal(await trigger.getAttribute('aria-expanded'), 'true');
    const menu = nav.locator('#' + await trigger.getAttribute('aria-controls'));
    if (key === 'Tab') {
      for (const link of await menu.getByRole('link').all()) {
        await page.keyboard.press('Tab');
        assert.ok(await active(link), 'Tab follows the open disclosure links');
        assert.equal(await trigger.getAttribute('aria-expanded'), 'true');
      }
    }
    await page.keyboard.press(key);
    assert.ok(await active(next), `${key} should leave focus on ${destination}`);
    await snapshot(page, `nav-390-focus-out-${key}`);
    assert.equal(await trigger.getAttribute('aria-expanded'), 'false', `${key} should close the disclosure`);
    assert.equal(await menu.isVisible(), false);
  };
}

checks['archive keyboard'] = async page => {
  await archive(page);
  await page.getByRole('heading', { name: 'Matches', exact: true }).focus();
  const rows = page.locator('main a[href^="/match/"]');
  for (const [name, key, param, value, expected] of [
    ['ARAM', 'Space', 'mode', '50101', 4], ['Wins', 'Enter', 'result', 'win', 2], [/MVP/, 'Space', 'mvp', 'true', 0],
  ]) {
    const button = page.locator('main').getByRole('button', { name, exact: true });
    await tabTo(page, button);
    assert.equal(await button.getAttribute('aria-pressed'), 'false');
    const request = page.waitForResponse(r => {
      const url = new URL(r.url());
      return url.pathname === '/api/matches' && url.searchParams.get(param) === value;
    });
    await page.keyboard.press(key);
    await page.waitForFunction(([p, v]) => new URLSearchParams(location.search).get(p) === v, [param, value]);
    assert.equal(await button.getAttribute('aria-pressed'), 'true');
    const response = await request;
    assert.equal(response.status(), 200);
    assert.equal((await response.json()).length, expected);
    await page.waitForFunction(n => document.querySelectorAll('main a[href^="/match/"]').length === n, expected);
    assert.equal(await rows.count(), expected);
  }
  await snapshot(page, 'archive-filters');
  const reset = page.getByRole('button', { name: /reset/i });
  await tabTo(page, reset);
  await page.keyboard.press('Enter');
  await page.waitForURL(origin + '/matches');

  await reset.waitFor({ state: 'detached' });
  const all = page.locator('main').getByRole('button', { name: 'All', exact: true });
  assert.equal(await all.getAttribute('aria-pressed'), 'true');
  assert.ok(await active(all), 'Reset should leave focus on the All mode filter');

  await page.goto(origin + '/matches?map=Silver+City');
  const input = page.getByPlaceholder('search map…');
  await input.focus();
  await tabTo(page, page.getByRole('button', { name: /clear map/i }));
  await page.keyboard.press('Space');
  await page.waitForURL(origin + '/matches');
  assert.equal(await input.inputValue(), '');
  assert.ok(await active(input), 'Clearing a datalist restores focus to the cleared input');
};

checks['390px detail'] = async page => {
  await page.setViewportSize({ width: 390, height: 844 });
  await archive(page);
  const match = await page.locator('main a[href^="/match/"]').first().getAttribute('href');
  await page.goto(origin + match);
  const replay = page.getByRole('button', { name: 'Replay 2D' });
  await replay.waitFor();
  const width = () => page.evaluate(() => document.documentElement.scrollWidth);
  const actions = [page.getByRole('button', { name: 'Score', exact: true }), replay,
    page.getByRole('switch', { name: 'Show player names' }), page.getByRole('button', { name: 'Save as image' }),
    page.getByRole('link', { name: /raw dump/i })];
  const offscreen = async () => (await Promise.all(actions.map(a => a.evaluate(el => {
    const { left, right } = el.getBoundingClientRect();
    return left < 0 || right > document.documentElement.clientWidth ? el.textContent : null;
  })))).filter(Boolean);
  await snapshot(page, 'detail-score');
  const score = { width: await width(), offscreen: await offscreen() };
  await replay.click();
  const slider = page.getByRole('slider', { name: 'replay time' });
  await slider.waitFor();
  await snapshot(page, 'detail-2d');
  const twoD = { width: await width(), offscreen: await offscreen() };
  assert.deepEqual({ score, twoD }, { score: { width: 390, offscreen: [] }, twoD: { width: 390, offscreen: [] } });
  const download = page.waitForEvent('download');
  await actions[3].click();
  assert.match((await download).suggestedFilename(), /^storm-codex-.*\.png$/);
  const initial = await slider.inputValue();
  await page.getByRole('button', { name: 'play', exact: true }).click();
  await page.waitForFunction(value => document.querySelector('[aria-label="replay time"]').value !== value, initial);
};

checks['rejected replay'] = async page => {
  const response = await page.request.get(origin + '/api/standalone/status');
  assert.ok(response.ok());
  const { phase, done, total, rejected } = await response.json();
  assert.equal(phase, 'ready');
  assert.equal(rejected.length, 1);
  assert.equal(rejected[0].file, 'qa-invalid.StormReplay');
  await page.goto(origin + '/welcome');
  await page.getByText(`${done}/${total}`, { exact: true }).waitFor();
  await snapshot(page, 'welcome-rejected');
  const text = await page.locator('main').innerText();
  assert.match(text, new RegExp(`${done}/${total}\\s+processed`, 'i'));
  assert.match(text, /rejected replays were not imported/i);
  assert.doesNotMatch(text, /all your replays are imported/i);
  console.log(`  standalone API: ${done}/${total} processed, ${rejected.length} rejected`);
};

for (const path of ['/matches', '/match/1', '/v2/match/1']) {
  checks[`fonts ${path}`] = async page => {
    const responses = new Map();
    page.on('response', r => responses.set(r.url(), r.status()));
    await page.goto(origin + path);
    const fonts = await page.locator('link[rel=preload][as=font]').evaluateAll(els => els.map(e => e.href));
    assert.equal(fonts.length, 3);
    for (const font of fonts) {
      assert.match(new URL(font).pathname, /^\/v2\/app\/fonts\/.+\.woff2$/);
      assert.equal(responses.get(font), 200, font);
    }
  };
  checks[`favicon ${path}`] = async page => {
    await page.goto(origin + path);
    const icons = await page.locator('link[rel=icon]').evaluateAll(els => els.map(e => e.href));
    assert.equal(icons.length, 1);
    assert.match(new URL(icons[0]).pathname, /^\/v2\/app\/.+\.png$/);
    const icon = await page.request.get(icons[0]);
    assert.equal(icon.status(), 200);
    assert.equal(icon.headers()['content-type'], 'image/png');
    assert.deepEqual(await icon.body(), readFileSync(join(__dirname, '../../../desktop/icons/32x32.png')));
  };
}

(async () => {
  const browser = await chromium.launch();
  try {
    for (const [name, check] of Object.entries(checks)) {
      if (process.env.AUDIT_FILTER && !name.includes(process.env.AUDIT_FILTER)) continue;
      const page = await browser.newPage();
      page.setDefaultTimeout(5000);
      const errors = [];
      page.on('pageerror', e => errors.push(String(e)));
      await page.addInitScript(() => {
        localStorage.setItem('onboarding_done', '1');
        localStorage.setItem('whatsnew_seen', '9999-01-01');
        sessionStorage.setItem('standalone_welcome_decided', '1');
      });
      try {
        await check(page);
        assert.deepEqual(errors, []);
        console.log(`PASS ${name}`);
      } catch (error) {
        console.error(`FAIL ${name}: ${error.message}`, ...errors);
        process.exitCode = 1;
      } finally {
        await page.close();
      }
    }
  } finally {
    await browser.close();
  }
})().catch(e => { console.error(e); process.exitCode = 1; });
