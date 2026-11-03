# Architecture

![51Degrees](https://raw.githubusercontent.com/51Degrees/common-ci/main/images/logo/360x67.png "Data rewards the curious")
**Pipeline API**

[Developer Documentation](https://51degrees.com/documentation/index.html?utm_source=github&utm_medium=docs&utm_campaign=51degrees-rust&utm_content=architecture.md&utm_term=documentation)

This Cargo workspace is a Rust implementation of the 51Degrees libraries, built
to the 51Degrees specification. A customer can run cloud or on-premise Device
Detection and IP Intelligence, in console or web (axum) applications, with full
example coverage, docs and tests.

## Layering

Crates are arranged in dependency order. Each layer depends only on the layers
above it. The diagram reads top to bottom, with the two products (Device
Detection and IP Intelligence) running as parallel columns that share the core,
engine, native and web layers.

```
core            fiftyone-pipeline-core, fiftyone-caching
  |
engines         fiftyone-pipeline-engines
  |             fiftyone-pipeline-engines-fiftyone
  |             fiftyone-cloud-request-engine
  |             fiftyone-derived-properties
  |             fiftyone-json-builder, fiftyone-javascript-builder
  |
sys / native    fiftyone-common-sys
  |             fiftyone-device-detection-sys, fiftyone-ip-intelligence-sys
  |             fiftyone-native (safe RAII wrapper)
  |
products        device-detection-{shared,onpremise,cloud}
  |             ip-intelligence-{shared,onpremise,cloud}
  |
facades         fiftyone-device-detection, fiftyone-ip-intelligence
  |
web             fiftyone-pipeline-web -> fiftyone-pipeline-web-axum
  |
examples        examples-shared, device-detection-examples,
                ip-intelligence-examples, pipeline-examples
```

- **core** (`fiftyone-pipeline-core`, `fiftyone-caching`). FlowData, the
  FlowElement/Pipeline traits, immutable Evidence, ElementData, TypedKey,
  WeightedValue, errors and constants, plus the sharded-LRU cache and the
  loading cache. The caching crate's `pipeline` feature, on by default,
  brings `DataKeyedCache` and the crate's only dependency on the core, so
  with it off the caches build without the pipeline, for WebAssembly hosts
  included. Its `fastly`, `cloudflare`, `spin` and `tokio` features, off by
  default, add stores over those platforms' key-value stores and caches and
  spawners for their tasks, each pulling in the platform's SDK only on the
  target the platform runs.
- **engines** (`fiftyone-pipeline-engines`,
  `fiftyone-pipeline-engines-fiftyone`, `fiftyone-cloud-request-engine`,
  `fiftyone-derived-properties`, `fiftyone-json-builder`,
  `fiftyone-javascript-builder`). The aspect-engine layer, the
  51Degrees-specific elements (ShareUsage, SetHeaders, Sequence and the
  metadata model), the cloud request engine, the derived property element and
  the JSON and JavaScript builder elements.

  `fiftyone-derived-properties` computes one property from properties earlier
  elements have already produced, following a shared script format that every
  language implementation reads, so one script gives the same answer
  everywhere. The scripts are compiled in rather than read from disk, because
  the element has to run where there is no filesystem.
- **sys / native FFI** (`fiftyone-common-sys`,
  `fiftyone-device-detection-sys`, `fiftyone-ip-intelligence-sys`,
  `fiftyone-native`). Raw `extern "C"` bindings to the native common,
  device detection and IP Intelligence C libraries in the `cxx` submodule
  (51degrees-cxx), plus a safe RAII wrapper shared by both products. This
  native path builds in parallel to the
  pure-Rust path and rejoins at the on-premise wrappers.
- **products: shared / on-prem / cloud** (`*-shared`, `*-onpremise`, `*-cloud`
  for both device-detection and ip-intelligence). The product data traits with
  typed and weighted accessors, the on-premise engines over `fiftyone-native`,
  and the cloud engines that map cloud JSON to the data traits.
- **facades** (`fiftyone-device-detection`, `fiftyone-ip-intelligence`).
  Re-exports plus a builder that selects the cloud or on-premise deployment.
  Both populate the same data type under the same key, so swapping deployment
  does not change the result-reading code.
- **web** (`fiftyone-pipeline-web`, `fiftyone-pipeline-web-axum`). The
  framework-neutral web elements and client-side endpoint logic, with axum as
  the reference adapter. The axum crate mounts `GET /51Degrees.core.js` and
  `POST /51dpipeline/json`, runs detection in tower middleware, and exposes the
  processed result to handlers through a `FiftyOneResult` extractor.
- **examples** (`examples-shared`, `examples/device-detection-examples`,
  `examples/ip-intelligence-examples`, `examples/pipeline-examples`). Shared
  example helpers and the runnable example set.
  Each example is a self-contained `src/bin/<name>.rs` with a `run` function, a
  `main`, a `#[cfg(test)]` test and a descriptive comment block at the bottom
  that the 51Degrees web docs are generated from.

The `fodid` crate (a 51Did/OWID reader) sits alongside this stack and is
independent of it.

## Feature gating

The pure-Rust crates never touch the linker, so cloud-only users and most of CI
build without a C toolchain. Cargo features gate the native path. The
`device-detection` and `ip-intelligence` facades default to
`["cloud", "on-premise"]`, and a cloud-only consumer can build with
`--no-default-features --features cloud` to drop the FFI crates entirely.

## Native co-link symbol-namespacing

Device Detection and IP Intelligence each embed their own copy of the common
C library (`src/common` in 51degrees-cxx), the shared C base layer. When both
on-premise engines are linked into one binary (which the workspace supports, so
a single program can do DD and IPI), those two copies define the same common
symbols and the linker sees a duplicate-symbol collision.

The fix is applied entirely on the IP Intelligence side, so Device Detection is
untouched. `fiftyone-ip-intelligence-sys` force-includes a header
(`src/symbol_prefix.h`) into every C and C++ translation unit it compiles, via
`/FI` on MSVC and `-include` on the GNU and Clang drivers. That header renames
this crate's private common symbols to an `ipi_*` namespace at preprocess
time, so both the definitions compiled here and every call site that references
them are renamed together. Device Detection keeps the unprefixed names. The two
copies of the common library therefore no longer collide.

On the Rust side, the affected FFI declarations bind to the prefixed names with
`#[link_name = "ipi_..."]` (for example `ipi_fiftyoneDegreesResourceManagerFree`
and `ipi_fiftyoneDegreesExceptionGetMessage`). The header carries the full
symbol list and the note on how to regenerate it.

## IP Intelligence data strategy

The IP Intelligence data files come in two tiers. The example data-path
resolver in `examples-shared` (`ipi_data_path(IpiTier)`) encodes the strategy:

- **ASN** (`51Degrees-IPIV4AsnIpiV41.ipi`). Small, current and checked into the
  `ip-intelligence-data` repository (checked out inside the `cxx`
  submodule). It loads against the current native library and is the default
  offline tier, suitable for quick tests.
- **Enterprise** (`51Degrees-IPIV4EnterpriseIpiV41.ipi`, on the dated production
  UNC share). The full-accuracy file, reachable only from the 51Degrees network.
  The resolver walks the `YYYY/MM/DD` share layout for the latest dated folder
  that actually contains the file, and treats any I/O error (the common case off
  the network) as "not reachable" rather than blocking or panicking.

A full IP Intelligence data file is downloaded from the data repository (see its
README); the file committed to the repository for convenient testing is the ASN
file.

The default, `IpiTier::BestAvailable`, picks the best *loadable* tier:
Enterprise when the production share is reachable, otherwise ASN. The
`51DEGREES_IPI_PATH` environment variable, when set, overrides the tier
selection and points every example at one explicit file. On-premise examples
also print the more-than-28-days-old data-file warning through
`examples_shared::check_data_file`.

## JavaScript minification opt-in

The Rust `fiftyone-javascript-builder` leaves JavaScript minification **off by
default** and gates it behind a `minify` Cargo feature. The current Rust
minifier (`minify-js` 0.6.0) raises an internal assertion on the
promise/fetch conditional expressions in the generated template, so it always
falls back to the unminified output. Rather than ship a default that does no
useful work, the element emits the correct, unminified template output, and the
builder's `set_minify(true)` flag is honoured only when the feature is enabled
(with the same fall-back-on-error behavior, so a request never crashes).
Integrating a robust minifier so this can be on by default is a tracked
follow-up.
