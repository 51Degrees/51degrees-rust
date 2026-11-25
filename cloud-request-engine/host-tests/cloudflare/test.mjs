// Runs the Worker built from this crate on workerd, the Workers runtime,
// through Miniflare, against a stub cloud server this script serves on
// 127.0.0.1, and checks the cloud request engine's requests and results
// there. Build the Worker first with `worker-build --release`, and install
// Miniflare with `npm install --no-save miniflare@4.20260730.0`. Exits
// non-zero when a check fails.

import assert from "node:assert/strict";
import http from "node:http";
import { Miniflare } from "miniflare";

// The answers of the stub cloud server.
const EVIDENCE_KEYS = ["header.user-agent", "query.user-agent"];
const ACCESSIBLE_PROPERTIES = {
  Products: {
    device: {
      DataTier: "CloudV4",
      Properties: [
        { Name: "IsMobile", Type: "Bool", Category: "Device" },
        { Name: "HardwareVendor", Type: "String", Category: "Hardware" },
      ],
    },
  },
};
const DEVICE = { device: { ismobile: true, hardwarevendor: "Example Vendor" } };

// What the Worker sends, which the stub server checks.
const RESOURCE_KEY = "example-resource-key";
const USER_AGENT = "Mozilla/5.0 (example)";

// How long the slow route waits before answering, and the client timeout
// the timeout check gives the Worker, in milliseconds.
const SLOW_ANSWER_MS = 3000;
const SHORT_TIMEOUT_MS = 300;

// Every request the stub server received, in order.
const received = [];

function listen(server) {
  return new Promise((resolve) => server.listen(0, "127.0.0.1", () => resolve(server.address().port)));
}

const stub = http.createServer((req, res) => {
  let body = "";
  req.on("data", (chunk) => (body += chunk));
  req.on("end", () => {
    const url = new URL(req.url, "http://127.0.0.1");
    received.push({
      method: req.method,
      path: url.pathname,
      query: url.search,
      contentType: req.headers["content-type"] ?? null,
      body,
    });
    const answer = (status, value) => {
      res.writeHead(status, { "content-type": "application/json" });
      res.end(JSON.stringify(value));
    };
    if (url.pathname === "/api/v4/evidencekeys" && req.method === "GET") {
      answer(200, EVIDENCE_KEYS);
    } else if (url.pathname === "/api/v4/accessibleproperties" && req.method === "GET") {
      answer(200, ACCESSIBLE_PROPERTIES);
    } else if (url.pathname === "/api/v4/json" && req.method === "POST") {
      answer(200, DEVICE);
    } else if (url.pathname === "/slow/json") {
      setTimeout(() => answer(200, DEVICE), SLOW_ANSWER_MS);
    } else {
      answer(404, { errors: [`no route for ${req.method} ${url.pathname}`] });
    }
  });
});
const port = await listen(stub);
const cloud = `http://127.0.0.1:${port}/api/v4/`;

// A port nothing listens on, for a request that cannot complete.
const closed = http.createServer();
const closedPort = await listen(closed);
await new Promise((resolve) => closed.close(resolve));

const mf = new Miniflare({
  modules: true,
  scriptPath: "build/index.js",
  modulesRules: [{ type: "CompiledWasm", include: ["**/*.wasm"], fallthrough: true }],
  compatibilityDate: "2026-07-01",
});

async function report(params) {
  const query = new URLSearchParams(params).toString();
  const response = await mf.dispatchFetch(`http://localhost/?${query}`);
  const text = await response.text();
  console.log(`  worker ${response.status} ${text}`);
  return JSON.parse(text);
}

function show(requests) {
  for (const r of requests) {
    console.log(`  stub received ${r.method} ${r.path}${r.query} ${r.contentType ?? ""} ${r.body}`);
  }
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

await check("a request through the Workers fetch returns the device element data", async () => {
  received.length = 0;
  const result = await report({ cloud });
  show(received);
  assert.deepEqual(result, { stage: "done", ismobile: true, hardwarevendor: "Example Vendor" });
  assert.deepEqual(
    received.map((r) => `${r.method} ${r.path}`),
    ["GET /api/v4/evidencekeys", "GET /api/v4/accessibleproperties", "POST /api/v4/json"],
  );
  assert.equal(received[1].query, `?resource=${RESOURCE_KEY}`);
  const data = received[2];
  assert.equal(data.contentType, "application/x-www-form-urlencoded");
  const form = new URLSearchParams(data.body);
  assert.equal(form.get("resource"), RESOURCE_KEY);
  assert.equal(form.get("user-agent"), USER_AGENT);
});

await check("a data request that cannot complete is reported as an error", async () => {
  const result = await report({ cloud, data: `http://127.0.0.1:${closedPort}/api/v4/json` });
  assert.equal(result.stage, "process");
  assert.match(result.error, /failed to send request/);
});

await check("a data request that does not answer in time is reported as a timeout", async () => {
  const result = await report({
    cloud,
    data: `http://127.0.0.1:${port}/slow/json`,
    timeout_ms: String(SHORT_TIMEOUT_MS),
  });
  assert.equal(result.stage, "process");
  assert.match(result.error, /timed out after 300 ms/);
});

await mf.dispose();
stub.closeAllConnections();
stub.close();
console.log(failed === 0 ? "all checks passed" : `${failed} checks failed`);
process.exitCode = failed === 0 ? 0 : 1;
