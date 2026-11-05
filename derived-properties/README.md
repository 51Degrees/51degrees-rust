[![51Degrees](https://51degrees.com/img/logo.png?utm_source=github&utm_medium=readme&utm_campaign=51degrees-rust&utm_content=fiftyone-derived-properties-readme.md&utm_term=logo "Data rewards the curious")](https://51degrees.com/?utm_source=github&utm_medium=readme&utm_campaign=51degrees-rust&utm_content=fiftyone-derived-properties-readme.md&utm_term=logo)

# 51Degrees Derived Properties

`DerivedPropertyElement` for the 51Degrees pipeline: computes one more property
from the properties other elements have already produced.

This crate is part of the [51Degrees](https://51degrees.com/?utm_source=github&utm_medium=readme&utm_campaign=51degrees-rust&utm_content=fiftyone-derived-properties-readme.md&utm_term=introduction) Rust solution for high-performance
device detection and IP intelligence, available both on-premise from a local
data file and from the 51Degrees cloud.

## What it does

How a property is computed is written in a **script**, which is one YAML or
JSON file describing one output property. The script format is shared between
every 51Degrees language implementation, at
[51Degrees/derived-properties](https://github.com/51Degrees/derived-properties),
so one script gives the same answer in .NET, Node, Java, Python, PHP and Rust.
This crate reads format 1 in full and runs the same conformance cases as the
other languages.

The element holds no data file, sends no request over the network, keeps
nothing between one request and the next, and reads no evidence from the
request. Work that needs any of those four belongs in an element of your own
rather than in a script.

## The one rule

A script names source properties inside its conditions. Where every one of
those properties is available, the checks and the rules run and a value is
chosen. Where any one of them is not available, the script produces no value,
and the message names every property that was missing along with what its
source element said about each one. There is no third state.

## The scripts are compiled in

The scripts 51Degrees ships are embedded at build time rather than read from a
file, so an element carrying one runs where there is no file system and where
fetching a file over the network would defeat the purpose, such as in a
WebAssembly host or at an edge runtime. `HumanConfidence` is the one shipped
today, and its own comment header says what each of its thresholds rests on.

## Using it

Add the element to a pipeline **after** the elements that supply the properties
its scripts name, and read the result under the element data key `derived`.

```rust
use std::sync::Arc;

use fiftyone_derived_properties::{BuiltInScript, DerivedPropertyElement};
use fiftyone_pipeline_core::Pipeline;

let derived = DerivedPropertyElement::builder()
    // A script shipped inside this crate.
    .add_built_in(BuiltInScript::HumanConfidence)
    // The text of a script of your own, as YAML or as JSON.
    .add_script("MyProperty", my_script_text)
    // A file, where there is a file system to read one from.
    .add_script_file("derived/MyOtherProperty.yaml")
    .build()?;

let pipeline = Pipeline::builder()
    .add_element(device_detection)
    .add_element(Arc::new(derived))
    .build()?;

let mut data = pipeline.create_flow_data_with(evidence);
data.process()?;

match data.get_str("derived").unwrap().get("HumanConfidence") {
    Ok(value) => println!("{value:?}"),
    // The message names every property that could not be read, and why.
    Err(no_value) => println!("{}", no_value.message),
}
```

Building reports everything wrong with every script at once, rather than
stopping at the first fault, and each fault carries the script, where it came
from, the place in the document such as `Rules[3].When.All[1]`, and a plain
message.

### Without a pipeline

A script runs over any `PropertySource`, so a host that already holds the
values can run one on its own. `Script::evaluate_detailed` also answers what
each check came out as and which rule matched, which is what answers the
question a set of rules raises most often, being why a request came out Medium
rather than High.

```rust
use fiftyone_derived_properties::{BuiltInScript, Lookup, MapSource, SourceValue};

let script = BuiltInScript::HumanConfidence.compile()?;
let source = MapSource::new()
    .with("device.IsCrawler", Lookup::Value(SourceValue::Bool(false)));
    // and the rest of the properties the script names
let evaluation = script.evaluate_detailed(&source);
```

## Two things worth knowing about a script

**A source property's type comes from the literal it is compared against**, so
`{ Property: ip.HumanProbability, Ge: 7 }` reads the property as a whole
number. `int` is fixed at a signed 32 bit whole number by the format, so that
one script gives one answer in every language, and a value outside that range
cannot be read.

**Values are never coerced loosely.** The strings `N/A`, `Unknown` and the
empty string never become `false` or `0`, they make the property absent. That
is why `HumanConfidence` reads `device.IsVisible` and `device.HasWebDriver` as
text rather than as booleans, because the data carries the string `Unknown` for
both until the client side JavaScript has run and the script has to be able to
see that value.

## Where the shared files are

`vendor/` holds a copy of the shared repository's script and its conformance
cases, taken unaltered. `cargo test` runs the cases in `vendor/tests` and
checks that every script in `vendor/tests/invalid` is refused for the reason
its file name gives, which is what proves this crate agrees with the other
languages.

## License

Licensed under the European Union Public Licence v1.2 (EUPL-1.2). See the
[repository](https://github.com/51Degrees/51degrees-rust) for the full text.

## Find out more

- Source and issues: [github.com/51Degrees/51degrees-rust](https://github.com/51Degrees/51degrees-rust)
- API documentation: [docs.rs/fiftyone-derived-properties](https://docs.rs/fiftyone-derived-properties)
- The script format and the shipped scripts: [github.com/51Degrees/derived-properties](https://github.com/51Degrees/derived-properties)
- The element specification: [51Degrees/specifications](https://github.com/51Degrees/specifications/blob/main/pipeline-specification/pipeline-elements/derived-property-element.md)
- Build custom data services with the Pipeline API: [51degrees.com/build-your-own](https://51degrees.com/build-your-own?utm_source=docs.rs&utm_medium=package&utm_campaign=51degrees-rust&utm_content=fiftyone-derived-properties-readme.md&utm_term=build-your-own)
- Pipeline API documentation: [51degrees.com/documentation](https://51degrees.com/documentation/_pipeline_api__index.html?utm_source=docs.rs&utm_medium=package&utm_campaign=51degrees-rust&utm_content=fiftyone-derived-properties-readme.md&utm_term=documentation)
- About 51Degrees: [51degrees.com](https://51degrees.com/?utm_source=docs.rs&utm_medium=package&utm_campaign=51degrees-rust&utm_content=fiftyone-derived-properties-readme.md&utm_term=about)
- Data files and pricing: [51degrees.com/pricing](https://51degrees.com/pricing?utm_source=docs.rs&utm_medium=package&utm_campaign=51degrees-rust&utm_content=fiftyone-derived-properties-readme.md&utm_term=pricing)
