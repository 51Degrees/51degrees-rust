# Missing CI scripts and the road to the shared reusable workflows

This note records why the Rust repository's CI diverges from the rest of the
51Degrees organisation today, what the organisation's standard contract looks
like, and the incremental direction for closing the gap. It exists so the next
person (or agent) does not have to re-derive the layout from scratch, and so the
docs that must be kept current (see `AGENTS.md`) point at a single source of
truth.

## The organisation standard (how the other repos work)

Consuming repositories keep their GitHub Actions YAML deliberately thin. They do
not `run:` common-ci PowerShell directly; instead they `uses:` common-ci's
*reusable workflows*. For example, `51degrees-cxx/.github/workflows`:

```
nightly-pipeline.yml
  └─ uses: 51Degrees/common-ci/.github/workflows/nightly-pull-requests.yml@main
       └─ uses: nightly-pull-request.yml         (per PR: Configure → BuildAndTest
          │                                        → ComparePerformance → Complete)
          └─ runs: nightly-pull-request.build-and-test.ps1
```

The orchestrator `nightly-pull-request.build-and-test.ps1` then calls back into
**repo-local** hook scripts by a fixed naming convention:

```
./<repo>/ci/fetch-assets.ps1
./<repo>/ci/setup-environment.ps1
./<repo>/ci/build-project.ps1
./<repo>/ci/run-unit-tests.ps1
./<repo>/ci/run-integration-tests.ps1
./<repo>/ci/run-performance-tests.ps1   # only when Options.RunPerformance
```

plus a `ci/options.json` describing the build matrix. In short: common-ci owns
the *orchestration*; each repo owns a set of `ci/<verb>.ps1` *customization
entry points*. The only common-ci script a repo's YAML runs directly is the
generic linter `scripts/utm-lint.ps1`.

## Where the Rust repo stands today

The Rust repo has **not** joined the shared orchestration. It carries its own
self-contained `.github/workflows/nightly-performance.yml`, which the workflow
header itself notes is "kept self-contained here (rather than calling the shared
reusable workflow) until the repository joins the shared nightly orchestration."

Historically its performance adapter also lived in the wrong repository:
`common-ci/rust/run-performance-tests.ps1`. That was an anomaly — common-ci
exists to hold code shared *across* repositories, and the Rust repo is the only
Rust consumer, so there is nothing to share. Every other language keeps its
`run-performance-tests.ps1` under its own `<repo>/ci/`.

## What this change does (Phase 1)

- Adds the Rust performance adapter to this repo at
  `ci/run-performance-tests.ps1`, matching the org's `<repo>/ci/<verb>.ps1`
  convention. This is the customization entry point that further repo-specific
  work will build on.
- Repoints `nightly-performance.yml` to run `./51degrees-rust/ci/run-performance-tests.ps1`
  (the workflow checks this repo out into a `51degrees-rust/` subdirectory) instead of
  `./common-ci/rust/run-performance-tests.ps1`.
- Keeps the `Checkout common-ci` step, because the steps that publish and
  compare the results, `steps/publish-performance-results.ps1` and
  `steps/compare-performance.ps1`, are shared and come from common-ci.

Moving the adapter's home is the structural part of the change. Alongside it,
this repo's copy has since diverged from the common-ci original with
repository-specific fixes (the ASN data fetch, the `dryrun` input, the
`common-ci-ref`/`dd-cxx-ref`/`ipi-cxx-ref` inputs, and letting the compare step
fail the job), so the behaviour of the nightly run is not identical to the old
`common-ci/rust` path.

common-ci no longer carries a `rust` folder, so this repo's copy is the only
one.

## The remaining gap (Phase 2, deferred)

Fully aligning with the organisation standard would mean:

- Providing the complete `ci/` verb set (`build-project.ps1`,
  `run-unit-tests.ps1`, `run-integration-tests.ps1`, `setup-environment.ps1`,
  `fetch-assets.ps1`, `run-performance-tests.ps1`) and a `ci/options.json`
  build matrix.
- Replacing the bespoke `nightly-performance.yml` with a thin
  `nightly-pipeline.yml` that `uses:` the common reusable workflows, letting
  common-ci drive build, test, performance and publish uniformly with the other
  languages.

This is a much larger change that alters how the Rust repo is built and tested
in CI across the whole organisation, and it carries open-ended maintenance until
the shared workflows fully accommodate a Cargo-workspace consumer. It is
recorded here as a direction, not scheduled work. Weigh that cost explicitly
before starting it.
