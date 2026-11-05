# Comparison with the .NET packages

These programs give the same evidence to the crates in this repository and
to the published 51Degrees .NET packages, and report every answer that
differs. The work that brings the crates to parity with .NET
([#217](https://github.com/51Degrees/51degrees-rust/issues/217)) proves each
change by a difference this comparison stops reporting.

| Path | What it is |
| --- | --- |
| `corpus/evidence.json` | The cases. Each has an id, a sentence saying what it covers, and its evidence. None of it comes from real visitors. |
| `dotnet/` | A .NET 8 console program that runs the cases through the .NET packages and writes one JSON line for each. |
| `rust/` | The same program over the crates in this checkout. |
| `compare/` | The comparer, which reports each difference with the case that caused it. |
| `allowed-differences.json` | The differences that are known, each with its reason and, where there is one, the issue that removes it. |

The workflow
[dotnet-comparison.yml](../.github/workflows/dotnet-comparison.yml) runs all
of it on every pull request, over the Lite files in the `cxx` submodule and,
when the license keys are available to the run, over the Enterprise files.

## The pipeline

Both programs build the same pipeline, with process errors collected rather
than thrown.

1. The Sequence element.
2. Usage sharing, in .NET only. The records go to a handler inside the
   program, so nothing is sent anywhere. The crates have no public sink for
   usage records yet ([#232](https://github.com/51Degrees/51degrees-rust/issues/232)).
3. On-premise Device Detection, HighPerformance profile.
4. On-premise IP Intelligence, LowMemory profile.
5. The JSON builder.
6. The JavaScript builder, with minification off, because the two languages
   use different minifiers.

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
and the report shows the first line that differs.

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
that differs is a difference in the code around the C, never in the C
itself.

When the `cxx` submodule moves, the package versions move in the same pull
request, to the 51degrees-dotnet release built from the new commit, so no
comparison runs with the two sides on different C code. A run can try other
packages by naming them:

```
dotnet build dotnet -c Release -p:DeviceDetectionVersion=<version> -p:IpIntelligenceVersion=<version>
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
sentence saying what it covers. Give it a `query.session-id` unless it is
about a generated one, because usage records are matched to cases by
session id. Keep every character within the Basic Multilingual Plane. Then
run both programs and list any new difference with its reason, or fix it.
