# AGENTS.md

A shared, tool-neutral handover for anyone (human or AI agent) working in this
repository. Keep it short and current, recording the things that are not
obvious from the code or git history, and update it as the project changes. It
is not tied to any one assistant.

If you have been asked to get an example running, follow
[Run the examples](#run-the-examples). It gives every step in order, from a
fresh clone to the output.

## Run the examples

The two on-premise getting-started examples are the ones to show first,
because they need no key and run against free data files that come with the
repository. The cloud examples need a resource key.

### Prerequisites

- Git.
- Rust 1.94 or later, the `rust-version` of both workspaces.
- A C compiler, being the Visual Studio Build Tools on Windows and GCC or
  Clang elsewhere. The `-sys` crates compile the 51Degrees C sources with the
  `cc` crate, and `aws-lc-sys`, which the HTTPS client brings in, compiles C
  too. If `aws-lc-sys` fails to build, its
  [requirements](https://aws.github.io/aws-lc-rs/requirements/index.html) list
  what it needs on each platform.
- Network access to GitHub, crates.io and the 51Degrees Distributor, from
  which the data repository's script downloads the free data files.
- PowerShell 7 (`pwsh`) on Windows, or `sh` elsewhere, for that script, and
  `pwsh` to run the examples against this checkout.

Clone the repository with its submodules.

```sh
git clone --recurse-submodules https://github.com/51Degrees/51degrees-rust.git
cd 51degrees-rust
```

In a clone made without `--recurse-submodules`, fetch them.

```sh
git submodule update --init --recursive
```

### Data files

The examples read the free data files in the
[data](https://github.com/51Degrees/data) repository, which is at `cxx/data`.
It lists each file as a `.link` file, and its script downloads the files. An
on-premise example that cannot find its file runs that script for the one file
itself, so this step is only needed to download the files ahead of time.

```sh
sh cxx/data/get-data-files.sh 51Degrees-LiteV4.1.hash 51Degrees-IPIV4AsnIpiV41.ipi
```

On Windows, run `pwsh ./cxx/data/get-data-files.ps1` with the same names. The
IP Intelligence getting-started example also reads the Lite IP Intelligence
file, `51Degrees-LiteV41.ipi`, a download of about 320 MB, which it fetches
the first time it runs.

### Run

The examples are a Cargo workspace of their own in `examples/`, which the root
`Cargo.toml` excludes, so run them from there. From the root,
`-p device-detection-examples` matches no package.

```sh
cd examples
cargo run -p device-detection-examples --bin dd-onprem-getting-started
cargo run -p ip-intelligence-examples --bin ipi-onprem-getting-started
```

The first build compiles the 51Degrees C libraries, so it is the slow one.

### What correct output looks like

- `dd-onprem-getting-started` processes a desktop and a mobile User-Agent. For
  each it prints the evidence under "Input values:" and then, under
  "Results:", the properties the Lite file carries, such as Mobile Device,
  Platform Name and Browser Name. Each property only the Enterprise data file
  carries, such as Hardware Vendor and Device Type, reads "needs the
  Enterprise data file".
- `ipi-onprem-getting-started` reads the ASN file and then the Lite IP
  Intelligence file, and for `1.1.1.1` and `2606:4700:4700::1111` prints Asn
  and AsnName from the first and RegisteredName, RegisteredOwner and
  RegisteredCountry from the second. The location properties need the
  Enterprise data file.
- Both end by naming the properties the file in use lacks, with a link to ask
  about the Enterprise data file, and a "Find out more" section.

An example that cannot find or download its data file fails with the command
to run, so an exit code of 0 means it ran against a file.

### Paths and keys from the environment

The examples read `51DEGREES_DD_PATH` and `51DEGREES_IPI_PATH` for a data file
elsewhere, and the cloud examples read `51DEGREES_RESOURCE_KEY`, for which a
key comes from the
[Configurator](https://configure.51degrees.com/?utm_source=github&utm_medium=docs&utm_campaign=51degrees-rust&utm_content=agents.md&utm_term=configure).
These names start with a digit, so bash and sh refuse both `export
51DEGREES_RESOURCE_KEY=...` and the `51DEGREES_RESOURCE_KEY=... cargo run`
prefix. Use `env` there, and `$env:` in PowerShell.

```sh
env 51DEGREES_RESOURCE_KEY=<key> cargo run -p device-detection-examples --bin dd-cloud-getting-started
```

```powershell
$env:51DEGREES_RESOURCE_KEY = '<key>'
cargo run -p device-detection-examples --bin dd-cloud-getting-started
```

### Run them against this checkout

The examples depend on the crates published on crates.io, so by default they
show the last release and not a change made here. To run one against this
checkout, use the script CI uses, from PowerShell 7 at the repository root.
It needs the release tags, which a full clone has, and the OWID source that
the `fodid` crate compiles in, placed once as the README's "Building" section
says.

```powershell
./ci/copy-owid-source.ps1
./ci/test-examples-against-source.ps1 -Command { cargo run --config source.toml -p device-detection-examples --bin dd-onprem-getting-started }
```

The command runs in `examples/`, and with no `-Command` the script runs the
examples' tests instead. It puts the manifests and the lock back when it ends.

## What this is

The 51Degrees Rust SDK is a single Cargo workspace of libraries plus a separate
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
(https://github.com/51Degrees/51degrees-cxx). Its `data` submodule is the data
repository (https://github.com/51Degrees/data). It lists each free data file,
and the Device Detection evidence the examples read, as a `.link` file in
`cxx/data/device-detection` and `cxx/data/ip-intelligence`, and that
repository's script downloads them. The IP Intelligence evidence is stored in
it. Nothing here reads the older `cxx/device-detection-data` and
`cxx/ip-intelligence-data` folders. The submodule pointer
records which C commit the crates build, and the nightly pipeline moves it
with an update pull request when 51degrees-cxx changes. After a move, build
and run `pwsh ./ci/check-native-symbols.ps1`, which fails when a common C
function is missing from `ip-intelligence-sys/src/symbol_prefix.h`.

## Building and testing

Once per clone, and again whenever the `owid-rust` submodule moves, check out
the submodules and place the OWID source that `fodid` compiles in (see
"Releasing to crates.io" below for why it is carried this way). Then download
the data files the tests read with the data repository's script, which keeps a
file that is already there (`sh cxx/data/get-data-files.sh` takes the same
names, and `-u` or `-Update` replaces a file that has a newer version):

```
git submodule update --init --recursive
pwsh ./cxx/data/get-data-files.ps1 51Degrees-LiteV4.1.hash 51Degrees-IPIV4AsnIpiV41.ipi
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

On-premise tests and examples need a data file. They find the free files in
`cxx/data`, and the tests skip cleanly when no file is present, so the offline
build and tests stay green without data. An example that cannot find its file
runs the data repository's script for that one file, with the source
`examples-51degrees-rust`, and fails with the command to run if it still
cannot find it. To run them against another file,
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

## Releases

Pull requests go to `rc`, not `main`, and stay drafts until they have been
reviewed, because the nightly merges any pull request into `rc` that is not a
draft and has no review requested. When a release is wanted, one pull request
merges `rc` into `main`, and the "Only rc into main" check fails a pull request
into `main` from any other branch.

## Releasing to crates.io

Releases are automated, and every push to `main` publishes the crates that
changed. Each crate has its own version, tagged `<crate>/<version>` (for
example `fodid/4.5.19`), and no version is committed.
`.github/workflows/publish.yml` runs `ci/get-release-plan.ps1`, which applies
common-ci's `steps/release-plan.ps1` to `ci/packages.json`. A crate is
released at its last tag plus one patch when a file under its folder, or under
a submodule named in its `ExtraPaths`, changed since that tag, and every crate
that depends on it is released with it. A change only to CI, documentation or
examples releases nothing. `ci/set-crate-versions.ps1` then writes each
planned crate's version, and the version every other crate last released,
into the manifests and the requirements between them on the build machine.
The workflow refuses a crate that moves its major or minor number against its
own tags unless a person dispatches it with `allow-version-bump`, tests the
examples against this tree, publishes the planned crates in the order of
`ci/crates.txt` with `ci/publish-crates.sh`, skipping any version already
published, and tags each crate and gives it a GitHub release with
`ci/tag-crate-releases.ps1`. `release-plan.yml` prints the same plan on every
pull request and publishes nothing.

`ci/packages.json` lists each crate `ci/crates.txt` publishes, with its
folder, the submodules it is built from and the crates it depends on.
`ci/check-packages.ps1` fails when the file disagrees with the manifests, so a
dependency added to a manifest is added there too. A crate with no tag of its
own stops the publish, and `ci/start-crate-tags.ps1` is the script a person
runs once to tag every crate at the release it was last published from.

Authentication uses crates.io **Trusted Publishing**, where the publish workflow
exchanges the run's GitHub OIDC identity for a short-lived token, so no API
token is stored. Each crate has this repository and `publish.yml` registered as
a trusted publisher (one-time setup in `ci/setup-trusted-publishing.sh`).

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

- `pull-request.yml` runs fmt, build, test, clippy and doc on Windows, Linux
  (two LTS) and macOS (Apple Silicon), plus a wasm32-wasip1 job for the cloud
  crates, on pushes to `main` and on PRs into `main`, `rc` and `release/**`.
  It builds the C sources and restores the data files into `cxx/data` from
  the entry `data-files.yml` keeps in the Actions cache, downloading with the
  data repository's script only a file it lacks. It also runs
  `ci/check-native-symbols.ps1` and the property lookup scaling test in
  release.
- `data-files.yml` runs each day on `main` and keeps the free data files in
  one Actions cache entry. It restores the newest entry, asks the Distributor
  only whether each file has changed (`-Update`), and saves the files under a
  new dated key. `pull-request.yml`, `examples.yml` and
  `nightly-performance.yml` restore that entry through
  `.github/actions/restore-data-files`. They leave the daily entry to
  `data-files.yml`, because an entry saved by a pull request run can only be
  restored by that pull request. Until a daily entry
  exists, the action falls back to an entry keyed by the ref, the workflow
  and the date, which `pull-request.yml` and `examples.yml` save when they
  restored nothing, so later runs of the same workflow on the same ref that
  day download nothing. An entry is found only by the same list of paths it
  was saved with, so the list lives in that action alone.
- `nightly-pipeline.yml` first runs common-ci's submodule update, which moves
  every submodule to the head of its default branch and raises a pull request
  into `rc` when one has moved, then builds the eligible pull requests into
  `rc` through common-ci's nightly checks and merges each one that passes.
  That covers `owid-rust` as well as `cxx`, so a push to the main branch of
  SWAN-community/owid-rust reaches `fodid` through the next update pull
  request once its checks pass, and is published with the next merge of `rc`
  into `main`. It sets `cache-assets: true`, so common-ci runs
  `ci/fetch-assets.ps1` once before the builds and keeps the Lite and ASN
  files it fetches in the Actions cache. Each build runs the script again,
  which takes the files from that cache through common-ci's
  `steps/fetch-assets.ps1` and copies them into `cxx/data`.
- `caching-wasm.yml` lints and tests `fiftyone-caching`, the loading cache
  included, on wasm32-wasip1 under wasmtime and, with default features off,
  on wasm32-unknown-unknown under Node through `ci/run-wasm-unknown.mjs`.
  Each host feature has a job too. `fastly` is tested on Viceroy, Fastly's
  local runtime. `cloudflare` runs the Worker in
  `caching/host-tests/cloudflare` on workerd through Miniflare. `spin` runs
  the component in `caching/host-tests/spin` on Spin, and that job also
  tests the crate on wasm32-wasip2 under wasmtime. Same triggers, and it
  reads no secrets.
- `examples.yml` builds and tests `examples/` against local source
  (`source.toml`) before a release, on every PR.
- `publish.yml` publishes the crates that changed on push to `main`, then a
  `verify-published` job builds and tests `examples/` against the
  just-released crates.io packages.
- `release-plan.yml` checks `ci/packages.json` against the manifests and runs
  the Pester tests of the release scripts (`ci/packages.Tests.ps1`) on Linux,
  Windows and macOS, then prints the crates a release would publish, on every
  PR into `main` or `rc`.
- `utm-link-lint.yml` checks the UTM links described below, on every PR.
- `mustache-drift.yml` guards the generated JavaScript resource template.
  Push and pull request builds compare against a pinned upstream commit
  rather than the branch, because the canonical template is being changed by
  the create last programme in the cloud service, so comparing against the
  branch would fail work here that has nothing to do with that programme.
  The weekly schedule and a manual run still compare against the branch, so
  a real drift is still reported. 51Degrees/51degrees-rust#52 lifts the pin, by moving
  the bundled asset and `UPSTREAM_PINNED_REF` to the same commit in one pull
  request.
- `nightly-performance.yml` is the nightly (and on-demand) on-premise
  throughput run that feeds the documentation performance graphs. Its adapter,
  `ci/run-performance-tests.ps1`, runs the performance examples against this
  checkout's crates through `ci/test-examples-against-source.ps1`. Each
  example writes its own results JSON, and common-ci provides the steps that
  validate and compare it (`steps/publish-performance-results.ps1` and
  `steps/compare-performance.ps1`). See
  [ci/missing-ci-scripts.md](ci/missing-ci-scripts.md) for how this relates to
  the org-standard reusable-workflow contract and the planned direction.

## Conventions and gotchas

- **Generated accessors.** The strongly typed property accessors in
  `device-detection-shared/src/device_data.rs` and
  `ip-intelligence-shared/src/ip_intelligence_data.rs` are generated by the
  `PropertyGenerator` tool in the 51Degrees `tools` repository. Change the
  generator rather than editing them by hand.
- **Feature unification with examples.** The library crates' default features
  are cloud and on-premise, and `reqwest-client` is off by default. Some tests
  are gated on `reqwest-client` and rely on it being enabled to compile. Because
  `examples/` is a separate workspace, the root `cargo build --workspace` no
  longer unifies `reqwest-client` on, so gate test-only imports and helpers on
  the exact feature whose test uses them, or `-D warnings` will fail on unused
  code.
- **UTM links.** Links to `51degrees.com` in source, docs and READMEs must
  carry the five UTM parameters in order (`utm_source`, `utm_medium`,
  `utm_campaign`, `utm_content`, `utm_term`), with `utm_campaign=51degrees-rust`. The
  lint config is `.utm-lint.json`, which allows untagged links in `Cargo.toml`
  files.
- **Code style.** Wrap code and doc comments to about 80 columns, a soft limit,
  so do not break URLs or string literals to hit it. This keeps diffs
  reviewable.
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

When working on `fodid`, keep the three levels distinct. The **51Did** is the
identifier as a whole. The **envelope** (also called the wrapper) is the signed
OWID that carries it and changes on every issue. The **match key** is the
stable, comparable part of the payload, read through `FodId::match_key`.
Compare match keys, never envelopes. The payload offsets and lengths are
internal to the crate, so every field is read through its typed accessor,
and the layout is specified at
https://github.com/51Degrees/specifications/blob/main/did-specification/identifier-layout.md
