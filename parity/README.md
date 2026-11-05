# Comparison with the .NET packages

These programs give the same evidence to the crates in this repository and
to the published 51Degrees .NET packages, and report every answer that
differs. The work that brings the crates to parity with .NET
([#217](https://github.com/51Degrees/51degrees-rust/issues/217)) proves each
change by a difference this comparison stops reporting.

| Path | What it is |
| --- | --- |
| `corpus/evidence.json` | The cases. Each has an id, a sentence saying what it covers, its evidence and, for some, the pipeline it runs through. None of it comes from real visitors. |
| `dotnet/` | A .NET 8 console program that runs the cases through the .NET packages and writes one JSON line for each. |
| `rust/` | The same program over the crates in this checkout. |
| `compare/` | The comparer, which reports each difference with the case that caused it. |
| `allowed-differences.json` | The differences that are known, each with its reason and, where there is one, the issue that removes it. |

The workflow
[dotnet-comparison.yml](../.github/workflows/dotnet-comparison.yml) runs all
of it on every pull request, over the Lite files in the `cxx` submodule and,
when the license keys are available to the run, over the Enterprise files.

## The pipelines

Both programs build the same two pipelines, with process errors collected
rather than thrown. A case runs through the first unless its `pipeline` is
`values`.

1. The Sequence element.
2. Usage sharing, in .NET only. The records go to a handler inside the
   program, so nothing is sent anywhere. The crates have no public sink for
   usage records yet ([#232](https://github.com/51Degrees/51degrees-rust/issues/232)).
3. On-premise Device Detection, HighPerformance profile.
4. On-premise IP Intelligence, LowMemory profile.
5. The JSON builder.
6. The JavaScript builder, with minification off, because the two languages
   use different minifiers.

The values pipeline has the Sequence element, the values element, the JSON
builder and the JavaScript builder. The values element is part of each
program. Its data holds one of each kind of value the JSON builder writes,
being strings that need escaping, doubles and 32-bit floats that need an
exponent or a decimal point, a whole number, true or false, a list, a
property with no value and its reason, a delayed JavaScript property and a
property naming it as evidence, and a weighted list. Last comes `Promise`,
which the JavaScript builder reads, taken from the `header.values-promise`
evidence and with no value when that is absent. The element also adds two
errors and a warning. With no engine in the pipeline, nothing the engines
give differently reaches the JSON or the JavaScript, so their text can be
the same on both sides, and the comparison then checks both byte for byte,
apart from the JavaScript's line endings (see "The comparison"). The crates'
JSON builder leaves the crates' Sequence element's data out there, because
.NET's Sequence element has none
([#231](https://github.com/51Degrees/51degrees-rust/issues/231)).

The JavaScript builder in the values pipeline calls back to the endpoint
`json`, names its object `fod`, has cookies and minification off, and takes
the host and protocol from the evidence. The `values-promise-` cases run in
turn with `Promise` set to `Full`, then with no value, then `Full` again,
so the builder has to decide for each request whether promises are on
([#230](https://github.com/51Degrees/51degrees-rust/issues/230)).

## The line

Each program writes one JSON object per case.

| Field | What it holds |
| --- | --- |
| `id` | The case id. |
| `processError` | The error processing gave, or null. |
| `properties` | For each element that lists properties, apart from the two builders, every property it lists and, for `device`, the match metrics. Each is keyed by its lowercase name and holds its `name` with either a `type` and `value`, a `noValue` reason, or a `missing` message when reading it failed. |
| `session` | The session id and sequence the pipeline used. |
| `evidenceAfter` | `query.session-id` and `query.sequence` as the evidence holds them after processing. |
| `errors` | The errors in the flow data, each with its element's data key and its message. |
| `json` | The JSON builder's output. |
| `javascript` | The JavaScript builder's output. |
| `usage` | The usage sharing record written for the case, or null. |

Value types are `string`, `bool`, `integer`, `double`, `stringList`,
`javascript`, `ipAddress`, `weightedList` (each item a `rawWeighting` and a
`value`), `keyValueList` and `list`.

## The comparison

The comparer names each difference by the case, a path into the line and a
kind. Properties, the session and the errors are compared as data, so
numbers compare by value. When a property's types differ its values are
compared as well, so allowing a type never hides a value. The JSON output
is compared by structure, then by
key order, then as text, so a path such as `json.device.ismobile` names the
key that differs. The JavaScript and the usage record are compared as text,
and the report shows the first line that differs. When the two texts are the
same once each CRLF is read as LF, the difference is of kind `lineEndings`
instead, and each side is the number of lines it ends with CRLF. The .NET
4.5.129 package embeds its JavaScript template with CRLF line endings, where
the template the specification names and the crates' copy have LF, so each
line of .NET's script that comes from the template ends with CRLF. The
`lineEndings` entry in `allowed-differences.json` allows that and nothing
else, because the crates' side must have no CRLF.

| Kind | Meaning |
| --- | --- |
| `missingLine` | A program wrote no line for the case. |
| `onlyInDotnet`, `onlyInRust` | One side has the element, property or key. |
| `name` | A property name differs in case. |
| `state` | One side has a value and the other has none. |
| `type`, `value` | The types or the values differ. |
| `noValueMessage` | The reasons for no value differ. |
| `order` | The same keys in a different order. |
| `text` | The same structure written differently. |
| `lineEndings` | The same text once each CRLF is read as LF. |
| `presence` | One side wrote null. |

A session id the Sequence element generates is random, so when a case
supplies none the generated id is replaced on each side before comparing,
provided it has the form .NET gives it.

A difference is allowed only when an entry in `allowed-differences.json`
matches it. An entry has a `path` pattern, where `*` matches any run of
characters, an optional `kind`, optional `cases` patterns, optional `data`
(`lite` or `enterprise`), a required `reason`, and an `issue` number.
Optional `dotnet` and `rust` sides say what each side gave, either a
`value` list, compared as data, or `"noValue": true`, so that an entry
allows only the change its reason describes. For a property the value is
the property's value, and for anything else the value at the path. The
first entry that matches takes the difference. The run also fails when an
entry that applies to its data files allows nothing, so a change that
removes a difference removes its entry in the same pull request.

## The C code

Both sides build the same C code. The crates build their native code from
the [51degrees-cxx](https://github.com/51Degrees/51degrees-cxx) commit the
`cxx` submodule records, 4.5.103. The .NET packages named in
`dotnet/Parity.csproj` are the
[51degrees-dotnet](https://github.com/51Degrees/51degrees-dotnet) release
whose own `cxx` submodule records the same commit, being Device Detection
4.5.180, IP Intelligence 4.5.155 and Pipeline 4.5.129. So an engine answer
that differs comes from what each side does with the same C code, including
which of its functions each side calls, and never from the two sides
building different versions of it. Latitude and longitude are an example,
where .NET reads each value through text 51degrees-cxx writes with three
decimal places and the crates read the value itself.

When the `cxx` submodule moves, the package versions move in the same pull
request, to the 51degrees-dotnet release built from the new commit. The
comparison workflow checks this before it writes any answers. Each engine
package records the 51degrees-dotnet commit it was built from, and the step
fails unless that commit's `cxx` submodule is the one this repository
records, so no comparison runs with the two sides on different C code.

The nightly submodule update moves `cxx` on its own, in a pull request into
`rc`, and the comparison does not run on pull requests into `rc`. After such
a move, the next comparison, on a pull request into `main` or a `release/`
branch, fails at that check until the package versions move to a
51degrees-dotnet release built from the new commit.

The check runs only in the workflow, so a local run can try other packages
by naming them. Name all three versions, because each Device Detection and
IP Intelligence release depends on its own Pipeline release, and a
`PipelineVersion` older than that is a package downgrade, which the .NET SDK
treats as an error
([NU1605](https://learn.microsoft.com/en-us/nuget/reference/errors-and-warnings/nu1605)):

```
dotnet build dotnet -c Release -p:DeviceDetectionVersion=<version> -p:IpIntelligenceVersion=<version> -p:PipelineVersion=<version>
```

## Running it

From this folder, with the submodules checked out, the Lite files pulled
from Git LFS and `pwsh ../ci/copy-owid-source.ps1` run once, as
[AGENTS.md](../AGENTS.md) describes:

```
cargo build -p parity-rust -p parity-compare
dotnet build dotnet -c Release -o out/dotnet
DATA="--corpus corpus/evidence.json --dd ../cxx/device-detection-data/51Degrees-LiteV4.1.hash --ipi ../cxx/ip-intelligence-data/51Degrees-IPIV4AsnIpiV41.ipi"
dotnet out/dotnet/Parity.dll $DATA --out out/dotnet.jsonl
target/debug/parity-rust $DATA --out out/rust.jsonl
target/debug/parity-compare --corpus corpus/evidence.json --dotnet out/dotnet.jsonl \
  --rust out/rust.jsonl --allowed allowed-differences.json --data lite --strict \
  --report out/report.md
```

`cargo test -p parity-compare` tests the comparer and needs nothing native.

## Adding a case

Add it to `corpus/evidence.json` with an id no other case has and a
sentence saying what it covers, and with `"pipeline": "values"` if it runs
through the values pipeline. Give it a `query.session-id` unless it is
about a generated one, because usage records are matched to cases by
session id. Only the engines' pipeline shares usage, so a values case never
has a usage record, even when it gives an engines' case's session id. Keep
every character within the Basic Multilingual Plane. Then run both programs
and list any new difference with its reason, or fix it.
