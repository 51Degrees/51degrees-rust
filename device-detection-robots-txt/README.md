[![51Degrees](https://raw.githubusercontent.com/51Degrees/51degrees-rust/main/.github/images/logo-160.png "Data rewards the curious")](https://51degrees.com/?utm_source=github&utm_medium=readme&utm_campaign=51degrees-rust&utm_content=fiftyone-device-detection-robots-txt-readme.md&utm_term=logo)

# 51Degrees robots.txt

`RobotsTxtEngine` for the 51Degrees pipeline: writes a robots.txt that allows
or refuses each known crawler by what it uses the pages it fetches for, from
the crawlers a 51Degrees Device Detection data file describes.

This crate is part of the [51Degrees](https://51degrees.com/?utm_source=github&utm_medium=readme&utm_campaign=51degrees-rust&utm_content=fiftyone-device-detection-robots-txt-readme.md&utm_term=introduction) Rust solution for high-performance
device detection and IP intelligence, available both on-premise from a local
data file and from the 51Degrees cloud.

## What it does

A crawler is a program that fetches pages, such as a search engine's or one
gathering text to train a model. The Device Detection data file names each
crawler, the product tokens it answers to in a robots.txt file, and the
usages it makes of the pages it fetches, such as `Search` or `Monitoring`.

The engine reads every crawler and usage when the pipeline is built. A
request then says which usages to allow, and the engine writes a robots.txt,
following [RFC 9309](https://www.rfc-editor.org/rfc/rfc9309.html), in which
each crawler making none of the allowed usages has its own group with
`Disallow: /`, and every other crawler falls through to a closing
`User-Agent: *` group with `Allow: /`. It gives two properties under the
element data key `robotstxt`.

| Property | Holds |
|----------|-------|
| `PlainText` | The text with one copyright comment, ready to serve as `/robots.txt`. |
| `AnnotatedText` | The text with the licence, a legend, and comments giving each crawler's name, usages and address, for a person to read. |

The text is the one .NET's `FiftyOne.DeviceDetection.RobotsTxt` writes, byte
for byte.

## Using it

Add the engine after the on-premise Device Detection engine, whose data file
must hold the crawler properties, as the Enterprise file does. A data file
without them gives a text that allows every crawler.

| Evidence | Meaning |
|----------|---------|
| `query.robotstxt.<usage>` | `allow`, `true`, `yes`, `enabled` or `on` allows the usage, and `disallow`, `false`, `no`, `disabled` or `off` refuses it, ignoring case. Refusing wins. The usage is named in lower case. A crawler the data records no usage for follows the usage `n/a`. |
| `query.robotstxt.tdl` | One or more Terms Document Locator URIs, or TDL source ids such as `MOW-SOCW`, separated by `,` or `\|`. Each is written in the closing group, and an entry that is neither is left out. |

The engine writes its data only when a request carries one of these keys.
`RobotsTxtEngine::model` gives the usages with their descriptions, for a page
that lets a person choose which to allow.

With the `reqwest-client` feature, a TDL source id is resolved by listing the
source's GitHub folder and taking the highest numbered file, kept for twelve
hours. Without it, or after `set_tdl_source_resolver(None)`, only URIs are
accepted. Any other `TdlSourceResolver` can be given in its place.

A pipeline made from a configuration file written for .NET finds the engine
under .NET's name, `RobotsTxtEngineBuilder` or `RobotsTxtEngine`, once
`configuration::add_builders` has added it to the registry. Its settings are
.NET's, being `UserAgent` for the GitHub resolver and `Properties`, with
.NET's cache and lazy loading settings read and having no effect.

## Links

- Source and issues: [github.com/51Degrees/51degrees-rust](https://github.com/51Degrees/51degrees-rust)
- API documentation: [docs.rs/fiftyone-device-detection-robots-txt](https://docs.rs/fiftyone-device-detection-robots-txt)
- About 51Degrees: [51degrees.com](https://51degrees.com/?utm_source=github&utm_medium=readme&utm_campaign=51degrees-rust&utm_content=fiftyone-device-detection-robots-txt-readme.md&utm_term=about)

## License

Licensed under the European Union Public Licence v1.2 (EUPL-1.2). See the
[repository](https://github.com/51Degrees/51degrees-rust) for the full text.
