// Usage: node web-rs/sources/tests/recap-refresh.cjs SOURCES_DIST
const assert = require("node:assert/strict");
const { join } = require("node:path");
const { chromium } = require("playwright");

const [dist] = process.argv.slice(2);
assert.ok(dist, "Pass the Trunk sources dist directory");
const origin = "http://localhost:41729";

async function main() {
  const browser = await chromium.launch();
  try {
    const page = await browser.newPage();
    page.setDefaultTimeout(15000);
    const errors = [];
    page.on("pageerror", error => errors.push(String(error)));
    let current = -1;
    let holdOld = false;
    const blocked = Promise.withResolvers();
    const release = Promise.withResolvers();
    const socket = Promise.withResolvers();
    await page.routeWebSocket("**/ws", ws => socket.resolve(ws));
    await page.route("**/*", async route => {
      const { origin: from, pathname } = new URL(route.request().url());
      if (from !== origin) return route.abort();
      if (pathname === "/api/dim/heroes") return route.fulfill({ json: {} });
      if (pathname === "/api/sessions") {
        return route.fulfill({ json: { sessions: [{ id: current, manual: true, open: true }] } });
      }
      if (pathname === "/api/sessions/-1") {
        if (holdOld) {
          blocked.resolve();
          await release.promise;
        }
        return route.fulfill({ json: { id: -1, record: { wins: 1, losses: 0 } } });
      }
      if (pathname === "/api/sessions/-2") {
        return route.fulfill({ json: { id: -2, record: { wins: 0, losses: 0 } } });
      }
      const asset = pathname.startsWith("/v2/sources/") ? pathname.slice(12) : "index.html";
      return route.fulfill({ path: join(dist, asset) });
    });

    await page.goto(origin + "/recap");
    const recap = page.getByText("Session recap", { exact: true });
    await recap.waitFor();
    holdOld = true;
    (await socket.promise).send(JSON.stringify({ type: "match.parsed" }));
    await blocked.promise;

    // A newer poll sees the newly opened empty session while the live read is still blocked.
    current = -2;
    await recap.waitFor({ state: "hidden" });
    const completed = page.waitForResponse(response => new URL(response.url()).pathname === "/api/sessions/-1");
    release.resolve();
    await (await completed).finished();
    // Allow the old response body to be decoded and any resulting render to occur.
    await page.evaluate(() => new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve))));
    assert.equal(await recap.count(), 0, "the delayed previous review must not repaint the recap");
    assert.deepEqual(errors, []);
    console.log("recap-refresh: 1 passed, 0 failed");
  } finally {
    await browser.close();
  }
}

main().catch(error => {
  console.error(error);
  process.exitCode = 1;
});
