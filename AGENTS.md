# AGENTS.md

A shared, tool-neutral handover for anyone (human or AI agent) working in this
repository. Keep it short and current: record the things that are not obvious
from the code or git history, and update it as the project changes. It is not
tied to any one assistant.

## What this is

The 51Degrees Rust SDK: a single Cargo workspace of libraries plus a separate
workspace of runnable examples.

- **Library workspace** (repository root): the pipeline core and engines,
  Device Detection and IP Intelligence (cloud and on-premise), the cloud
  request engine, the JSON and JavaScript builders, the axum web integration,
  and the `fodid` 51Did reader. Each concern is its own crate so a consumer
  depends only on what it uses.
- **Example workspace** (`examples/`): excluded from the root workspace
  (`exclude = ["examples"]`). It depends on the **published** crates from
  crates.io by default, so a fresh checkout of `examples/` builds the way a
  real consumer's project would. See [examples/](examples/) below.

The three `-sys` crates (`common-sys`, `device-detection-sys`,
`ip-intelligence-sys`) link the 51Degrees C sources, which come from the `cxx`
submodule at the root of the repository
(https://github.com/51Degrees/51degrees-cxx). The submodule's data submodules,
`cxx/device-detection-data` and `cxx/ip-intelligence-data`, hold the Lite data
files and the evidence the tests and examples read. The submodule pointer
records which C commit the crates build, and the nightly pipeline moves it
with an update pull request when 51degrees-cxx changes. After a move, build
and run `pwsh ./ci/check-native-symbols.ps1`, which fails when a common C
function is missing from `ip-intelligence-sys/src/symbol_prefix.h`.

## Building and testing

Once per clone, and again whenever the `owid-rust` submodule moves, check out
the submodules and place the OWID source that `fodid` compiles in (see
"Releasing to crates.io" below for why it is carried this way). The data files
are Git LFS content, so pull them too:

```
git submodule update --init --recursive
git -C cxx submodule foreach --recursive 'git lfs pull'
pwsh ./ci/copy-owid-source.ps1
```

Then, from the repository root:

```
cargo build --workspace --all-targets
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo doc --workspace --no-deps
```

CI builds with `RUSTFLAGS=-D warnings` and `RUSTDOCFLAGS=-D warnings`, so any
warning fails the build. Match that locally before pushing.

Each `-sys` `build.rs` resolves its C sources in this order: the
`FIFTYONE_CXX_DIR` environment variable, pointing at a 51degrees-cxx checkout,
then the `cxx` submodule, then the `cxx` folder inside the crate, which only a
packaged crate has (see "Releasing to crates.io" below).

On-premise tests and examples need a data file. They find the Lite files in
the `cxx` submodule, and skip cleanly when no file is present, so the offline
build and tests stay green without data. To run them against another file,
point the relevant variable at it: `51DEGREES_DD_PATH` for a Device Detection
`.hash` file, `51DEGREES_IPI_PATH` for an IP Intelligence `.ipi` file. Live
cloud tests are `#[ignore]`d and read a resource key from
`51DEGREES_RESOURCE_KEY` (or the CI tiered names `_51DEGREES_RESOURCE_KEY_BESPOKE`
then `_51DEGREES_RESOURCE_KEY_BASIC`).

## examples/

The example crates form their own workspace so they can default to the
published crates while the library workspace builds from source.

- **Default (crates.io):** `cd examples && cargo run --bin <name>` resolves the
  released `fiftyone-*` packages from crates.io.
- **Local source:** `pwsh ci/test-examples-against-source.ps1` builds and tests
  the examples against the working tree, which is what the examples workflow
  runs. `examples/source.toml` maps every `fiftyone-*` crate to its local path
  through `[patch.crates-io]`, but Cargo only takes a patch whose version is the
  one it would pick, so a plain `cargo test --config source.toml` builds the
  published crates, which are newer than the 4.5.2 in the manifests. The script
  gives the crates a version above the published ones for the run, moves the
  examples' lock to the local copies, fails if any still comes from crates.io,
  and puts the files back afterwards. The patch paths in `examples/source.toml`
  are relative to the **repository root**, not to `examples/`.

Example binaries live in `<crate>/src/bin/*.rs` (run with `--bin`), not in an
`examples/` directory.

## Releasing to crates.io

Releases are automated, as in the other languages. The nightly pipeline at
01:00 UTC merges the eligible pull requests and then publishes `main` through
common-ci's shared publish when it has moved past the last release tag, so a
night's merges go out as one patch release. `.github/workflows/publish.yml`
runs the same publish by hand. The version is never committed. The shared
publish works it out from the release tags with GitVersion and common-ci's
`GitVersion.yml`, which gives `main` the next plain patch, such as 4.5.10 after
4.5.9, and refuses a major or minor move unless a person dispatches
`publish.yml` with `allow-version-bump`. The `ci/*.ps1` publish hooks write
the version into every manifest on the build machine, copy the OWID source
into `fodid` and the C sources into the `-sys` crates, test the examples
against the crates at that version, package every crate, and publish each to
crates.io in dependency order, skipping any version already published and
waiting for the crates.io index to list each one before the next. The shared
publish then tags the commit and creates the GitHub release.
`ci/missing-ci-scripts.md` describes the hooks.

Authentication uses crates.io **Trusted Publishing**. The publish exchanges the
run's GitHub OIDC identity for a short-lived token, so no API token is stored.
Each crate has this repository's `nightly-pipeline.yml` and `publish.yml`
registered as trusted publishers (one-time setup in
`ci/setup-trusted-publishing.ps1`).

The OWID library the `fodid` crates build on is not a crates.io dependency.
Its source is compiled into `fodid` as a private module from the `owid-rust`
submodule (https://github.com/SWAN-community/owid-rust) by
`ci/copy-owid-source.ps1`, which every workflow runs before building, so no
OWID crate has to exist on any registry. The copied directory `fodid/src/owid`
is ignored by git and listed in the `fodid` manifest's `include`, so
`cargo publish` packages it.

The `-sys` crates carry their C sources the same way. Cargo will not package
files from outside a crate's folder, so before publishing
`ci/copy-cxx-sources.ps1` copies the `.c` and `.h` files each build script
compiles from the `cxx` submodule into a `cxx` folder inside the crate,
keeping the `src/` layout because the C files include each other by relative
path. Each copy is ignored by git, listed in the crate manifest's `include`,
and records the 51degrees-cxx commit in `cxx/SOURCE.txt`. A build in the
workspace reads the submodule and never needs the copy.

## CI gates

- `pull-request.yml` — fmt, build, test, clippy and doc on Windows, Linux
  (two LTS) and macOS (Intel and Apple Silicon), plus a wasm32-wasip1 job for
  the cloud crates. Runs on pushes to `main` and PRs targeting `main`. It
  builds the C sources and reads the data files in the `cxx` submodule. It
  also runs `ci/check-native-symbols.ps1` and the property lookup scaling
  test in release.
- `caching-wasm.yml` lints and tests `fiftyone-caching`, the loading cache
  included, on wasm32-wasip1 under wasmtime and, with default features off,
  on wasm32-unknown-unknown under Node through `ci/run-wasm-unknown.mjs`.
  Each host feature has a job too. `fastly` is tested on Viceroy, Fastly's
  local runtime. `cloudflare` runs the Worker in
  `caching/host-tests/cloudflare` on workerd through Miniflare. `spin` runs
  the component in `caching/host-tests/spin` on Spin, and that job also
  tests the crate on wasm32-wasip2 under wasmtime. Same triggers, and it
  reads no secrets.
- `examples.yml` — builds and tests `examples/` against local source
  (`source.toml`) before a release. Runs on every PR.
- `nightly-pipeline.yml` runs at 01:00 UTC. It first runs common-ci's submodule
  update, which moves every submodule to the head of its default branch and
  raises a pull request when one has moved. That covers `owid-rust` as well as
  `cxx`, so a push to the main branch of SWAN-community/owid-rust reaches
  `fodid` through the next update pull request once its checks pass. It then
  merges the eligible pull requests, publishes `main` through common-ci's
  shared workflows, and builds and tests `examples/` against the crates.io
  packages.
- `publish.yml` — the same publish, dispatched by hand, the only route that can
  allow a major or minor version move.
- `utm-link-lint.yml` — see UTM conventions below. Runs on every PR.
- `mustache-drift.yml` — guards the generated JavaScript resource template.
  Push and pull request builds compare against a pinned upstream commit
  rather than the branch, because the canonical template is being changed by
  the create last programme in the cloud service, so comparing against the
  branch would fail work here that has nothing to do with that programme.
  The weekly schedule and a manual run still compare against the branch, so
  a real drift is still reported. 51Degrees/51degrees-rust#52 lifts the pin, by moving
  the bundled asset and `UPSTREAM_PINNED_REF` to the same commit in one pull
  request.
- `nightly-performance.yml` — nightly (and on-demand) on-premise throughput
  run that feeds the documentation performance graphs. Its adapter,
  `ci/run-performance-tests.ps1`, runs the performance examples against this
  checkout's crates through `ci/test-examples-against-source.ps1`. Each
  example writes its own results JSON, and common-ci provides the steps that
  validate and compare it (`steps/publish-performance-results.ps1` and
  `steps/compare-performance.ps1`). See
  [ci/missing-ci-scripts.md](ci/missing-ci-scripts.md) for how the repository
  uses the shared reusable workflows.

## Conventions and gotchas

- **Generated accessors.** The strongly typed property accessors in
  `device-detection-shared/src/device_data.rs` and
  `ip-intelligence-shared/src/ip_intelligence_data.rs` are generated by the
  `PropertyGenerator` tool in the 51Degrees `tools` repository. Do not edit
  them by hand; change the generator.
- **Feature unification with examples.** The library crates' default features
  are cloud + on-premise; `reqwest-client` is off by default. Some tests are
  gated on `reqwest-client` and rely on it being enabled to compile. Because
  `examples/` is a separate workspace, the root `cargo build --workspace` no
  longer unifies `reqwest-client` on, so gate test-only imports and helpers on
  the exact feature whose test uses them, or `-D warnings` will fail on unused
  code.
- **UTM links.** Links to `51degrees.com` in source, docs and READMEs must
  carry the five UTM parameters in order (`utm_source`, `utm_medium`,
  `utm_campaign`, `utm_content`, `utm_term`), with `utm_campaign=51degrees-rust`. The
  lint config is `.utm-lint.json`; `Cargo.toml` `homepage`/`repository`
  metadata URLs are allow-listed as untagged.
- **Code style.** Wrap code and doc comments to about 80 columns (soft limit;
  do not break URLs or string literals to hit it). This keeps diffs reviewable.
- **Caching on WebAssembly.** `fiftyone-caching` must keep building and
  running on wasm32-unknown-unknown with default features off, where the
  standard library has no clock and no randomness. Only its `pipeline`
  feature may bring in another crate from this workspace, `ahash` takes a
  build-time seed on wasm targets, and the loading cache takes every time
  from its `Clock`, never from `Instant` or `SystemTime::now`.
- **Caching host stores.** The `fastly`, `cloudflare` and `spin` modules
  compile only with their feature on their own target, and their SDKs are
  dependencies on that target alone, so a native build with every feature
  leaves them out. Logic they can share without the SDK sits outside the
  binding and is unit tested on every target. The crates in
  `caching/host-tests` are test harnesses with workspaces of their own.
  On Cloudflare the runtime stops a request that waits with nothing of its
  own pending, and refuses I/O a request makes in another request's turn.
  `cloudflare::with_context` handles both, and only the workerd job proves
  a change there.
- **MSRV.** The workspace pins `rust-version = "1.94"` (the JavaScript builder's
  minifier toolchain needs a recent compiler).

## 51Did terminology

When working on `fodid`, keep the three levels distinct: the **51Did** is the
identifier as a whole; the **envelope** (also called the wrapper) is the signed
OWID that carries it and changes on every issue; the **match key** is the
stable, comparable part of the payload, read through `FodId::match_key`.
Compare match keys, never envelopes. The payload offsets and lengths are
internal to the crate, so every field is read through its typed accessor,
and the layout is specified at
https://github.com/51Degrees/specifications/blob/main/did-specification/identifier-layout.md
