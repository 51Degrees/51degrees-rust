// Runs the Worker built from this crate on workerd, the Workers runtime,
// through Miniflare, and checks the cloudflare feature's stores and
// spawner. Build the Worker first with `worker-build --release`, and
// install Miniflare with `npm install --no-save miniflare@4.20260730.0`.
// Exits non-zero when a check fails.

import assert from "node:assert/strict";
import { Miniflare } from "miniflare";

const mf = new Miniflare({
  modules: true,
  scriptPath: "build/index.js",
  modulesRules: [{ type: "CompiledWasm", include: ["**/*.wasm"], fallthrough: true }],
  compatibilityDate: "2026-07-01",
  kvNamespaces: ["CACHE"],
});

async function text(path) {
  const response = await mf.dispatchFetch(`http://localhost${path}`);
  return response.text();
}

let failed = 0;

async function check(name, run) {
  try {
    await run();
    console.log(`ok ${name}`);
  } catch (error) {
    failed += 1;
    console.log(`FAILED ${name}: ${error.message}`);
  }
}

await check("the KV store writes, reads and removes bytes", async () => {
  assert.equal(await text("/kv"), "ok");
});

await check("the Cache API store writes, reads and removes bytes", async () => {
  assert.equal(await text("/cache-api"), "ok");
});

await check("concurrent requests share one load", async () => {
  const values = await Promise.all(Array.from({ length: 5 }, () => text("/get?key=a")));
  assert.deepEqual(values, Array(5).fill("a from load 1"));
  assert.equal(await text("/loads"), "1");
});

await check("a load outlives the response of the request that started it", async () => {
  assert.equal(await text("/start?key=b"), "started");
  assert.equal(await text("/get?key=b"), "b from load 2");
  assert.equal(await text("/loads"), "2");
});

await check("a later request reads the KV store rather than loading", async () => {
  assert.equal(await text("/get?key=a"), "a from load 1");
  assert.equal(await text("/loads"), "2");
});

await mf.dispose();
console.log(failed === 0 ? "all checks passed" : `${failed} checks failed`);
process.exitCode = failed === 0 ? 0 : 1;
