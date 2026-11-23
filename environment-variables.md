# Environment variable names

Every name 51Degrees defines starts with `_51DEGREES_`, so wherever it appears
it is clearly a 51Degrees name. The underscore is there because a POSIX shell
cannot set a name that starts with a digit. Each name holds one thing, and no
name is filled from another.

This page lists every name the crates, tests, examples, scripts and workflows
of this repository read.
[`examples/examples-shared/tests/environment_variable_names.rs`](examples/examples-shared/tests/environment_variable_names.rs)
reads the tables below and fails when a file reads a name that is not here.
A change that reads a new name adds a row in the same piece of work.

A variable that one workflow sets for its own steps also starts with
`_51DEGREES_`. It is local to that workflow, so it has no row here.

## Names a person sets

An example reads them as a customer sets them. `_51DEGREES_CLOUD_ENDPOINT`
is also an organization variable, which the workflows pass under the same
name. No other name here has an organization secret or variable. A workflow
step that needs `_51DEGREES_LICENSE_KEY` sets it from the secret that carries
the products it needs, listed under "Secrets only CI uses". The example's own test, or the script that starts the
example, sets them for the example.

| Name | What it holds |
| --- | --- |
| `_51DEGREES_RESOURCE_KEY` | The Resource Key a cloud example reads. |
| `_51DEGREES_LICENSE_KEY` | A 51Degrees License Key. The data file update examples and the 51Did demo read it, the shared fetch step reads it from its step's environment to download licensed data files, and one live test of the cloud request engine reads it. |
| `_51DEGREES_CLOUD_ENDPOINT` | The address of the cloud service, in place of the public one. Unset means the public service. A workflow that runs cloud tests or examples passes the organization variable of the same name from `vars.`. |
| `_51DEGREES_DD_PATH` | The path of a Device Detection data file, for the tests and the examples. |
| `_51DEGREES_IPI_PATH` | The path of an IP Intelligence data file, for the tests and the examples. An Enterprise file is given this way. |
| `_51DEGREES_EXAMPLE_HTTP_PORTS` | The port a web example listens on. Each web example listens on one port, so it takes the first of a list. |
| `_51DEGREES_CXX_DIR` | A checkout of the C sources for the native build, in place of the copy in the crate or the `cxx` submodule. |

## Keys the tests read

The three Resource Keys and `_51DEGREES_LICENSE_KEY_51DID` are organization
secrets of the same name. A job in one of this repository's own workflows
sets each from `secrets.` on the step that reads it, and on no other step.
The name check fails on a workflow that reads any `_51DEGREES_RESOURCE_KEY`
name from `vars.`.

A job that calls a shared workflow from common-ci passes `secrets: inherit`
and names no secret. The shared workflow reads what it needs under its own
`_51DEGREES_` names, so those names have no row here. A shared step, such as
the one that fetches data files, takes no secret as an input. The test named
above fails on a call that passes a secret any other way.

| Name | What it holds |
| --- | --- |
| `_51DEGREES_RESOURCE_KEY_BESPOKE` | A Resource Key that carries every property. The live cloud tests and the examples' own live tests read it. |
| `_51DEGREES_RESOURCE_KEY_BASIC` | A Resource Key on the basic tier, which the 51Did live tests also run with, and which the Human Confidence examples' test on the staging cloud expects to get no `HumanConfidenceSource`. |
| `_51DEGREES_RESOURCE_KEY_51DID` | The Resource Key for the 51Did live tests and the 51Did demo page check. |
| `_51DEGREES_LICENSE_KEY_51DID` | The License Key of the account `_51DEGREES_RESOURCE_KEY_51DID` belongs to. |

## Settings of the tests and the release scripts

| Name | What it holds |
| --- | --- |
| `_51DEGREES_REQUIRE_ENTERPRISE` | Set where `_51DEGREES_IPI_PATH` names the Enterprise file, so a test that needs that file fails rather than skips. |
| `_51DEGREES_REQUIRE_LOCATION` | Makes the location tests fail rather than skip. |
| `_51DEGREES_REQUIRE_TAC` | Makes the TAC tests fail rather than skip. |
| `_51DEGREES_IPI_MEMORY_PROFILES` | The performance profiles the IP Intelligence memory test measures. |
| `_51DEGREES_SPIN` | The Spin program the caching host test starts, in place of `spin` on the path. |
| `_51DEGREES_ALLOW_VERSION_BUMP` | `true` lets a publish move the major or minor version. The publish workflow sets it from its input. |

## Names a build or a test sets for itself

Nobody sets these. A build script sets each for its own crate, or a test or
a script sets it for the process it starts.

| Name | What it holds |
| --- | --- |
| `_51DEGREES_RUSTC_VERSION` | The compiler version that usage sharing reports. |
| `_51DEGREES_DERIVED_PROPERTIES_COMMIT` | The commit of the derived properties scripts the crate was built from. |
| `_51DEGREES_TRANSLATIONS_DIR` | The folder the translation files are compiled in from. |
| `_51DEGREES_WEB_ETAG_CHILD` | Set by the ETag test for its child process. |
| `_51DEGREES_CACHING_HELD_FOLDER` | Set by the disk cache test for its child process. |
| `_51DEGREES_EXAMPLE_URL` | Set by the browser contract script for the shared browser test suite, which is told where the running example answers. |
| `_51DEGREES_EXAMPLE_LANG` | Set by the browser contract script for the shared browser test suite, which is told the example's language. |

## Secrets and variables only CI uses

No customer sets these, so they have no `_51DEGREES_` start. A step sets the
name a customer uses from one of them, on that step alone, or a test that
the step runs reads it.

| Name | What it holds |
| --- | --- |
| `CLOUD_STAGING_ENDPOINT` | An organization variable naming the staging cloud, the API base including `/api/v4/`. The production cloud does not serve `HumanConfidenceSource` yet, so the Human Confidence examples' tests of the source and of the passive level read the cloud from it, and skip without it. It has no default address. |
| `LICENSE_KEY_V4TAC_IPIV4ENTERPRISE` | A License Key that carries the V4TAC and IPIV4Enterprise products, which the TAC and Enterprise data files need. The steps that fetch the TAC and Enterprise data files set `_51DEGREES_LICENSE_KEY` from it, and the steps that check it is there read it. |
| `RESOURCE_KEY_BESPOKE_STAGING` | A Bespoke Resource Key the staging cloud resolves, which the Human Confidence examples' tests on the staging cloud read. |

## Names other tools define

These are not 51Degrees names. Each is used as its tool requires. A name
that ends with `*` stands for every name that starts that way.

| Name | Whose it is |
| --- | --- |
| `CARGO_*` | Cargo, including `CARGO_REGISTRY_TOKEN`, which the publish workflow sets from the crates.io login step. |
| `OUT_DIR` | Cargo |
| `RUSTC` | Cargo |
| `RUSTFLAGS` | Cargo |
| `RUSTDOCFLAGS` | Cargo |
| `GITHUB_*` | GitHub Actions |
| `RUNNER_*` | GitHub Actions |
| `GH_TOKEN` | The GitHub command line tool |
| `PATH` | The operating system |
| `ProgramFiles` | Windows |
| `PSModulePath` | PowerShell |
| `CHROMEWEBDRIVER` | The GitHub runner images |
| `DOTNET_*` | .NET |
