// Runs the Worker built from this crate on workerd, the Workers runtime,
// through Miniflare, and checks the cloud request engine's request path
// there. Build the Worker first with `worker-build --release`, and install
// Miniflare with `npm install --no-save miniflare@4.20260730.0`. Exits
// non-zero when a check fails.

import assert from "node:assert/strict";
import { Miniflare } from "miniflare";

const mf = new Miniflare({
  modules: true,
  scriptPath: "build/index.js",
  modulesRules: [{ type: "CompiledWasm", include: ["**/*.wasm"], fallthrough: true }],
  compatibilityDate: "2026-07-01",
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

await check("a request returns the cloud's JSON", async () => {
  assert.equal(await text("/process"), "ok");
});

await check("a request whose transport fails is reported as an error", async () => {
  assert.equal(await text("/failure"), "ok");
});

await mf.dispose();
console.log(failed === 0 ? "all checks passed" : `${failed} checks failed`);
process.exitCode = failed === 0 ? 0 : 1;
