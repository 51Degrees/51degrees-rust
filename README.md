# 51Degrees Device Detection and IP Intelligence for Rust

<img src=".github/images/logo.png" alt="51Degrees" title="Data rewards the curious" width="160">

Our User-Agent parser for Rust supports Device Detection, browser detection, mobile detection, and crawler detection on-premise or in the cloud. IP Intelligence adds IP geolocation, including IP to country data.

It reads User-Agent and User-Agent Client Hints, HTTP request headers that carry browser and device information. For cloud bot detection, Human Confidence reports confidence that a person is viewing the page.

## Install

Each product is a separate crate on crates.io. For Device Detection and IP Intelligence, cloud or on-premise:

```sh
cargo add fiftyone-device-detection
cargo add fiftyone-ip-intelligence
```

The crates below list the others. To build from source, see [Building](#building).

The crates of one release require each other at exactly that release's version, so each crate builds with the 51Degrees crates and the C library of its own release, and `cargo update` moves them together. Where two of a project's 51Degrees crates share a crate beneath them, Cargo takes both from one release, even if that means an older version of one, and stops with an error only when the project's requirements allow no single release, such as two crates pinned with `=` to different releases. Running `cargo update` with one crate named leaves that crate at the release of the crates locked beside it. Pinning a crate with `=` therefore also pins the crates beneath it, and the C library they build.

Try the [User-Agent tester](https://51degrees.com/developers/user-agent-tester?utm_source=github&utm_medium=readme&utm_campaign=51degrees-rust&utm_content=readme.md&utm_term=install) and [IP tester](https://51degrees.com/developers/ip-tester?utm_source=github&utm_medium=readme&utm_campaign=51degrees-rust&utm_content=readme.md&utm_term=install). For cloud use, [create a Resource Key](https://configure.51degrees.com/?utm_source=github&utm_medium=readme&utm_campaign=51degrees-rust&utm_content=readme.md&utm_term=install).

This repository was renamed from `rust` and continues that work.

### Example

Prepare the data file and build requirements described below before running this example.

```rust
use fiftyone_ip_intelligence::{
    IpIntelligencePipelineBuilder, PerformanceProfile, IP_DATA_KEY,
};

// Use an on-premise IP Intelligence data file. The free ASN file from the data
// repository suits quick tests. An Enterprise file has the full property set.
let pipeline = IpIntelligencePipelineBuilder::on_premise("51Degrees-IPIV4AsnIpiV41.ipi")
    .performance_profile(PerformanceProfile::HighPerformance)
    // Off because the free ASN file has no licence key for updates.
    .auto_update(false)
    .build()?;
```

**Pipeline API**

[Developer Documentation](https://51degrees.com/documentation/index.html?utm_source=github&utm_medium=readme&utm_campaign=51degrees-rust&utm_content=readme.md&utm_term=documentation)

## Introduction

Our libraries form a Cargo workspace for console and axum web applications, with examples, documentation, and tests.

The
[specification](https://github.com/51Degrees/specifications)
is the source of truth for the concepts and design of this API and is recommended
reading. See [ARCHITECTURE.md](ARCHITECTURE.md) for the layering, the native
co-link symbol-namespacing, the IP Intelligence data strategy and the
minification opt-in.

## Crates

The library workspace has 29 members and the examples workspace 7, listed
below in dependency order from the pure-Rust core up to the runnable examples.

### Core

| Crate | Responsibility |
|-------|----------------|
| [`fiftyone-pipeline-core`](pipeline-core) | FlowData, the FlowElement/Pipeline traits, immutable Evidence, ElementData, TypedKey, WeightedValue, errors and constants. |
| [`fiftyone-caching`](caching) | Sharded-LRU cache trait and the default implementation used to wrap an engine, and the loading cache that loads a missing value once over pluggable stores, for native and WebAssembly hosts, with optional stores for Fastly, Cloudflare and Spin and a tokio spawner. |

### Engines and builders

| Crate | Responsibility |
|-------|----------------|
| [`fiftyone-pipeline-engines`](pipeline-engines) | AspectEngine/AspectData, AspectPropertyValue, missing-property and data-update services, and cache wiring. |
| [`fiftyone-pipeline-engines-fiftyone`](pipeline-engines-fiftyone) | ShareUsage, SetHeaders and Sequence elements plus the FiftyOne metadata model. |
| [`fiftyone-cloud-request-engine`](cloud-request-engine) | Cloud HTTP engine that resolves the accepted evidence keys and accessible properties at build time (with a persistable state for short-lived hosts) and recovery mode. |
| [`fiftyone-json-builder`](json-builder) | JSON serialization element. |
| [`fiftyone-javascript-builder`](javascript-builder) | JavaScript snippet builder element with the bundled Mustache asset. Minification is an opt-in feature (see below). |
| [`fiftyone-agent-signature`](agent-signature) | Agent Signature element, which checks the HTTP Message Signature an automated agent sends under the IETF Web Bot Auth protocol. |
| [`fiftyone-derived-properties`](derived-properties) | Derived property element, which computes a property from properties earlier elements have already produced, using the shared script format every language implementation reads. |
| [`fiftyone-pipeline-translation`](pipeline-translation) | Translation engine, which translates the values another element produced into the language the request asks for, from YAML sources of `value: translation` lines. |

### Native FFI

These crates touch the C toolchain. They are pulled in only by the on-premise
path, so cloud-only users and most of CI build without a C compiler.

| Crate | Responsibility |
|-------|----------------|
| [`fiftyone-common-sys`](common-sys) | Raw FFI bindings to the 51Degrees common C library: ResourceManager, Evidence, Exception, StatusCode and result buffers. |
| [`fiftyone-device-detection-sys`](device-detection-sys) | Raw FFI bindings to the Hash device-detection ABI in the 51Degrees C library, built on fiftyone-common-sys. |
| [`fiftyone-ip-intelligence-sys`](ip-intelligence-sys) | Raw FFI bindings to the Ipi ABI in the 51Degrees C library, including the weighted getters. Compiles its own copy of the common C library into an `ipi_*` symbol namespace so it can co-link with device-detection (see ARCHITECTURE.md). |
| [`fiftyone-native`](native) | Safe RAII wrapper over the native manager, results, evidence and exception types, shared by both products. |

### Device Detection

| Crate | Responsibility | Deployment |
|-------|----------------|------------|
| [`fiftyone-device-detection-shared`](device-detection-shared) | DeviceData trait with typed accessors, the property model and the UACH high-entropy decoder element. | both |
| [`fiftyone-device-detection-onpremise`](device-detection-onpremise) | On-premise Hash engine over the safe fiftyone-native wrapper. | on-premise |
| [`fiftyone-device-detection-cloud`](device-detection-cloud) | Cloud engine that maps the cloud JSON response to DeviceData. | cloud |
| [`fiftyone-device-detection`](device-detection) | Facade: re-exports plus a builder that selects cloud or on-premise. `cloud` and `on-premise` are both default-on features. | both |

### IP Intelligence

| Crate | Responsibility | Deployment |
|-------|----------------|------------|
| [`fiftyone-ip-intelligence-shared`](ip-intelligence-shared) | IpIntelligenceData trait with weighted property accessors shared by both engines. | both |
| [`fiftyone-ip-intelligence-onpremise`](ip-intelligence-onpremise) | On-premise Ipi engine over the safe fiftyone-native wrapper. | on-premise |
| [`fiftyone-ip-intelligence-cloud`](ip-intelligence-cloud) | Cloud engine that maps the cloud JSON response to IpIntelligenceData. | cloud |
| [`fiftyone-ip-intelligence`](ip-intelligence) | Facade: re-exports plus a builder that selects cloud or on-premise. `cloud` and `on-premise` are both default-on features. | both |
| [`fiftyone-ip-intelligence-translation`](ip-intelligence-translation) | Country translation engines: the English names of the countries IP Intelligence gives, those names in the language the request asks for, and every country in that language's order. | both |

### Web

| Crate | Responsibility |
|-------|----------------|
| [`fiftyone-pipeline-web`](pipeline-web) | Framework-neutral web elements, client-side endpoint logic and the JavaScript/Mustache assets. |
| [`fiftyone-pipeline-web-axum`](pipeline-web-axum) | axum extractor and tower middleware adapter that mounts the two client-side endpoints (`/51Degrees.core.js` and `/51dpipeline/json`) and exposes a `FiftyOneResult` extractor. |

### Examples and standalone

| Crate | Responsibility |
|-------|----------------|
| [`examples-shared`](examples/examples-shared) | Shared example helpers: file-finder, resource-key-from-env, the DD/IPI data-path resolvers, the data-file-age warning, property-as-string and the sample evidence. |
| [`examples-web-shared`](examples/examples-web-shared) | Shared HTML and static-asset helpers for the web examples: the vendored design-system assets, asset-serving handlers, HTML escaping and the common table helpers. |
| [`device-detection-examples`](examples/device-detection-examples) | Runnable device-detection examples (cloud, on-premise and web). |
| [`ip-intelligence-examples`](examples/ip-intelligence-examples) | Runnable IP-intelligence examples (cloud, on-premise and web). |
| [`pipeline-examples`](examples/pipeline-examples) | Runnable pipeline examples: custom flow elements, caching, usage sharing and the combined-pipeline server-side examples. |
| [`fodid-examples`](examples/fodid-examples) | Runnable 51Did examples: the creator context web demo, which verifies a 51Did from the browser and redeems the encrypted verdict on the server. |
| [`examples-benches`](examples/benches) | Criterion micro-benchmarks guarding the DD, IPI and JavaScript-builder throughput budgets. |
| [`fodid`](fodid) | Standalone reader for the 51Did (51Degrees Identifier) returned by the cloud, described in the [identifiers documentation](https://51degrees.com/documentation/_identifiers__index.html?utm_source=github&utm_medium=readme&utm_campaign=51degrees-rust&utm_content=readme.md&utm_term=51did). It parses the OWID envelope and is independent of the pipeline stack. |
| [`fiftyone-fodid-core`](fodid-core) | The 51Did element data, property names, metadata and engine trait that any engine producing 51Dids is built on. It does not depend on the cloud request engine. |
| [`fiftyone-fodid-cloud`](fodid-cloud) | Cloud engine that maps the cloud response's `fodid` member to the 51Did element data, and re-exports `fiftyone-fodid-core`. |
| [`fodid-client`](fodid-client) | The server side of the 51Did two-step verification: fetches and caches the signing keys, verifies a signature offline or through the cloud, and redeems the sealed creator context result a browser relays, with the typed outcomes the other 51Did packages report. Builds without a network stack by default. The `reqwest-client` feature turns on the built-in transport. |

## Feature notes

- The Device Detection and IP Intelligence facades both default to
  `["cloud", "on-premise", "share-usage"]`. A cloud-only application can
  disable the `on-premise` feature to drop the native FFI crates and build
  without a C toolchain.
- Usage sharing is on by default in the facades' pipeline builders, as it is
  in the other languages. The default `share-usage` feature adds the
  share-usage element to both builders and compiles the built-in HTTP sender
  it posts with. A console or offline tool turns it off, as the console
  examples do, with `.share_usage(false)` or by leaving out the feature. With
  the feature off, the builders leave usage sharing off unless
  `.share_usage(true)` is called, and the element's sender is a no-op unless the
  `fiftyone-pipeline-engines-fiftyone/share-usage-transport` feature is
  enabled, so no network connection to 51Degrees is made. See the
  [usage sharing](https://51degrees.com/documentation/_pipeline_api__features__usage_sharing.html?utm_source=github&utm_medium=readme&utm_campaign=51degrees-rust&utm_content=readme.md&utm_term=usage-sharing)
  documentation for what is shared, and the `fiftyone-pipeline-engines-fiftyone`
  crate docs for the feature detail.
- Automatic data file updates are on by default in the facades' on-premise
  pipeline builders, as they are in .NET and Java. The pipeline owns a
  data update service that polls the 51Degrees data download service every thirty
  minutes, staggered by up to ten minutes, with the License Keys given through
  `.data_update_license_keys(..)`, and that refreshes the engine when the data
  file changes on disk. The data download service serves nothing without a License Key,
  so a build with polling on and no key fails with an error that says so, as
  it does in .NET, and a deployment of the free Lite file turns automatic
  updates off with `.auto_update(false)`, which stops the polling and the file
  watcher together, as the specification's master switch does.
  `.file_system_watcher(false)` turns only the watcher off, and
  `pipeline.resource::<DataUpdateService>()` reaches the service for a check
  on demand.
- JavaScript minification in `fiftyone-javascript-builder` is opt-in behind the
  `minify` feature and off by default. See ARCHITECTURE.md for why.

## Quick start

Both facade crates use the same steps. A builder picks the deployment. Create flow data, add evidence, process it, and read the typed result through a shared key.

A cloud build needs a Resource Key. Choose the properties you need and
[create a Resource Key](https://configure.51degrees.com?utm_source=github&utm_medium=readme&utm_campaign=51degrees-rust&utm_content=readme.md&utm_term=resource-key), and see the
[Resource Keys](https://51degrees.com/documentation/_services__cloud__resource_keys.html?utm_source=github&utm_medium=readme&utm_campaign=51degrees-rust&utm_content=readme.md&utm_term=resource-keys)
documentation for what a key grants.

### Cloud Device Detection

```rust
use fiftyone_device_detection::{
    DeviceData, DeviceDetectionPipelineBuilder, Evidence, DEVICE_DATA_KEY,
};

let pipeline = DeviceDetectionPipelineBuilder::cloud("YOUR_RESOURCE_KEY").build()?;

let mut data = pipeline.create_flow_data_with(
    Evidence::builder()
        .add("header.user-agent", "Mozilla/5.0 (iPhone; CPU iPhone OS 15_0 like Mac OS X) ...")
        .build(),
);
data.process()?;

if let Some(device) = data.get(DEVICE_DATA_KEY) {
    println!("IsMobile: {:?}", device.is_mobile().value());
    println!("BrowserName: {:?}", device.browser_name().value());
}
```

### On-premise Device Detection

```rust
use fiftyone_device_detection::{
    DeviceDetectionPipelineBuilder, Evidence, PerformanceProfile, DEVICE_DATA_KEY,
};

let pipeline = DeviceDetectionPipelineBuilder::on_premise("51Degrees-LiteV4.1.hash")
    .performance_profile(PerformanceProfile::HighPerformance)
    // Off because the free Lite file has no licence key for updates.
    .auto_update(false)
    .build()?;

let mut data = pipeline.create_flow_data_with(
    Evidence::builder().add("header.user-agent", "Mozilla/5.0 ...").build(),
);
data.process()?;

if let Some(device) = data.get(DEVICE_DATA_KEY) {
    println!("IsMobile: {:?}", device.is_mobile().value());
}
```

Requires the `on-premise` feature (default-on) and a C toolchain.

### Cloud IP Intelligence

```rust
use fiftyone_ip_intelligence::{
    IpIntelligencePipelineBuilder, IpIntelligenceData, Evidence, IP_DATA_KEY,
};

let pipeline = IpIntelligencePipelineBuilder::cloud("YOUR_RESOURCE_KEY").build()?;

let mut data = pipeline.create_flow_data_with(
    Evidence::builder().add("query.client-ip-51d", "185.28.167.77").build(),
);
data.process()?;

if let Some(ip) = data.get(IP_DATA_KEY) {
    // Each accessor returns an AspectPropertyValue wrapping a weighted list,
    // ordered most-probable first.
    println!("RegisteredCountry: {:?}", ip.registered_country().value());
}
```

### On-premise IP Intelligence

```rust
use fiftyone_ip_intelligence::{
    IpIntelligencePipelineBuilder, PerformanceProfile, IP_DATA_KEY,
};

// Use an on-premise IP Intelligence data file. The free ASN file from the data
// repository suits quick tests. An Enterprise file has the full property set.
let pipeline = IpIntelligencePipelineBuilder::on_premise("51Degrees-IPIV4AsnIpiV41.ipi")
    .performance_profile(PerformanceProfile::HighPerformance)
    // Off because the free ASN file has no licence key for updates.
    .auto_update(false)
    .build()?;
```

### axum web integration

```rust
use std::net::SocketAddr;
use axum::{routing::get, Router};
use fiftyone_device_detection::DeviceDetectionPipelineBuilder;
use fiftyone_pipeline_core::FlowElement;
use fiftyone_pipeline_web::{WebIntegrationOptions, WebPipeline};
use fiftyone_pipeline_web_axum::{register, FiftyOneResult, FiftyOneState};
use std::sync::Arc;

# async fn run() -> anyhow::Result<()> {
// Usage sharing is on by default, and is set here so the choice is visible.
// It helps improve 51Degrees data. The facade's pipeline is only used for its
// elements and is dropped at the end of this statement, because a usage
// sharing element can be in only one pipeline at a time. An on-premise
// facade's pipeline also runs its data file updates, so for on-premise data
// build the facade with .share_usage(false), keep its pipeline, and add a
// ShareUsageElement to the elements given to WebPipeline::build. The
// on-premise web example adds its ShareUsageElement to those elements in the
// same way, beside a Hash engine it builds without the facade.
let elements: Vec<Arc<dyn FlowElement>> =
    DeviceDetectionPipelineBuilder::cloud("YOUR_RESOURCE_KEY")
        .share_usage(true)
        .build()?
        .flow_elements()
        .to_vec();

// Hand the facade's elements to the web pipeline, which adds the sequence,
// set-headers, JSON and JavaScript elements on top.
let web = WebPipeline::build(elements, WebIntegrationOptions::default())?;
let state = FiftyOneState::from_web_pipeline(&web);

// register mounts GET /51Degrees.core.js and POST /51dpipeline/json and the
// per-request middleware. A handler reads the result through the extractor.
async fn home(result: FiftyOneResult) -> String {
    format!("had errors: {}", result.has_errors())
}
let app = register(Router::new().route("/", get(home)), state);

let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await?;
axum::serve(listener, app.into_make_service_with_connect_info::<SocketAddr>()).await?;
# Ok(())
# }
```

## Building

The `fodid` crate compiles the OWID library in from the `owid-rust` submodule,
so place that source once per clone before building.

```sh
git submodule update --init
pwsh ./ci/copy-owid-source.ps1
```

```sh
cargo build --workspace
cargo test --workspace --all-features
cargo doc --workspace --no-deps
```

A cloud-only build that skips the native path:

```sh
cargo build -p fiftyone-device-detection --no-default-features --features cloud
```

### Data files

The on-premise tests and examples read the free data files from the
[data repository](https://github.com/51Degrees/data), which the `cxx` submodule
holds at `cxx/data`. The files are not stored in git. Each one is listed as a
`.link` file in `cxx/data/device-detection` or `cxx/data/ip-intelligence`, and
the data repository's script downloads it. These commands get the two files the
tests read.

```sh
git submodule update --init --recursive
sh cxx/data/get-data-files.sh 51Degrees-LiteV4.1.hash 51Degrees-IPIV4AsnIpiV41.ipi
```

On Windows, run `cxx\data\get-data-files.ps1` with the same file names. An
on-premise example that cannot find its data file runs the script for that file
itself, and a test skips when its file is missing.

A clone made while 51degrees-cxx still has its `device-detection-data` and
`ip-intelligence-data` submodules holds those two folders inside `cxx`. Once
51degrees-cxx removes the two submodules, delete both folders, because git
leaves a removed submodule's folder on disk with the old data files in it.

```sh
rm -rf cxx/device-detection-data cxx/ip-intelligence-data
```

On Windows, run
`Remove-Item -Recurse -Force cxx\device-detection-data, cxx\ip-intelligence-data`.

### Running the examples

The examples are a Cargo workspace of their own in `examples`, which the root
workspace leaves out, so run them from that folder. They build against the
crates published on crates.io. Add `--config source.toml` to build them against
the crates in this checkout instead.

```sh
git clone --recurse-submodules https://github.com/51Degrees/51degrees-rust.git
cd 51degrees-rust/examples

# On-premise examples run with no settings. Each reads the free data files in
# cxx/data, downloading one that is missing, unless 51DEGREES_DD_PATH or
# 51DEGREES_IPI_PATH names another file.
cargo run -p device-detection-examples --bin dd-onprem-getting-started
cargo run -p ip-intelligence-examples --bin ipi-onprem-getting-started

# Cloud examples read 51DEGREES_RESOURCE_KEY from the environment.
cargo run -p device-detection-examples --bin dd-cloud-getting-started
cargo run -p ip-intelligence-examples --bin ipi-cloud-getting-started

# Web examples (cloud variant shown).
cargo run -p device-detection-examples --bin dd-web-getting-started-cloud
cargo run -p fodid-examples --bin fodid-web-creator-context
```

The free data files carry a small set of properties. Each on-premise example
also asks for properties only the Enterprise data file carries, such as
HardwareVendor, DeviceType and PriceBand for Device Detection, and Country,
Town and IsVPN for IP Intelligence. On a free file each of those says it needs
the Enterprise data file, and the output gives a link to contact 51Degrees
about a copy. The on-premise IP Intelligence examples read the free Lite file,
a download of about 320 MB that carries RegisteredName, RegisteredOwner and
RegisteredCountry. `ipi-onprem-performance` and `ipi-onprem-update-data-file`
read the ASN file instead, a download of a few megabytes, because their
properties do not matter to what they show, and `ipi-onprem-getting-started`
reads both, the ASN file for Asn and AsnName. Every example ends with a "Find
out more" section.

Every cloud example calls the public 51Degrees cloud at
`https://cloud.51degrees.com/api/v4/` unless `51DEGREES_CLOUD_ENDPOINT` names
another host. A host other than cloud.51degrees.com would be used to (a) use an
on premise web server, or (b) use a privately hosted version of the 51Degrees
cloud for performance reasons. This is the private hosting option of the
51Degrees cloud service. Both run the same service as the public cloud, so the
examples work unchanged. The variable is the same one the cloud request engine
reads, so setting it once points every example at the same place, and the
value is the API base including the `/api/v4/` segment.

```sh
51DEGREES_CLOUD_ENDPOINT=http://localhost:8080/api/v4/ cargo run -p ip-intelligence-examples --bin ipi-cloud-getting-started
```

#### Browser (Selenium) tests

Browser-driven verification of the web examples comes from the shared
[selenium-api-tests](https://github.com/51Degrees/selenium-api-tests) suite,
which every 51Degrees SDK reuses rather than maintaining its own browser
automation. Its `Contract` category drives headless Chrome against a running
example and checks that the page serves `51Degrees.core.js`, that client-side
evidence flows back, and that the server renders a real detection result.

CI runs it here in the `contract` job of the Examples workflow
(`.github/workflows/examples.yml`), through `ci/run-browser-contract.ps1`,
against both Device Detection web examples built from this checkout's crates
through `ci/test-examples-against-source.ps1`. The cloud example
(`dd-web-getting-started-cloud`) talks to the public cloud with the bespoke
Resource Key. The on-premise example
(`dd-web-getting-started-onprem`) loads the TAC data file, because the Lite
file has neither `DeviceType` nor the screen size JavaScript properties the
contract checks. A test that fails, or that the suite skips as inconclusive,
fails the job. Start it on any branch with
`gh workflow run Examples --ref <branch>`. The cloud service also runs the
cloud example against its own container, alongside the .NET, Java, Node,
Python and PHP examples.

To run the contract against an example that is already running, set
`EXAMPLE_URL` to its address, as the CI script does. To run it locally, check out `selenium-api-tests` as a sibling of this repo and
let the suite launch the example itself through its `rust` descriptor. Point
`CLOUD_ROOT_URL` at a cloud container rather than the public service, so the
result reflects the code and data you are testing:

```sh
cd ../selenium-api-tests
export CLOUD_ROOT_URL="http://localhost:8080/"   # a running cloud container
export PAID_RESOURCE_KEY="<your resource key>"
export EXAMPLE_LANG="rust"
dotnet test --filter TestCategory=Contract
```

The `fodid` crate compiles the OWID envelope library (the library a 51Did is
built on) into itself from the `owid-rust` submodule
(https://github.com/SWAN-community/owid-rust), so no OWID crate has to exist on
any registry. After cloning, run `git submodule update --init` and then
`pwsh ./ci/copy-owid-source.ps1` (PowerShell 7, on any platform) to place the
source under `fodid/src/owid`, which git ignores. Run the script again after
moving the submodule to another commit.

## Editor and IDE setup

rust-analyzer drives code intelligence in any LSP editor (VS Code, Neovim,
Helix, Zed), and the JetBrains RustRover IDE works out of the box.

For VS Code the workspace ships `.vscode/tasks.json` and `.vscode/launch.json`,
and `.vscode/extensions.json` recommends the two extensions they rely on:

- [rust-analyzer](https://marketplace.visualstudio.com/items?itemName=rust-lang.rust-analyzer)
  for build, completion and inline diagnostics.
- [CodeLLDB](https://marketplace.visualstudio.com/items?itemName=vadimcn.vscode-lldb)
  for debugging. The launch configurations use its `lldb` debug type and `cargo`
  integration, which resolve the built binary on Linux, macOS and Windows
  (including the Windows `.exe` suffix). If CodeLLDB is not installed, VS Code
  reports the launch configurations' debug type as unrecognized. Installing it
  clears that.

Opening the folder prompts to install the recommended extensions. Then run a
task with Terminal then Run Task (Ctrl+Shift+B for the default build), and pick a
configuration in the Run and Debug view to run or debug any example or test.

On first open, rust-analyzer indexes the whole workspace and runs the `-sys`
crate build scripts before code intelligence becomes active. Because of the size
of this repo this can take several minutes, during which the inline Run and Debug
lenses, completion and diagnostics will not appear yet. Wait for the
rust-analyzer status in the bottom status bar to finish (or check View then
Output then rust-analyzer Language Server) before expecting the buttons.

## License

EUPL-1.2. See [LICENSE](LICENSE).

## History

This repository was renamed from `rust` and continues that work. The old address redirects here, and the repository retains its history, stars, and issues.

## Find out more

- [Device Detection](https://51degrees.com/device-detection?utm_source=github&utm_medium=readme&utm_campaign=51degrees-rust&utm_content=readme.md&utm_term=find-out-more)
- [IP Intelligence](https://51degrees.com/ip-intelligence?utm_source=github&utm_medium=readme&utm_campaign=51degrees-rust&utm_content=readme.md&utm_term=find-out-more)
- [Bots and humans](https://51degrees.com/bots-and-humans?utm_source=github&utm_medium=readme&utm_campaign=51degrees-rust&utm_content=readme.md&utm_term=find-out-more)
- [Documentation](https://51degrees.com/documentation/index.html?utm_source=github&utm_medium=readme&utm_campaign=51degrees-rust&utm_content=readme.md&utm_term=documentation)
- https://github.com/51Degrees/51degrees-cxx
