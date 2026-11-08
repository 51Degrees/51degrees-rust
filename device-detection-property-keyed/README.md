[![51Degrees](https://raw.githubusercontent.com/51Degrees/51degrees-rust/main/.github/images/logo-160.png "Data rewards the curious")](https://51degrees.com/?utm_source=github&utm_medium=readme&utm_campaign=51degrees-rust&utm_content=fiftyone-device-detection-property-keyed-readme.md&utm_term=logo)

# 51Degrees hardware profiles by TAC and native model name

`TacEngine` and `NativeEngine` for the 51Degrees pipeline: every hardware
profile in a 51Degrees Device Detection data file that holds a TAC or a
native model name.

This crate is part of the [51Degrees](https://51degrees.com/?utm_source=github&utm_medium=readme&utm_campaign=51degrees-rust&utm_content=fiftyone-device-detection-property-keyed-readme.md&utm_term=introduction) Rust solution for high-performance
device detection and IP intelligence, available both on-premise from a local
data file and from the 51Degrees cloud.

## What it does

A TAC (Type Allocation Code) is the first eight digits of a mobile device's
IMEI, and a native model name is the model a device's own operating system
reports, such as `SM-G960F`. One TAC or model name can belong to several
devices, so each engine gives a list of the hardware profiles that hold it,
as .NET's `FiftyOne.DeviceDetection.PropertyKeyed` does.

| Builder | Key property | Evidence | Data key |
|---------|--------------|----------|----------|
| `TacEngineBuilder` | `TAC` | `query.tac` | `hardware` |
| `NativeEngineBuilder` | `NativeModel` | `query.nativemodel` | `hardware` |
| `PropertyKeyedDeviceEngineBuilder` | Any, set with `set_key_property` | `query.` and the property in lower case | `profiles-` and the property in lower case, unless set |

Each engine writes a `MultiDeviceData` with one property, `Profiles`, which
lists each profile's `ProfileId` and the values of the properties of the
key property's component that are not JavaScript, the hardware for `TAC`
and `NativeModel`. The JSON builder writes those values alone for each
profile under `hardware.profiles`, leaving out a property the profile holds
with no value. Each profile reads with the same `DeviceData` accessors as a
detection. The TAC
and Native engines both write under `HARDWARE_DATA_KEY`, as the cloud's
hardware lookup does, so when both are in one pipeline the profiles of the
engine added first come first.

## Use

Add the engines after an on-premise Device Detection engine whose data file
holds the key property, and name the property to that engine's
`indexed_property`, or to its `IndexedProperties` setting in a
configuration, so each lookup reads the data file's index rather than
every profile. Without the index, the engine logs a warning when the
pipeline is built.

```rust
use std::sync::Arc;

use fiftyone_device_detection_onpremise::DeviceDetectionOnPremiseEngineBuilder;
use fiftyone_device_detection_property_keyed::{NativeEngineBuilder, TacEngineBuilder};
use fiftyone_device_detection_shared::{DeviceData, HARDWARE_DATA_KEY};
use fiftyone_pipeline_core::{Evidence, FlowElement, Pipeline};

let device = DeviceDetectionOnPremiseEngineBuilder::new("TAC-HashV41.hash")
    .indexed_property("TAC")
    .indexed_property("NativeModel")
    .build()?;
let pipeline = Pipeline::builder()
    .add_element(device as Arc<dyn FlowElement>)
    .add_element(TacEngineBuilder::new().build()? as Arc<dyn FlowElement>)
    .add_element(NativeEngineBuilder::new().build()? as Arc<dyn FlowElement>)
    .build()?;
let mut data = pipeline.create_flow_data_with(
    Evidence::builder().add("query.nativemodel", "SM-G960F").build(),
);
data.process()?;
if let Some(hardware) = data.get(HARDWARE_DATA_KEY) {
    for profile in hardware.profiles() {
        println!("{:?}", profile.hardware_vendor().value());
    }
}
```

A request carrying the evidence always gets the data, holding no profiles
when its value matches none. A TAC that is not exactly eight digits, or a
native model name shorter than two characters, gives .NET's warning in the
flow data's warnings and no profiles, never an error. A value is matched
exactly, including case, as the data file's index matches it, where .NET's
engines ignore the case of a value.

`TAC` is only in the TAC data file, and `NativeModel` is in the Enterprise
and TAC data files. The free Lite file holds neither, so with it the
engines accept no evidence and write nothing. "Data files and pricing"
under "Find out more" says how to get the other files.

A pipeline made from a configuration file written for .NET finds the
engines under .NET's names, `TacEngineBuilder`, `NativeEngineBuilder` and
`PropertyKeyedDeviceEngineBuilder`, each also without `Builder`, once
`configuration::add_builders` has added them to the registry. Their
settings are .NET's, being `KeyProperty` and `ElementDataKey`, with .NET's
cache, lazy loading and property settings read and having no effect. The
Device Detection engine before them names the key properties in
`IndexedProperties`, which .NET's builder does not have.

## License

Licensed under the European Union Public Licence v1.2 (EUPL-1.2). See the
[repository](https://github.com/51Degrees/51degrees-rust) for the full text.

## Find out more

- Source and issues: [github.com/51Degrees/51degrees-rust](https://github.com/51Degrees/51degrees-rust)
- API documentation: [docs.rs/fiftyone-device-detection-property-keyed](https://docs.rs/fiftyone-device-detection-property-keyed)
- Device detection: [51degrees.com/device-detection](https://51degrees.com/device-detection?utm_source=docs.rs&utm_medium=package&utm_campaign=51degrees-rust&utm_content=fiftyone-device-detection-property-keyed-readme.md&utm_term=device-detection)
- Device detection documentation: [51degrees.com/documentation](https://51degrees.com/documentation/_device_detection__index.html?utm_source=docs.rs&utm_medium=package&utm_campaign=51degrees-rust&utm_content=fiftyone-device-detection-property-keyed-readme.md&utm_term=documentation)
- About 51Degrees: [51degrees.com](https://51degrees.com/?utm_source=docs.rs&utm_medium=package&utm_campaign=51degrees-rust&utm_content=fiftyone-device-detection-property-keyed-readme.md&utm_term=about)
- Data files and pricing: [51degrees.com/pricing](https://51degrees.com/pricing?utm_source=docs.rs&utm_medium=package&utm_campaign=51degrees-rust&utm_content=fiftyone-device-detection-property-keyed-readme.md&utm_term=pricing)
