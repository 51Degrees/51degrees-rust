# Comparison with the .NET packages

These programs give the same evidence to the crates in this repository and
to the published 51Degrees .NET packages, and report every answer that
differs. The work that brings the crates to parity with .NET
([#217](https://github.com/51Degrees/51degrees-rust/issues/217)) proves each
change by a difference this comparison stops reporting.

| Path | What it is |
| --- | --- |
| `corpus/evidence.json` | The cases. Each has an id, a sentence saying what it covers, its evidence and, for some, the pipeline it runs through. None of it comes from real visitors. |
| `corpus/fodid.json` | The 51Did cases. Each is a payload to sign, a text to read as it is, or the 51Did metadata (see "The 51Did comparison"). |
| `corpus/configuration.json` | The configuration cases. Each is a configuration file in .NET's shape to build a pipeline from (see "The configuration comparison"). |
| `corpus/robots-txt.json` | The fixed robots.txt cases. The cases for each usage come from the data file (see "The robots.txt comparison"). |
| `corpus/property-keyed.json` | The fixed TAC and Native cases. The cases for each TAC and NativeModel value come from the data file (see "The TAC and Native comparison"). |
| `dotnet/` | A .NET 8 console program that runs the cases through the .NET packages and writes one JSON line for each. |
| `rust/` | The same program over the crates in this checkout. |
| `compare/` | The comparer, which reports each difference with the case that caused it. |
| `allowed-differences.json` | The differences that are known, each with its reason and, where there is one, the issue that removes it. |
| `check-native-model-case.ps1` | Checks that every NativeModel difference in the TAC and Native comparison comes from case alone (see "The TAC and Native comparison"). |

The workflow
[dotnet-comparison.yml](../.github/workflows/dotnet-comparison.yml) runs all
of it on every pull request, over the Lite files in the `cxx` submodule and,
when the license keys are available to the run, over the Enterprise files.

## The pipelines

Both programs build the same five pipelines, with process errors collected
rather than thrown. A case runs through the first unless its `pipeline` is
`values`, `agent-signature`, `translation` or `cloud-translation`.

1. The Sequence element.
2. Usage sharing, twice, with the same settings on both sides. The first
   element writes the record the built-in transport sends, and its documents
   go to a handler in .NET and to a sink in the crates, inside the program,
   so nothing is sent anywhere. The second writes an element before and after
   each record and its own product and Flow Element names, through a
   `ShareUsageBase` subclass in .NET and a sink in the crates
   ([#232](https://github.com/51Degrees/51degrees-rust/issues/232)). Both
   share every request, with a share percentage of 1 and a repeat evidence
   interval of 0, leave out the `Referer` header, share the `utm_source`
   query parameter beside those starting `51D_`, and share no request whose
   `header.x-health-check` is `true`.
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
.NET's Sequence element has none. The crates' has the session id and
sequence the specification's Element Data gives.

The JavaScript builder in the values pipeline calls back to the endpoint
`json`, names its object `fod`, has cookies and minification off, and takes
the host and protocol from the evidence. The `values-promise-` cases run in
turn with `Promise` set to `Full`, then with no value, then `Full` again,
so the builder has to decide for each request whether promises are on
([#230](https://github.com/51Degrees/51degrees-rust/issues/230)).

The agent signature pipeline has the Sequence element and the agent
signature element. The element's key directories come from a client inside
each program rather than the network. It serves the public part of the
Ed25519 test key of
[RFC 9421 Appendix B.1.4](https://www.rfc-editor.org/rfc/rfc9421.html#appendix-B.1.4)
for `agent.parity.test`, serves the same key for `slow-agent.parity.test`
only after ten seconds, and answers every other address with 404. The wait
budget is two seconds on both sides. The cases were signed with the private
part of that key, so the signatures are fixed in the corpus. Each is valid
from 2025 until 2124, so no clock has to be set.

The translation pipeline has the Sequence element, an element that writes
IP Intelligence country codes, the country codes translation engine, the
countries translation engine and the JSON builder
([#227](https://github.com/51Degrees/51degrees-rust/issues/227)). .NET's
engines come from the FiftyOne.IpIntelligence.Translation package at the IP
Intelligence version. The codes come from the case's evidence rather than a
data file, so both sides translate the same codes and weightings whatever the
file. `query.parity-geographical` gives `CountryCodesGeographical` and
`query.parity-population` gives `CountryCodesPopulation`, each as codes and
raw weightings such as `GB:35535,FR:30000`. A property whose evidence is
absent is not set, one whose evidence is empty has no value, and with neither
there is no IP Intelligence data. The element is part of each program, and on
each side it writes that side's IP Intelligence data, so the engines read the
codes as they read an IP Intelligence engine's. In the crates that is
`IpIntelligenceDataBase` with a missing property source, as both IP
Intelligence engines write it, so a weighted record is a `value` and a
`weight` and a property the evidence did not give is missing. .NET's JSON
writes a record as `rawweighting` and `value`, which the
[#226](https://github.com/51Degrees/51degrees-rust/issues/226) entries in
`allowed-differences.json` allow. It lists no properties, so only the JSON
shows its data. The JSON builder leaves the crates' Sequence element's data
out, as in the values pipeline. The countries engine sorts every country by
the collation of the language asked for, being the ICU on the runner in .NET
and ICU4X's compiled data in the crates, so any difference in how the two sort
shows in the lists of every country.

The cloud translation pipeline has the Sequence element, the cloud request
engine, the cloud countries translation engine and the JSON builder
([#297](https://github.com/51Degrees/51degrees-rust/issues/297)). .NET's
engine comes from the FiftyOne.IpIntelligence.Cloud package at the IP
Intelligence version. The cloud is a stand-in inside each program, an
`HttpMessageHandler` in .NET and a `CloudHttpClient` in the crates, so
nothing is sent anywhere. It grants the six translated properties that
.NET's tests of the engine list, takes the evidence `query.parity-cloud-response`
and `query.parity-cloud-status`, and answers each request with the body the
first gives and the status the second gives, 200 when it is absent. So the
case holds the whole response, and both engines read the same one. The
cases cover a full section, names and fields in either case, null values with
and without a reason, the section absent, named in capitals or empty, and a
failed request. In this pipeline a property the data does not hold is
written as `missing` on both sides, because .NET's cloud data fails such a
read, where the crates' read gives no value with the same reason. The
two reasons differ only in the configurator link's campaign tags, so
`allowed-differences.json` allows that difference and a workflow step
compares the reasons with the tags removed.

## The line

Each program writes one JSON object per case.

| Field | What it holds |
| --- | --- |
| `id` | The case id. |
| `processError` | The error processing gave, or null. |
| `properties` | For each element that lists properties, apart from the two builders and the cloud request engine, whose data is the case's own response, every property it lists, with the match metrics for `device` and `SortingCultureUsed`, which the engine writes without listing, for `countrynamestranslated`. Each is keyed by its lowercase name and holds its `name` with either a `type` and `value`, a `noValue` reason, or a `missing` message when reading it failed. |
| `session` | `query.session-id` and `query.sequence` from the evidence after processing, the sequence as a number, or null when it is not a 64-bit integer. |
| `evidenceAfter` | `query.session-id` and `query.sequence` as the evidence holds them after processing. |
| `errors` | The errors in the flow data, each with its element's data key and its message. |
| `json` | The JSON builder's output. |
| `javascript` | The JavaScript builder's output. |
| `usage` | The usage sharing record written for the case, or null. |
| `usageCustom` | The record the second usage sharing element wrote for the case, or null. |
| `fodid` | For a 51Did case, the parse status and, when the 51Did was read, the value of each accessor, else null. |
| `metadata` | For the 51Did metadata case, the component's name and id and each property's metadata in list order. |
| `configuration` | For a configuration case, what was built or the error (see "The configuration comparison"). |
| `derived` | For a derived property case, the script's source properties and its answer (see "The derived property comparison"). |
| `robotsTxtEngine`, `plainText`, `annotatedText` | For a robots.txt case, the engine's evidence keys and properties, and the two texts (see "The robots.txt comparison"). |
| `warnings`, `profileIds`, `hardwareJson`, `propertyKeyedEngines`, `indexedValues` | For a TAC and Native case, the warnings and the ids of the profiles found, and for some cases the JSON's `hardware`, the engines' evidence keys and properties, and the values each side indexes (see "The TAC and Native comparison"). |

Value types are `string`, `bool`, `integer`, `double`, `stringList`,
`javascript`, `ipAddress`, `dateTime` (in .NET only, written as RFC 3339
text with the offset), `weightedList` (each item a `rawWeighting` and a
`value`), `keyValueList` and `list`.

## The comparison

The comparer names each difference by the case, a path into the line and a
kind. Properties, the session and the errors are compared as data, so
numbers compare by value. When a property's types differ its values are
compared as well, so allowing a type never hides a value. The JSON output
is compared by structure, then by
key order, then as text, so a path such as `json.device.ismobile` names the
key that differs. The JavaScript is compared as text, and the report shows
the first line that differs. When the two scripts are the
same once each CRLF is read as LF, the difference is of kind `lineEndings`
instead, and each side is the number of lines it ends with CRLF. The .NET
4.5.129 package embeds its JavaScript template with CRLF line endings, where
the template the specification names and the crates' copy have LF, so each
line of .NET's script that comes from the template ends with CRLF. The
`lineEndings` entry in `allowed-differences.json` allows that and nothing
else, because the crates' side must have no CRLF.

The usage records are compared as text once the text of `DateSent`,
`Version`, `Language`, `LanguageVersion`, `Platform` and each `FlowElement`
is left out on both sides, because those hold the time the record was
written or name the runtime. Only the values before the evidence entries are
left out, and the second element's records keep their `FlowElement` names,
which both sides must write the same. `ServerIP` is compared, because both
programs run on the same machine and work it out from the machine's own
name. The report shows how many characters come before the first that
differs, then the text from shortly before it.

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
`value` list, compared as data, a `pattern` list, where `*` matches any
run of characters in a text value, or `"noValue": true`, so that an entry
allows only the change its reason describes. For a property the value is
the property's value, and for anything else the value at the path. A text
difference gives each side as `line N: ` and that line, so a pattern such
as `line *: Allow: /` allows the line wherever it falls. The
first entry that matches takes the difference. The run also fails when an
entry that applies to its data files allows nothing, so a change that
removes a difference removes its entry in the same pull request.

## The 51Did comparison

The 51Did cases need no data file, so they run in both jobs. Each program
signs every payload in `corpus/fodid.json` for the domain `parity.test`
with a P-256 key it makes for the run, and writes a signed list with its own
prefix, `rust-` or `dotnet-`, before every id. Then each program reads both
signed lists, so each verifies the signatures the other made. No key is
committed.

A case has an `id`, an `about` sentence saying what it covers, and one of
these.

| Field | What it holds |
| --- | --- |
| `payload` | Hex bytes laid out as [identifier-layout.md](https://github.com/51Degrees/specifications/blob/main/did-specification/identifier-layout.md) describes, which the program signs. White space between digits is ignored. |
| `text` | Text given to the reader as it is, never signed. |
| `metadata` | `true` for the case whose line holds the 51Did metadata. |

A `payload` case may also have these.

| Field | What it holds |
| --- | --- |
| `encoding` | How the signed envelope reaches the reader. `standard`, the default, is the standard alphabet with padding. `url` is the URL-safe alphabet with no padding. `url-surrounded` is `url` with a space before it and a line feed after it. `url-spaced` is `url` with a space between each group of 20 characters. `bytes` gives the envelope's bytes. |
| `trailing` | Hex bytes added after the envelope before it is encoded. |
| `tamper` | `true` to change the last byte of the signature after signing. |

An entry of a signed list has the case's `id` with the prefix, its
`about`, a `form` of `text`, `bytes` or `metadata`, the `input` the reader
is given (hex for `bytes`), the signing key as SPKI PEM in `publicKey`, null
for a case that was not signed, and `tamper`.

The `fodid` field of a line holds `status` and then `type`, `usage`,
`idUsage`, `usageIsIndirect`, `licenseId`, `matchKey`, `terms`,
`base64Url`, `version`, `domain`, `date`, `payload`, `signature`, `base64`
and `signatureStatus`. Bytes are lower-case hex and the date is RFC 3339 in
UTC. The signature is checked with the entry's `publicKey`, so
`signatureStatus` is null for a case that was not signed. Statuses use
.NET's names, such as `Parsed`, `PayloadTooShort` and `SignatureValid`, and
the Rust program writes any status .NET has no name for with a `rust:`
prefix, so it shows as a difference.

The comparer takes both signed lists as cases and the 51Did lines beside the
others, because `--corpus`, `--dotnet` and `--rust` may each be given more
than once. It compares `fodid` and `metadata` key by key and list item by
item, so a path such as `fodid.matchKey` or
`metadata.properties[2].description` names the field that differs.

Two steps of the lite job check that this comparison means something.
Each side must read every signed case that is meant to read with its
signature `SignatureValid`, in both lists, and the tampered case with its
signature `SignatureInvalid`, so the two languages cannot agree by both
failing. Then a Rust line with a changed match key, and a Rust metadata line
with a changed description, must each fail the comparison.

## The configuration comparison

The configuration cases check that a configuration file written for the
.NET packages builds the same pipeline from the crates
([#220](https://github.com/51Degrees/51degrees-rust/issues/220)). A case
has an `id`, an `about` sentence and the file, in the shape .NET's
`PipelineOptions` reads, given either as JSON in `configuration` or as the
text of the file in `text`. Each program gives its reader the file as
JSON text. The Rust program writes a `configuration` back through
`serde_json`, which keeps the order of names and writes text, whole
numbers, `true`, `false` and `null` as given, but may write a fraction in
another form, so a fraction goes in a case given as `text`, which both
programs read unchanged. A `text` also carries what JSON cannot, such as
comments and trailing commas. The string `"{dd}"` stands for the Device
Detection data file and `"{ipi}"` for the IP Intelligence one, and each
program puts the full path of the file it was given in its place, written
as a JSON string.

The first case holds the Sequence element, the Hash and IP Intelligence
engines in a parallel group, the JSON builder and usage sharing, with the
pipeline setting `SuppressProcessExceptions`, every parameter given as
text. The second is the same with the two engines on their own. Six
change the first in one place, to a builder name that matches no
builder, a parameter no setting takes or a value that does not parse,
once at the top level and once inside the group. Two hold the Hash
engine and then the robots.txt engine, with usage sharing off, one giving
each setting .NET's robots.txt builder reads and one a parameter it has
no setting for, whose error lists the builder's settings. Three give a
pipeline setting under both `PipelineBuilderParameters` and
`BuildParameters`, two of them with `ShareUsage` false, so no usage
sharing element is added.
`configuration-engines-only` names only the two engines, so it compares
the Sequence element and usage sharing that `FiftyOnePipelineBuilder`
adds, and usage sharing's default settings. `configuration-value-forms`
is the first case with whole numbers, `true`, `false` and `null` in place
of text and with names in other cases, and `configuration-text-forms` a
file given as text with comments, trailing commas, a name holding an
escape and a fraction with an exponent.
`configuration-pipeline-setting-does-not-parse` gives the pipeline's
suppress setting a value that is not true or false.
The five `configuration-translation-` cases name the translation engines.
`configuration-translation-countries` makes the two country translation
engines after the IP Intelligence engine. The other four name the
translation engine's builder, whose translations are given in code only, so
none of them builds. One gives the three settings a configuration can give,
one a missing translation behavior that does not parse, one `AddTranslation`,
which takes two values, and one `Sources`, which names no setting.

The .NET program reads each file with .NET's own configuration reader and
builds it with `FiftyOnePipelineBuilder`, making the builders through a
service provider, as .NET's examples do. Its `HttpClient` refuses to send,
and neither program processes anything, so the usage sharing element sends
nothing. The Rust program builds each file with a registry holding the
builders of the crates it uses, in the order .NET lists those of the
packages the .NET program loads, which make pipelines as .NET's
`FiftyOnePipelineBuilder` does.

The `configuration` field of a line holds `error`, the message of the
configuration error, or null. When the pipeline was built it also holds
`suppressProcessExceptions`, `concurrent`, and `elements`, the elements in
the order the pipeline lists them, being those on their own first and then
those in each parallel group, as .NET's `IPipeline.FlowElements` lists them.
Each element has its `dataKey`. An on-premise engine also has `dataFile`,
its data file's settings with the path given as its placeholder. The usage
sharing element also has `settings`, read from the element itself on
both sides, the crates through `ShareUsageElement::config` and .NET from
fields of `ShareUsageBase` read by reflection, and `evidenceFilter`, the
element's evidence filter's answer for each of a list of keys, which shows
the blocked headers, the query string parameters shared and whether all
evidence is shared, because .NET holds those settings only in that filter.
`CreateTempDataCopy` is a build parameter that the crates do not read back, and
`a_copy_of_the_data_file_is_read_when_asked` in each on-premise engine's
`tests/configuration.rs` checks it against what .NET did.

The comparer compares `configuration` key by key and list item by item,
after making the parts of an error message that the languages write
differently by design the same in both. Line endings become LF. The type
name of an element's builder or of the pipeline's builder, the .NET full
name or the Rust name, becomes `{type}`.
In the list of a builder's settings, the parameter type after each name is
left out, because .NET 4.5.129 gives `RuntimeParameterInfo` for every
setting, then a name listed straight after itself is listed once and
`Build` is left out, because .NET lists both forms of two usage sharing
settings and lists `Build(Stream)` among the IP Intelligence engine's
settings. Generic types are left out of the available builders, because
.NET lists its abstract base builders there, which no configuration can
name. Everything else in the message is compared exactly.

One step of the lite job checks that this comparison means something. The
cases that build must build on both sides and the others fail, so the two
cannot agree by both failing. Then each of seven changed Rust answers,
compared on its own, must fail the comparison at its path: a usage
sharing setting read back differently, two elements in the other order,
the other answer for the suppress setting, another default usage sharing
setting, a setting given as a whole number read back differently, the
fraction read back differently, and another error for the pipeline
setting that does not parse. The report lists at most three differences
for each path and kind, so a change compared with the others could be
left out of it.

## The derived property comparison

The derived property cases check that the derived property element gives
the same answer in both languages for every case derived-properties ships
with its scripts
([#233](https://github.com/51Degrees/51degrees-rust/issues/233)). They are
the case files in `derived-properties/vendor/tests`, one for each script
the crate builds in, read as they are rather than copied into the corpus.
.NET's element comes from the FiftyOne.Pipeline.DerivedProperty package at
the Pipeline version, which builds in the same scripts, so each side runs
its own built-in copy of the script the file names.

`parity-rust derived-cases` writes the cases as a corpus for the comparer,
each with an id, an `about` naming the script and the case, and the
answer the file expects. The id is `derived-`, the script's name in lower
case, and the case's place in its file counting from 1, such as
`derived-humanconfidence-01`. Both programs give each case the same id.

Each program reads the case files with its own reader, the crate's YAML
library in the crates and the package's own parser in .NET, and builds a
pipeline for each case as the package's own conformance tests do. That is
one source element for each data key the script or the case names, in the
ordinal order of the keys, then the derived property element with the
built-in script. Each source element lists every source property of the
script under its key, so the pipeline check passes, and writes only the
values the case gives, so a case that leaves one out has it absent. A plain
value is written as its own type, `{ String: ... }` as text,
`{ NoValue: ... }` as a property with that no value message, and a list of
`Value` and `Weight` pairs as weighted values.

The `derived` field of a line holds `script`, `sourceProperties` (the
script's source properties in the order it names them), `processError`,
then `type` and `value` when the output has a value, or `noValue` with its
message when it has none, and `missing`, the source properties that
message names in the script's order. A pipeline that could not be built
gives `error` in place of everything after `sourceProperties`. The
comparer compares `derived` key by key and list item by item, so a path
such as `derived.value` or `derived.missing[0]` names the field that
differs.

Two steps of the lite job check that this comparison means something.
Each side must give every case the value the case file expects, or no
value with the missing properties it expects, and the number of cases is
counted in the case files themselves. Then a Rust line with a changed
value, and one naming no missing property where .NET names one, must
each fail the comparison.

## The robots.txt comparison

The robots.txt cases check that the robots.txt engine writes the same
`PlainText` and `AnnotatedText` in both languages
([#225](https://github.com/51Degrees/51degrees-rust/issues/225)). .NET's
engine comes from the FiftyOne.DeviceDetection.RobotsTxt package at the
Device Detection version. Each program builds one pipeline, the on-premise
Device Detection engine with the LowMemory profile and then the robots.txt
engine, with process errors collected rather than thrown. The engine reads
the crawlers and the usages from the data file, so over the Lite file, which
holds no crawlers, each text has only its header and the closing
`User-Agent: *` group, and over the Enterprise file each crawler the request
does not allow has a group of its own.

The engine's resolver of Terms Document Locator source ids is one inside each
program, so no case needs the network. It knows `parity-tdl`, ignoring case,
and gives `https://tdl.parity.test/terms/3.txt` for it, and knows
`parity-unresolved` but gives nothing for it, as when a lookup fails with no
earlier answer. It knows no other id.

`parity-rust robots-txt-cases` writes the cases as a corpus for the comparer
and for both programs. First come the fixed cases of `corpus/robots-txt.json`,
which give no evidence, evidence the engine does not read, and Terms Document
Locator entries that are URIs, lists, source ids, empty or not URIs. Then,
for each usage key the crates' engine reads from the data file, in its order,
come `robots-txt-allow-NN`, which allows that usage alone, and
`robots-txt-disallow-NN`, which refuses it alone, counting from 01. Last come
every usage allowed, every usage but the first allowed, every usage allowed
with a Terms Document Locator URI, the first usage's key in capitals, and
`robots-txt-value-NN`, which gives the first usage's key each value the
engine accepts in another case, two it does not and one with spaces around
it. Over the Lite file there are no usages, so only the fixed cases run.

The `robotsTxtEngine` field of the line for `robots-txt-none` holds the
engine's `dataKey`, `dataSourceTier`, `evidenceKeys` (in ordinal order) and
`properties` (in list order, each with its `name`, `category`, `type`,
`available`, `dataTiers` and `description`). Every other line holds null
there. `plainText` and `annotatedText` hold the two texts, or null when the
engine wrote no data. The comparer compares `robotsTxtEngine` key by key and
list item by item, and the two texts as text, so a difference names the first
line that differs. Texts that are the same byte for byte read the same under
an [RFC 9309](https://www.rfc-editor.org/rfc/rfc9309.html) reader, and the
crate's own tests check what such a reader reads from them.

Four entries in `allowed-differences.json` allow robots.txt differences,
each by a `pattern` for the first line that differs. Over the Enterprise
file, two allow the crawlers with no usable product token, which the
Device Detection release the comparison runs still writes and the crate
leaves out, as .NET does since
[51degrees-dotnet#78](https://github.com/51Degrees/51degrees-dotnet/pull/78).
They come out when the comparison runs a .NET release with that change.
Over the Lite file, two allow how a Terms Document Locator entry is
written, where the crate follows RFC 3986 and keeps its own text. Over the
Enterprise file those two cases first differ at a crawler the first two
entries allow.

Over the Enterprise file, one step of that job checks that each side read the
usages and wrote a group for at least one crawler, so the comparison cannot
pass with no crawler in either text, and another runs the crate's data file
tests over that file. Over the Lite file, one step checks that a Rust text
with one line changed fails the comparison.

## The TAC and Native comparison

The TAC and Native cases check that the TAC and Native engines find the same
hardware profiles, with the same values, in both languages
([#225](https://github.com/51Degrees/51degrees-rust/issues/225)). .NET's
engines come from the FiftyOne.DeviceDetection.PropertyKeyed package at the
Device Detection version. Each program builds the on-premise Device Detection
engine with the LowMemory profile, then two pipelines, each holding that
engine and then a TAC engine and a Native engine of its own, the second with
the Sequence element first, whose sequence the JSON builder reads, and the
JSON builder last. A case runs through the second when its `json` is true.
Both collect process errors rather than throwing them. The crates' Device
Detection engine indexes `TAC` and `NativeModel`, and .NET's engines build
their own index of each when they join a pipeline. The Lite file holds
neither property, so over it neither engine accepts evidence or writes data.

`parity-rust property-keyed-cases` writes the cases as a corpus for the
comparer and for both programs. First come the fixed cases of
`corpus/property-keyed.json`, which give no evidence, a User-Agent alone,
TACs and native model names each engine accepts or refuses, both refused in
one request, and query values neither engine reads. Then, from the values the
crates' Device Detection engine lists for each key property, come each key in
capitals, a native model name in lower case and in upper case, and both keys
in one request, each with the first value a profile holds that the engine
accepts. Then come `property-keyed-tac-value-NNNNNN` for each TAC and
`property-keyed-native-value-NNNNNN` for each NativeModel, in the data file's
order counting from 000001. Last come `property-keyed-profile-<id>` for every
50th distinct profile those cases find through a value the engine accepts,
counting from the first, each with the evidence of the first case that finds
it. Over the Lite file only the fixed cases run.

Each line holds `warnings`, the flow data's warnings, each with its
element's data key and its message, and `profileIds`, the id of each profile
in the `hardware` data in order, or null when neither engine wrote data. The
line for a profile case holds that profile's values in `properties.device`,
one for each property the engines list for a profile, read as the evidence
cases read a property, so the entries for Device Detection values apply to
them as they do to a detection. It holds null there when the case does not
find the profile. The line for a case whose `json` is true holds the JSON
builder's `hardware` in `hardwareJson`, or null when the JSON has none.

The line for `property-keyed-none` also holds `propertyKeyedEngines` and
`indexedValues`. The first holds each engine's `dataKey`, `dataSourceTier`,
`evidenceKeys` (in ordinal order) and `properties` (each with its `name`,
`category`, `type`, `available`, `dataTiers`, `description` and
`itemProperties`, and each of those with its `name`, `category`, `type` and
`available`). The second holds, for each key property the data file has,
each value some profile holds with how many profiles hold it. The crates
read those from the C library's index and .NET from its engine's own index,
so a value one side holds and the other does not shows there, including a
value .NET finds in another case. The comparer compares each of these fields
key by key and list item by item.

Three entries in `allowed-differences.json` allow TAC and Native differences.
One allows the type of `Profiles`, which .NET gives as its .NET type and the
crates as `keyValueList`. Over the Enterprise file, two allow .NET finding a
native model name in any case, as its own index compares values ignoring
case, where the crates match the name exactly through the C library's index,
as the draft specification of the lookup asks
([specifications#62](https://github.com/51Degrees/specifications/pull/62)).
One allows the difference in `indexedValues`, where .NET counts the values
that differ only in case together under the spelling it met first, and the
other the difference in `profileIds` in the NativeModel cases, where .NET
gives the profiles of every such value, and a profile holding two of them
twice. They come out when .NET's engines use the C library's index
([51degrees-dotnet#30](https://github.com/51Degrees/51degrees-dotnet/issues/30)).

Over the Enterprise file, one step of that job checks that each side found
profiles for at least one case and wrote the values of at least one profile,
so the comparison cannot pass with no profile on either side, another runs
`check-native-model-case.ps1`, which fails on any NativeModel difference that
case alone does not explain, so the two entries for case hide nothing else,
and another runs the crate's data file tests over that file.

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

From this folder, with the submodules checked out, the Lite files
downloaded with `pwsh ../cxx/data/get-data-files.ps1 51Degrees-LiteV4.1.hash
51Degrees-IPIV4AsnIpiV41.ipi` as the lite job does, and
`pwsh ../ci/copy-owid-source.ps1` run once, as [AGENTS.md](../AGENTS.md)
describes:

```
cargo build -p parity-rust -p parity-compare
dotnet build dotnet -c Release -o out/dotnet
DATA="--corpus corpus/evidence.json --dd ../cxx/data/device-detection/51Degrees-LiteV4.1.hash --ipi ../cxx/data/ip-intelligence/51Degrees-IPIV4AsnIpiV41.ipi"
dotnet out/dotnet/Parity.dll $DATA --out out/dotnet.jsonl
target/debug/parity-rust $DATA --out out/rust.jsonl
target/debug/parity-rust fodid-sign --cases corpus/fodid.json --prefix rust- --out out/fodid-rust.json
dotnet out/dotnet/Parity.dll fodid-sign --cases corpus/fodid.json --prefix dotnet- --out out/fodid-dotnet.json
SIGNED="--corpus out/fodid-rust.json --corpus out/fodid-dotnet.json"
target/debug/parity-rust fodid $SIGNED --out out/fodid-rust.jsonl
dotnet out/dotnet/Parity.dll fodid $SIGNED --out out/fodid-dotnet.jsonl
CONFIG="--cases corpus/configuration.json --dd ../cxx/data/device-detection/51Degrees-LiteV4.1.hash --ipi ../cxx/data/ip-intelligence/51Degrees-IPIV4AsnIpiV41.ipi"
dotnet out/dotnet/Parity.dll configuration $CONFIG --out out/configuration-dotnet.jsonl
target/debug/parity-rust configuration $CONFIG --out out/configuration-rust.jsonl
DERIVED="--cases ../derived-properties/vendor/tests"
target/debug/parity-rust derived-cases $DERIVED --out out/derived-cases.json
target/debug/parity-rust derived $DERIVED --out out/derived-rust.jsonl
dotnet out/dotnet/Parity.dll derived $DERIVED --out out/derived-dotnet.jsonl
DD="--dd ../cxx/data/device-detection/51Degrees-LiteV4.1.hash"
target/debug/parity-rust robots-txt-cases --corpus corpus/robots-txt.json $DD --out out/robots-txt-cases.json
target/debug/parity-rust robots-txt --corpus out/robots-txt-cases.json $DD --out out/robots-txt-rust.jsonl
dotnet out/dotnet/Parity.dll robots-txt --corpus out/robots-txt-cases.json $DD --out out/robots-txt-dotnet.jsonl
target/debug/parity-rust property-keyed-cases --corpus corpus/property-keyed.json $DD --out out/property-keyed-cases.json
target/debug/parity-rust property-keyed --corpus out/property-keyed-cases.json $DD --out out/property-keyed-rust.jsonl
dotnet out/dotnet/Parity.dll property-keyed --corpus out/property-keyed-cases.json $DD --out out/property-keyed-dotnet.jsonl
target/debug/parity-compare --corpus corpus/evidence.json $SIGNED \
  --corpus corpus/configuration.json --corpus out/derived-cases.json \
  --corpus out/robots-txt-cases.json --corpus out/property-keyed-cases.json \
  --dotnet out/dotnet.jsonl --dotnet out/fodid-dotnet.jsonl \
  --dotnet out/configuration-dotnet.jsonl --dotnet out/derived-dotnet.jsonl \
  --dotnet out/robots-txt-dotnet.jsonl --dotnet out/property-keyed-dotnet.jsonl \
  --rust out/rust.jsonl --rust out/fodid-rust.jsonl \
  --rust out/configuration-rust.jsonl --rust out/derived-rust.jsonl \
  --rust out/robots-txt-rust.jsonl --rust out/property-keyed-rust.jsonl \
  --allowed allowed-differences.json --data lite --strict --report out/report.md
```

`cargo test -p parity-compare` tests the comparer and needs nothing native.

## Adding a case

Add it to `corpus/evidence.json` with an id no other case has and a
sentence saying what it covers, and with its `pipeline` if it runs through
the values, agent signature, translation or cloud translation pipeline. Give it a `query.session-id` unless it is
about a generated one, because usage records are matched to cases by
session id. Only the engines' pipeline shares usage, so a values case never
has a usage record, even when it gives an engines' case's session id, and an
engines' case whose `header.x-health-check` is `true` has none either. Keep
every character within the Basic Multilingual Plane. Then run both programs
and list any new difference with its reason, or fix it.

A 51Did case goes in `corpus/fodid.json` in the same way. A new signed case
that is meant to read changes the number of cases the workflow step "Check
every signed 51Did verifies" expects to verify, so change that number in the
same pull request.

A configuration case goes in `corpus/configuration.json` in the same way.
One that builds changes the cases the workflow step "Check that a changed
configuration answer fails" expects to build, so change that list in the
same pull request.

A fixed robots.txt case goes in `corpus/robots-txt.json` in the same way.
The cases for each usage are written from the data file, so a usage the data
adds gives its cases with no change here. A fixed TAC and Native case goes
in `corpus/property-keyed.json` in the same way, and the cases for each
value are written from the data file too.

A derived property case is added to the case file in derived-properties,
and reaches this comparison when the crate's `vendor` folder is next copied
from that repository. A new case file needs its script built into both
the crate and the .NET package.
