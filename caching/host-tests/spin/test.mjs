// Runs the component built from this crate on Spin and checks the spin
// feature's store. Build the component first with
// `cargo build --target wasm32-wasip2 --release`. Spin is run as `spin`, or
// from the _51DEGREES_SPIN environment variable. Exits non-zero when a check fails.

import assert from "node:assert/strict";
import { spawn, spawnSync } from "node:child_process";
import { copyFileSync, mkdirSync, rmSync } from "node:fs";
import { join } from "node:path";

const target = process.env.CARGO_TARGET_DIR ?? "target";
mkdirSync("build", { recursive: true });
copyFileSync(
  join(target, "wasm32-wasip2", "release", "caching_spin_host_test.wasm"),
  "build/component.wasm",
);
// Each run starts with an empty key-value store.
rmSync(".spin", { recursive: true, force: true });

const address = "127.0.0.1:3077";
// Spin runs its trigger as a child process, so it gets a process group of
// its own and the whole group is stopped at the end. Its output is kept
// and shown only when a check fails.
const spin = spawn(process.env._51DEGREES_SPIN ?? "spin", ["up", "--listen", address], {
  stdio: ["ignore", "pipe", "pipe"],
  detached: process.platform !== "win32",
});
let spinOutput = "";
spin.stdout.on("data", (data) => (spinOutput += data));
spin.stderr.on("data", (data) => (spinOutput += data));

function stopSpin() {
  if (process.platform === "win32") {
    spawnSync("taskkill", ["/pid", String(spin.pid), "/t", "/f"]);
  } else {
    process.kill(-spin.pid, "SIGKILL");
  }
}

async function text(path) {
  const response = await fetch(`http://${address}${path}`);
  return response.text();
}

async function started() {
  for (let attempt = 0; attempt < 150; attempt += 1) {
    try {
      await text("/");
      return;
    } catch {
      await new Promise((resolve) => setTimeout(resolve, 200));
    }
  }
  throw new Error("Spin did not start listening");
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

try {
  await started();

  await check("the store writes, reads and removes bytes", async () => {
    assert.equal(await text("/kv"), "ok");
  });

  await check("an entry past its drop time misses and is removed", async () => {
    assert.equal(await text("/expired"), "ok");
  });

  await check("a sweep removes only the expired entry", async () => {
    assert.equal(await text("/sweep"), "ok");
  });

  await check("a second request reads the store rather than loading", async () => {
    const first = await text("/cached?key=a");
    assert.match(first, /^a loaded at \d+$/);
    assert.equal(await text("/cached?key=a"), first);
  });
} catch (error) {
  failed += 1;
  console.log(`FAILED ${error.message}`);
} finally {
  stopSpin();
}

if (failed > 0) {
  console.log(spinOutput);
}
console.log(failed === 0 ? "all checks passed" : `${failed} checks failed`);
process.exitCode = failed === 0 ? 0 : 1;
