[![51Degrees](https://raw.githubusercontent.com/51Degrees/51degrees-rust/main/.github/images/logo-160.png "Data rewards the curious")](https://51degrees.com/?utm_source=github&utm_medium=readme&utm_campaign=51degrees-rust&utm_content=fiftyone-ip-intelligence-translation-readme.md&utm_term=logo)

# 51Degrees IP Intelligence Country Translation Engines

Translation engines for the 51Degrees pipeline that turn the country codes
[IP Intelligence](https://51degrees.com/ip-intelligence?utm_source=github&utm_medium=readme&utm_campaign=51degrees-rust&utm_content=fiftyone-ip-intelligence-translation-readme.md&utm_term=introduction)
gives into country names, in the language the request asks for.

This crate is part of the 51Degrees Rust solution for high-performance device
detection and IP intelligence, available both on-premise from a local data
file and from the 51Degrees cloud. It is built on the
[translation engine](https://github.com/51Degrees/51degrees-rust/tree/main/pipeline-translation).

## What it does

IP Intelligence gives `CountryCodesGeographical` and `CountryCodesPopulation`,
each a weighted list of the codes of the countries an IP address may be in.
Two engines build on them.

| Engine | Data key | Gives |
| --- | --- | --- |
| `CountryCodeTranslationEngine` | `countrynames` | `CountryNamesGeographical` and `CountryNamesPopulation`, the English name of each country, with its weighting |
| `CountriesTranslationEngine` | `countrynamestranslated` | `CountryNamesGeographicalTranslated` and `CountryNamesPopulationTranslated`, those names in the language asked for, and four lists of every country |

The lists of every country are `CountryNamesGeographicalAllTranslated`,
`CountryNamesPopulationAllTranslated`, `CountryCodesGeographicalAll` and
`CountryCodesPopulationAll`. Each starts with the countries IP Intelligence
gives, highest weighting first, and follows with every other country sorted
by name as the language sorts it. The names and codes lists line up, so the
same place in each is the same country. A code that is not a country, such
as `Unknown`, is left out of them.

The names, in English and 16 other languages, come from the 51Degrees data
repository and are compiled in, so neither engine needs any configuration
or files.

## The language

The language is read as the translation engine reads it, from
`query.translation`, `query.accept-language` or `header.accept-language`. A
request that prefers English, or asks for no language the engine holds,
gets the English names, and the lists of every country are then sorted
ignoring case.

Each language's order comes from the Unicode CLDR collation data (the rules
each language uses to put text in order) in the version of `icu_collator`
that the build resolves, much as .NET's order comes from the ICU library on
the host it runs on. A build that resolves a newer version can order a few
names differently where that data changed.

The translated weighted names use the first of those evidence values that is
not blank. The lists of every country try each in turn until one names a
language the engine holds.

## Using it

Add the engines **after** the IP Intelligence engine, in this order.

```rust
use std::sync::Arc;

use fiftyone_ip_intelligence_translation::{
    CountriesTranslationData, CountriesTranslationEngine, CountryCodeTranslationEngine,
    COUNTRY_NAMES_TRANSLATED_KEY,
};
use fiftyone_pipeline_core::Pipeline;

let pipeline = Pipeline::builder()
    .add_element(ip_intelligence_engine)
    .add_element(Arc::new(CountryCodeTranslationEngine::builder().build()?))
    .add_element(Arc::new(CountriesTranslationEngine::builder().build()?))
    .build()?;

let mut data = pipeline.create_flow_data_with(evidence);
data.process()?;
if let Some(countries) = data.get(COUNTRY_NAMES_TRANSLATED_KEY) {
    // Every country, the most likely first, for a country picker.
    let codes = countries.country_codes_geographical_all();
    let names = countries.country_names_geographical_all_translated();
}
```

## Size in a WebAssembly build

`CountriesTranslationEngine` sorts for the language each request asks for,
so a build cannot know which locales it needs, and `icu_collator` compiles in
the Unicode collation data for every locale it holds. Edge platforms limit
the size of a deployed module, so the figures are given here.

In a release `wasm32-wasip1` build with link time optimisation, one codegen
unit and symbols stripped, a pipeline holding both engines gave a module of
1,765,409 bytes, against 377,472 bytes for `CountryCodeTranslationEngine`
alone. So `CountriesTranslationEngine` added 1,387,937 bytes. Of that, the 16
languages' country names are 99,380 bytes of text, and the rest is the
engine's code, the ICU4X code and data it links, and the few lines the larger
program uses to print the list of every country it checks. This was measured
with Rust 1.99.1 and `icu_collator` 2.4.0.

The repository's Pull Request workflow measures this on every run with
`ci/measure-wasm-size.sh`, and writes the figures to the run's summary.

A deployment that needs a smaller module can generate ICU4X data for the 16
locales alone with `icu4x-datagen`, and set `ICU4X_DATA_DIR` to it when
building, as ICU4X's
[data management tutorial](https://github.com/unicode-org/icu4x/blob/031d9e89f6a3fd26a14491e090729d46094328fd/tutorials/data-management.md#3-using-the-generated-data)
describes. Every ICU4X data crate in the build then reads that data in place
of its own. That route is not measured here.

## Links

- Source and issues: [github.com/51Degrees/51degrees-rust](https://github.com/51Degrees/51degrees-rust)
- API documentation: [docs.rs/fiftyone-ip-intelligence-translation](https://docs.rs/fiftyone-ip-intelligence-translation)
- The engine specification: [51Degrees/specifications](https://github.com/51Degrees/specifications/blob/main/pipeline-specification/pipeline-elements/translation-engine.md)
- The translation files: [51Degrees/data](https://github.com/51Degrees/data/tree/main/ip-intelligence/Translations)
- About 51Degrees: [51degrees.com](https://51degrees.com/?utm_source=github&utm_medium=readme&utm_campaign=51degrees-rust&utm_content=fiftyone-ip-intelligence-translation-readme.md&utm_term=about)

## License

Licensed under the European Union Public Licence v1.2 (EUPL-1.2). See the
[repository](https://github.com/51Degrees/51degrees-rust) for the full text.
