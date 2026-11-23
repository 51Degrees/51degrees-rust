// Runs a wasm32-unknown-unknown test binary built by cargo, as the
// CARGO_TARGET_WASM32_UNKNOWN_UNKNOWN_RUNNER.
//
// That target has no operating system, so the standard library prints
// nothing and a panic aborts. The module is started with no imports and its
// exported main is called. A trap or a non-zero return is a failing test,
// which this reports through the exit status. A module that needs imports,
// such as wasm-bindgen glue, cannot run here and is reported as such.
import { readFileSync } from 'node:fs';

const file = process.argv[2];
const module = await WebAssembly.compile(readFileSync(file));
const imports = WebAssembly.Module.imports(module);
if (imports.length > 0) {
  console.error(`${file} needs imports this runner cannot give:`, imports);
  process.exit(2);
}
const instance = await WebAssembly.instantiate(module, {});
try {
  const status = instance.exports.main(0, 0);
  if (status !== 0) {
    console.error(`${file} returned ${status}`);
    process.exit(1);
  }
} catch (error) {
  console.error(`${file} failed: ${error.message}`);
  process.exit(1);
}
