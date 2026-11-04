# CI scripts and the shared reusable workflows

This repository's nightly merge and its publish run through common-ci's shared
reusable workflows, as the other 51Degrees languages do. common-ci owns the
orchestration, and this repository supplies a set of `ci/<verb>.ps1` hook
scripts that the shared workflows call by name, with `ci/options.json`
describing the build matrix. The hooks run from the common-ci checkout, with
this repository cloned beside it as `rust/`, and each is given only the
parameters it declares.

## The nightly pipeline

`.github/workflows/nightly-pipeline.yml` runs at 01:00 UTC.

```text
nightly-pipeline.yml
  ├─ SubmoduleUpdate: uses common-ci nightly-submodule-update.yml
  │    moves cxx and owid-rust to the head of their default branches and
  │    raises a pull request when either has moved
  ├─ PullRequests: uses common-ci nightly-pull-requests.yml
  │    builds and tests each eligible pull request merged into main on every
  │    platform in ci/options.json, then merges it
  ├─ Publish: uses common-ci nightly-publish.yml
  │    publishes main when it has moved past the last release tag
  └─ published-examples: tests the examples against the crates on crates.io
```

The submodule update stage runs only common-ci's own steps and calls no hook.
Its pull request goes through the pull request stage like any other, in the
same run when it is eligible.

The pull request stage calls `ci/setup-environment.ps1`, `ci/fetch-assets.ps1`,
`ci/build-project.ps1`, `ci/run-unit-tests.ps1` and
`ci/run-integration-tests.ps1`.

The publish stage calls:

- `ci/get-next-package-version.ps1`, which stops the run when there are no
  version tags and otherwise works the version out from the tags with
  common-ci's GitVersion rules.
- `ci/build-package.ps1`, which writes the version into every crate, copies the
  OWID source into `fodid` and the C sources into the `-sys` crates, tests the
  examples against the crates at that version and packages every crate in
  `ci/crates.txt` into `package/`, with the commit it built.
- `ci/install-package.ps1`, which has nothing to do.
- `ci/publish-package.ps1`, which writes the version and copies the OWID and C
  sources again, because `cargo publish` packages each crate afresh from the
  tree, then publishes each crate from that commit in `ci/crates.txt` order,
  through crates.io Trusted Publishing, waiting for the crates.io index to list
  each version before the next crate.

The shared publish then tags the release and creates its GitHub release.
`.github/workflows/publish.yml` runs the same publish by hand, and is the only
route that can allow a major or minor version move.

No entry in `ci/options.json` is a package requirement, so the shared
publish's prebuild and test stages do nothing. The release is tested on every
platform as each pull request merges, and its examples are tested against the
crates being released before anything is published.

## crates.io Trusted Publishing

crates.io matches the workflow that started the run, so both
`nightly-pipeline.yml` and `publish.yml` are registered as trusted publishers
for every crate by `ci/setup-trusted-publishing.ps1`. The calling jobs grant
`id-token: write`, and `ci/publish-package.ps1` exchanges the run's GitHub OIDC
identity for a short-lived crates.io token for each crate and revokes it after
the publish. No crates.io token is stored.

## Performance

`.github/workflows/nightly-performance.yml` stays self-contained. It runs
`ci/run-performance-tests.ps1` from this repository and takes only the shared
steps that publish and compare the results,
`steps/publish-performance-results.ps1` and `steps/compare-performance.ps1`,
from common-ci.
