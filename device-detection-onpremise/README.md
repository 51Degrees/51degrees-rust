[![51Degrees](https://raw.githubusercontent.com/51Degrees/51degrees-rust/main/.github/images/logo-160.png "Data rewards the curious")](https://51degrees.com/?utm_source=github&utm_medium=readme&utm_campaign=51degrees-rust&utm_content=fiftyone-device-detection-onpremise-readme.md&utm_term=logo)

# 51Degrees Device Detection Onpremise

On-premise Hash device-detection engine built over the safe fiftyone-native wrapper.

This crate is part of the [51Degrees](https://51degrees.com/?utm_source=github&utm_medium=readme&utm_campaign=51degrees-rust&utm_content=fiftyone-device-detection-onpremise-readme.md&utm_term=introduction) Rust solution for high-performance
device detection and IP intelligence, available both on-premise from a local
data file and from the 51Degrees cloud.

## Looking up profiles by a property value

The engine lists the profiles in its data file that hold a value of a
property, for example the hardware profiles of a TAC or a native model name.
Name the properties to index when the engine is built, and the data set
indexes their values when it loads, and again on each refresh.

```rust
let engine = DeviceDetectionOnPremiseEngineBuilder::new("TAC-HashV41.hash")
    .indexed_properties(["TAC", "NativeModel"])
    .build()?;
let ids = engine.profile_ids_for_property_value("NativeModel", native_model)?;
```

The ids are in ascending order, and the property name and value are matched
exactly, including case. A property that is not indexed is looked up by
reading every profile, which gives the same ids more slowly. An id given to
the engine as `query.51D_ProfileIds` evidence gives that profile's values. The
TAC property is only in the TAC data file, and NativeModel is in the
Enterprise and TAC data files.

## License

Licensed under the European Union Public Licence v1.2 (EUPL-1.2). See the
[repository](https://github.com/51Degrees/51degrees-rust) for the full text.

## Find out more

- Source and issues: [github.com/51Degrees/51degrees-rust](https://github.com/51Degrees/51degrees-rust)
- API documentation: [docs.rs/fiftyone-device-detection-onpremise](https://docs.rs/fiftyone-device-detection-onpremise)
- Device detection: [51degrees.com/device-detection](https://51degrees.com/device-detection?utm_source=docs.rs&utm_medium=package&utm_campaign=51degrees-rust&utm_content=fiftyone-device-detection-onpremise-readme.md&utm_term=device-detection)
- Device detection documentation: [51degrees.com/documentation](https://51degrees.com/documentation/_device_detection__index.html?utm_source=docs.rs&utm_medium=package&utm_campaign=51degrees-rust&utm_content=fiftyone-device-detection-onpremise-readme.md&utm_term=documentation)
- About 51Degrees: [51degrees.com](https://51degrees.com/?utm_source=docs.rs&utm_medium=package&utm_campaign=51degrees-rust&utm_content=fiftyone-device-detection-onpremise-readme.md&utm_term=about)
- Data files and pricing: [51degrees.com/pricing](https://51degrees.com/pricing?utm_source=docs.rs&utm_medium=package&utm_campaign=51degrees-rust&utm_content=fiftyone-device-detection-onpremise-readme.md&utm_term=pricing)
