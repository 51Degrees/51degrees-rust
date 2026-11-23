[![51Degrees](https://raw.githubusercontent.com/51Degrees/51degrees-rust/main/.github/images/logo-160.png "Data rewards the curious")](https://51degrees.com/?utm_source=github&utm_medium=readme&utm_campaign=51degrees-rust&utm_content=fiftyone-device-detection-readme.md&utm_term=logo)

# 51Degrees Device Detection

Device-detection facade re-exporting the engines plus a builder that selects the cloud or on-premise deployment.

This crate is part of the [51Degrees](https://51degrees.com/?utm_source=github&utm_medium=readme&utm_campaign=51degrees-rust&utm_content=fiftyone-device-detection-readme.md&utm_term=introduction) Rust solution for high-performance
device detection and IP intelligence, available both on-premise from a local
data file and from the 51Degrees cloud.

## From a configuration file

A pipeline can also be made from a configuration file written for the .NET
package [FiftyOne.DeviceDetection](https://www.nuget.org/packages/FiftyOne.DeviceDetection),
so one file serves both languages. `configuration::registry()` finds each
builder and setting by its .NET name, and adds a Sequence element first and,
with the default `share-usage` feature, usage sharing last, as .NET's
`FiftyOnePipelineBuilder` does. The file may hold comments, as .NET's reader
allows. This one makes a pipeline with the on-premise engine reading the free
Lite data file.

```jsonc
{
  "PipelineOptions": {
    "Elements": [
      {
        "BuilderName": "DeviceDetectionHashEngineBuilder",
        "BuildParameters": {
          "DataFile": "51Degrees-LiteV4.1.hash",
          "PerformanceProfile": "LowMemory",
          "CreateTempDataCopy": false,
          // Off because the free Lite file has no licence key for updates.
          "AutoUpdate": false,
          "DataFileSystemWatcher": false
        }
      }
    ]
  }
}
```

```rust,no_run
use fiftyone_device_detection::configuration::{self, PipelineOptions};
use fiftyone_device_detection::{DeviceData, Evidence, PipelineBuilder, DEVICE_DATA_KEY};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let options = PipelineOptions::from_json(&std::fs::read_to_string("appsettings.json")?)?;
    let pipeline =
        PipelineBuilder::new().build_from_configuration(&configuration::registry(), &options)?;

    let mut data = pipeline.create_flow_data_with(
        Evidence::builder().add("header.user-agent", "Mozilla/5.0 ...").build(),
    );
    data.process()?;
    if let Some(device) = data.get(DEVICE_DATA_KEY) {
        println!("IsMobile: {:?}", device.is_mobile().value());
    }
    Ok(())
}
```

The cloud engines are `CloudRequestEngineBuilder`, given the resource key as
`ResourceKey`, and then `DeviceDetectionCloudEngineBuilder`. They send their
requests through a client of your own, given as
`PipelineBuilder::new().add_resource(Arc::new(client))` where `client` is an
`Arc<dyn CloudHttpClient>`, or through the built-in client of the
`reqwest-client` feature, which is off by default. A client given in any other
form, such as `add_resource(Arc::new(MyClient))`, is not found, so the build
uses the built-in client, or fails without the feature. `SetShareUsage` set
to false in the pipeline's own `BuildParameters` turns
usage sharing off, as does `ShareUsage` in `PipelineBuilderParameters`, the
older name .NET still reads. A builder this crate does not have, such as
.NET's `AppleProfileEngineBuilder`, fails the build with the message .NET
gives for a builder it cannot find. The
[pipeline configuration specification](https://github.com/51Degrees/specifications/blob/main/pipeline-specification/features/pipeline-configuration.md)
describes the file.

## License

Licensed under the European Union Public Licence v1.2 (EUPL-1.2). See the
[repository](https://github.com/51Degrees/51degrees-rust) for the full text.

## Find out more

- Source and issues: [github.com/51Degrees/51degrees-rust](https://github.com/51Degrees/51degrees-rust)
- API documentation: [docs.rs/fiftyone-device-detection](https://docs.rs/fiftyone-device-detection)
- Device detection: [51degrees.com/device-detection](https://51degrees.com/device-detection?utm_source=docs.rs&utm_medium=package&utm_campaign=51degrees-rust&utm_content=fiftyone-device-detection-readme.md&utm_term=device-detection)
- Device detection documentation: [51degrees.com/documentation](https://51degrees.com/documentation/_device_detection__quickstart.html?utm_source=docs.rs&utm_medium=package&utm_campaign=51degrees-rust&utm_content=fiftyone-device-detection-readme.md&utm_term=documentation)
- Get a resource key: [51degrees.com/configure](https://51degrees.com/configure/?utm_source=docs.rs&utm_medium=package&utm_campaign=51degrees-rust&utm_content=fiftyone-device-detection-readme.md&utm_term=configure)
- About 51Degrees: [51degrees.com](https://51degrees.com/?utm_source=docs.rs&utm_medium=package&utm_campaign=51degrees-rust&utm_content=fiftyone-device-detection-readme.md&utm_term=about)
- Data files and pricing: [51degrees.com/pricing](https://51degrees.com/pricing?utm_source=docs.rs&utm_medium=package&utm_campaign=51degrees-rust&utm_content=fiftyone-device-detection-readme.md&utm_term=pricing)
