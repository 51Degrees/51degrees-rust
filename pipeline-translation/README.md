[![51Degrees](https://raw.githubusercontent.com/51Degrees/51degrees-rust/main/.github/images/logo-160.png "Data rewards the curious")](https://51degrees.com/?utm_source=github&utm_medium=readme&utm_campaign=51degrees-rust&utm_content=fiftyone-pipeline-translation-readme.md&utm_term=logo)

# 51Degrees Translation Engine

`TranslationEngine` for the 51Degrees pipeline: translates the values another
element produced into the language the request asks for.

This crate is part of the [51Degrees](https://51degrees.com/?utm_source=github&utm_medium=readme&utm_campaign=51degrees-rust&utm_content=fiftyone-pipeline-translation-readme.md&utm_term=introduction) Rust solution for high-performance
device detection and IP intelligence, available both on-premise from a local
data file and from the 51Degrees cloud.

## What it does

The engine reads named properties from the data of one other element,
translates each value, and writes the result to its own data under a new
name. A string, a list of strings and a weighted list of strings can be
translated, and each keeps its form and its weightings.

Each language's translations come from a YAML source of `value: translation`
lines, named for its locale, such as `countries.fr_FR.yml`. Values are
matched ignoring case, and a translation left blank counts as none.

## The language

The engine translates to its fixed language when it has one. Otherwise it
reads the first of these evidence values that is not blank, and tries only
that one.

1. `query.translation`
2. `query.accept-language`
3. `header.accept-language`

The value is read as an `Accept-Language` header, so it can be a locale such
as `fr_FR` or `fr-FR`, a language such as `fr`, or a list with qualities such
as `es,de-DE;q=0.8,en;q=0.5`. The most preferred language the engine holds
is used, and a two letter language such as `fr` takes the first locale that
starts with it. A request that prefers English gets no translation, because
the source values are already in English.

## Values with no translation

`set_missing_translation_behavior` decides what the engine gives for a value
it has no translation for, and for every value when the request names no
language the engine holds.

| Behavior | Gives |
| --- | --- |
| `Original`, the default | The value as it was |
| `EmptyString` | An empty string |
| `FlowError` | No value, with a flow error saying why |

## Using it

Add the engine **after** the element whose values it translates, and read the
result under the element data key `translation`.

```rust
use std::sync::Arc;

use fiftyone_pipeline_core::Pipeline;
use fiftyone_pipeline_translation::{TranslationEngine, TRANSLATION_KEY};

// Translates the Country property of the element whose data key is
// "location", from the files countries.fr_FR.yml, countries.de_DE.yml and so
// on.
let translation = TranslationEngine::builder()
    .set_source_element_data_key("location")
    .add_source("translations/countries.*.yml")
    .add_translation("Country", "CountryTranslated")
    .build()?;

let pipeline = Pipeline::builder()
    .add_element(location_element)
    .add_element(Arc::new(translation))
    .build()?;

let mut data = pipeline.create_flow_data_with(evidence);
data.process()?;
let country = data
    .get(TRANSLATION_KEY)
    .and_then(|translations| translations.value("CountryTranslated"));
```

Engines that share the data key `translation` write to the same data, so one
engine can translate what another wrote, such as a country code to its
English name and then the name to the language asked for. Add such engines
one after another. Each element of a parallel group writes data of its own,
so the pipeline builder refuses two engines with one data key in the same
group.

## Links

- Source and issues: [github.com/51Degrees/51degrees-rust](https://github.com/51Degrees/51degrees-rust)
- API documentation: [docs.rs/fiftyone-pipeline-translation](https://docs.rs/fiftyone-pipeline-translation)
- The engine specification: [51Degrees/specifications](https://github.com/51Degrees/specifications/blob/main/pipeline-specification/pipeline-elements/translation-engine.md)
- About 51Degrees: [51degrees.com](https://51degrees.com/?utm_source=github&utm_medium=readme&utm_campaign=51degrees-rust&utm_content=fiftyone-pipeline-translation-readme.md&utm_term=about)

## License

Licensed under the European Union Public Licence v1.2 (EUPL-1.2). See the
[repository](https://github.com/51Degrees/51degrees-rust) for the full text.
