/* *********************************************************************
 * This Original Work is copyright of 51 Degrees Mobile Experts Limited.
 * Copyright 2026 51 Degrees Mobile Experts Limited, Davidson House,
 * Forbury Square, Reading, Berkshire, United Kingdom RG1 3EU.
 *
 * This Original Work is licensed under the European Union Public Licence
 * (EUPL) v.1.2 and is subject to its terms as set out below.
 *
 * If a copy of the EUPL was not distributed with this file, You can obtain
 * one at https://opensource.org/licenses/EUPL-1.2.
 *
 * The 'Compatible Licences' set out in the Appendix to the EUPL (as may be
 * amended by the European Commission) shall be deemed incompatible for
 * the purposes of the Work and the provisions of the compatibility
 * clause in Article 5 of the EUPL shall not apply.
 *
 * If using the Work as, or as part of, a network application, by
 * including the attribution notice(s) required under Article 5 of the EUPL
 * in the end user terms of the application under an appropriate heading,
 * such notice(s) shall fulfill the requirements of that article.
 * ********************************************************************* */

//! @page dd-onprem-update-data-file-example Update Data File (Device Detection, On-premise)
//!
//! On-premise data-file-update console example. See the descriptive block at the
//! bottom of this file for the full write-up.
//!
//! @snippet dd-onprem-update-data-file.rs example

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context, Result};

use fiftyone_device_detection::{DeviceDetectionOnPremiseEngineBuilder, PerformanceProfile};
use fiftyone_pipeline_engines::{DataFileConfiguration, DataUpdateService, OnPremiseAspectEngine};

use device_detection_examples::ENTERPRISE_PROPERTIES;

/// Where to ask about a license key and a copy of the Enterprise data file.
const CONTACT_US_URL: &str = "https://51degrees.com/contact-us?utm_source=code&utm_medium=example&utm_campaign=51degrees-rust&utm_content=examples-device-detection-examples-src-bin-dd-onprem-update-data-file.rs&utm_term=enterprise-data-file";

/// The links the example's output ends with.
const FIND_OUT_MORE: &[(&str, &str)] = &[
    ("Device Detection", "https://51degrees.com/device-detection?utm_source=code&utm_medium=example&utm_campaign=51degrees-rust&utm_content=examples-device-detection-examples-src-bin-dd-onprem-update-data-file.rs&utm_term=find-out-more"),
    ("Pricing", "https://51degrees.com/pricing?utm_source=code&utm_medium=example&utm_campaign=51degrees-rust&utm_content=examples-device-detection-examples-src-bin-dd-onprem-update-data-file.rs&utm_term=find-out-more"),
    ("Documentation", "https://51degrees.com/documentation/index.html?utm_source=code&utm_medium=example&utm_campaign=51degrees-rust&utm_content=examples-device-detection-examples-src-bin-dd-onprem-update-data-file.rs&utm_term=find-out-more"),
];

/// A 51Degrees distributor URL template for downloading an Enterprise Hash data
/// file. The license key is appended by a real deployment. It is shown here only
/// to document the remote-polling configuration; this example does not download.
const DISTRIBUTOR_URL_TEMPLATE: &str =
    "https://distributor.51degrees.com/api/v2/download?Type=HashV41&Download=True&Product=V4Enterprise";

/// Options the example runs with, so `main` and the test share one entry point.
pub struct ExampleOptions {
    /// The path to the on-premise Hash data file the engine loads.
    pub data_file: PathBuf,
    /// An optional license key for a real Enterprise update. When absent the
    /// example only illustrates the update mechanisms without downloading.
    pub license_key: Option<String>,
}

// [example]
/// Run the on-premise data-file-update example.
///
/// Illustrates the four ways an on-premise data file is kept current:
/// update-on-startup, the file-system watcher, remote-update polling, and a
/// programmatic (on-demand) update. It runs the programmatic path for real
/// against the supplied file (a safe no-network reload from disk) and documents
/// the configuration for the other three. It ends by saying which Enterprise
/// properties the data file does not carry, with where to ask about a copy,
/// and the "Find out more" links.
pub fn run(options: ExampleOptions) -> Result<()> {
    describe_update_mechanisms(options.license_key.as_deref());

    // The data update service drives all four mechanisms. One service instance is
    // shared across the engines an application builds; here a single engine uses
    // it. Dropping the service stops its background scheduler and releases any
    // file-system watchers.
    let service = Arc::new(DataUpdateService::new());

    // Build an engine wired to the service with a configuration that enables the
    // file-system watcher and remote polling but NOT update-on-startup, so
    // building the engine does not block on a network call. The Lite file shipped
    // with this example cannot actually be auto-updated (auto-update needs an
    // Enterprise license and a downloadable file), so this demonstrates the
    // wiring rather than performing a download.
    let config = DataFileConfiguration::builder(&options.data_file)
        // Off so `build` does not block; see the startup section in the docs.
        .update_on_startup(false)
        // Watch the file on disk and reload when it is replaced, while
        // automatic updates are on.
        .file_system_watcher_enabled(true)
        // Poll the distributor for a newer file on an interval. Automatic
        // updates are the master switch for polling and the watcher alike.
        .automatic_updates_enabled(options.license_key.is_some())
        // A short interval purely so the example does not appear idle; a real
        // deployment uses around 30 minutes.
        .polling_interval_seconds(30 * 60)
        // Stagger many instances so they do not all poll at once.
        .max_randomisation_seconds(10 * 60)
        // The license keys a real update URL is built from. Empty here unless a
        // key was supplied.
        .data_update_license_keys(options.license_key.iter().cloned())
        .build();

    let engine = DeviceDetectionOnPremiseEngineBuilder::new(&options.data_file)
        .performance_profile(PerformanceProfile::LowMemory)
        .auto_update(config, Some(Arc::clone(&service)))
        .build()
        .context("failed to build the auto-update engine")?;

    println!();
    println!("Engine built and its data file registered with the update service.");
    if let Some(published) = engine.data_file_published() {
        println!("Current data file published: {published}");
    }
    println!(
        "Data files registered with the update service: {}",
        service.registered_count()
    );

    // --- Programmatic (on-demand) update -------------------------------------
    //
    // Trigger an immediate update check. With no remote URL configured (the case
    // here, since the Lite file has none), the service refreshes the engine from
    // the file currently on disk, which is a safe no-network operation that
    // demonstrates the reload path. A `Success` status means the engine swapped
    // in a freshly read data set.
    println!();
    println!("Triggering a programmatic update check ...");
    let as_engine: Arc<dyn OnPremiseAspectEngine> = engine.clone();
    match service.check_for_update(&as_engine, None) {
        Ok(status) => println!("Programmatic update check status: {status:?}"),
        Err(error) => {
            // A failed check is expected when a real remote update is attempted
            // without a valid license, so it is reported, not fatal.
            println!("Programmatic update check did not apply an update: {error}");
        }
    }

    // --- Direct programmatic refresh -----------------------------------------
    //
    // An application can also reload the engine directly, bypassing the service,
    // for example after it has placed a new file on disk itself. This reloads the
    // same Lite file, proving the hot-swap works while in-flight detections keep
    // using the data set they snapshotted.
    println!();
    println!("Performing a direct engine refresh from the file on disk ...");
    engine
        .refresh(None)
        .context("the direct engine refresh should succeed")?;
    println!("Direct refresh complete; the engine reloaded its data file.");
    if let Some(published) = engine.data_file_published() {
        println!("Data file published after refresh: {published}");
    }

    // Print the standard data-file warnings, as the other examples do. The engine
    // is already in hand, so the engine-taking helper prints the same lines
    // without building a second engine.
    println!();
    device_detection_examples::print_data_file_warnings_for(engine.as_ref());

    // The engine's properties are those its data file carries, so they say
    // which Enterprise properties an update to the Enterprise data file adds.
    let available = engine.available_properties();
    println!();
    println!("--- Enterprise properties ---");
    for name in ENTERPRISE_PROPERTIES {
        let state = if examples_shared::is_available(name, &available) {
            "in this data file"
        } else {
            examples_shared::NEEDS_ENTERPRISE_DATA_FILE
        };
        println!("\t{name:<22}: {state}");
    }
    let missing = examples_shared::missing_properties(ENTERPRISE_PROPERTIES, &available);
    if let Some(message) = examples_shared::enterprise_data_file_message(&missing, CONTACT_US_URL) {
        println!();
        println!("{message}");
    }

    examples_shared::print_find_out_more(FIND_OUT_MORE, examples_shared::ON_PREMISE_REPOSITORIES);

    // Dropping `service` here stops the background scheduler and releases the
    // file-system watcher cleanly.
    Ok(())
}
// [example]

/// Print a description of the four update mechanisms and how each is configured,
/// so the example documents them even on a run that cannot download.
fn describe_update_mechanisms(license_key: Option<&str>) {
    println!("--- On-premise data file update mechanisms ---");
    println!(
        "1. Update on start-up. Set update_on_startup(true) on the data-file \
         configuration. Building the engine then blocks while the service polls \
         the distributor once, so the engine starts with current data. Requires \
         an Enterprise license key and a writable file location."
    );
    println!(
        "2. File-system watcher. Set file_system_watcher_enabled(true), with \
         automatic_updates_enabled(true), the master switch for automatic \
         updates. The service watches the data file on disk and reloads the \
         engine when the file is replaced, which is how a manually-downloaded \
         file is picked up."
    );
    println!(
        "3. Remote-update polling. Set automatic_updates_enabled(true), a \
         polling_interval_seconds (around 30 minutes in production) and a \
         max_randomisation_seconds (around 10 minutes) to stagger instances. The \
         service polls the distributor URL, sending If-Modified-Since so an \
         unchanged file is a cheap 304, and applies any newer file. Requires an \
         Enterprise license key."
    );
    println!(
        "4. Programmatic update. Call DataUpdateService::check_for_update to force \
         an immediate check, or OnPremiseAspectEngine::refresh to reload from disk \
         directly. Useful after placing a file yourself or on an external trigger."
    );
    match license_key {
        Some(_) => println!(
            "A license key was supplied, so remote polling is enabled and the URL \
             would be built from the template: {DISTRIBUTOR_URL_TEMPLATE}&LicenseKeys=<key>."
        ),
        None => {
            println!(
                "No license key was supplied, so this run only illustrates the wiring \
                 and exercises the no-network programmatic refresh. Set a key (argument \
                 or _51DEGREES_LICENSE_KEY) and point at an Enterprise file to perform a \
                 real download. The distributor URL template is: {DISTRIBUTOR_URL_TEMPLATE}."
            );
            println!(
                "Automatic updates need a license key for the Enterprise data file. \
                 Contact us about getting one at {CONTACT_US_URL}"
            );
        }
    }
}

/// Resolve the data file then run the example. The data file uses the usual
/// fallbacks (argument, env var, Lite file); the license key comes from
/// the second argument or `_51DEGREES_LICENSE_KEY`. With no data file present the
/// example fails, after the resolver has printed the command that gets the file.
fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let data_file = args
        .next()
        .map(PathBuf::from)
        .or_else(examples_shared::dd_data_path);
    let license_key = args.next().or_else(examples_shared::license_key_from_env);

    let Some(data_file) = data_file else {
        anyhow::bail!(
            "No device-detection data file found. Set _51DEGREES_DD_PATH, or pass \
             the path as the first argument."
        );
    };

    run(ExampleOptions {
        data_file,
        license_key,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Run the example against the Lite Hash file with no license key, exercising
    /// the no-network programmatic-update and direct-refresh paths. Skipped when
    /// no data file is present.
    #[test]
    fn runs_against_the_lite_data_file() {
        let Some(data_file) = examples_shared::dd_data_path() else {
            eprintln!("skipping: no on-premise data file found (set _51DEGREES_DD_PATH)");
            return;
        };
        run(ExampleOptions {
            data_file: data_file.clone(),
            // No license key, so the test never attempts a real download.
            license_key: None,
        })
        .expect("the on-premise update-data-file example should complete");

        // The Lite file carries none of the Enterprise properties, so the run
        // names them all with the contact link. _51DEGREES_DD_PATH can name
        // another file.
        if data_file.ends_with(examples_shared::DD_LITE_RELATIVE_PATH) {
            let available = device_detection_examples::data_file_properties(&data_file)
                .expect("the data file opens");
            assert_eq!(
                examples_shared::missing_properties(ENTERPRISE_PROPERTIES, &available),
                ENTERPRISE_PROPERTIES
            );
        }
    }
}

/*
 * @example dd-onprem-update-data-file.rs
 *
 * The device-detection on-premise data-file-update console example. It
 * illustrates the parameters that control when a new on-premise data file is
 * sought and when it is loaded.
 *
 * Four update mechanisms are demonstrated, all driven by the
 * `DataUpdateService`:
 *
 * - Update on start-up. With `update_on_startup(true)` on the data-file
 *   configuration, building the engine blocks while the service polls the
 *   51Degrees distributor once, so the engine starts with the most recent data.
 *   This needs an Enterprise license key and a writable file location.
 * - File-system watcher. With `file_system_watcher_enabled(true)` and
 *   automatic updates on, the service watches the data file on disk and
 *   reloads the engine whenever the file is replaced. This is how a file you
 *   download and drop into place yourself is picked up automatically.
 * - Remote-update polling. With `automatic_updates_enabled(true)`, a polling
 *   interval (around 30 minutes in production) and a randomisation window (around
 *   10 minutes, to stagger many instances), the service periodically polls the
 *   distributor URL with an `If-Modified-Since` header and applies any newer
 *   file. This needs an Enterprise license key.
 * - Programmatic update. `DataUpdateService::check_for_update` forces an immediate
 *   check on demand, and `OnPremiseAspectEngine::refresh` reloads from disk
 *   directly, which is useful after placing a file yourself or on an external
 *   trigger.
 *
 * The example builds an engine wired to a `DataUpdateService` with the watcher
 * and polling configured (but update-on-startup off, so building does not block),
 * then exercises the two no-network paths for real against the supplied file: a
 * programmatic `check_for_update` (which, with no remote URL on the Lite file,
 * refreshes from the file on disk) and a direct `engine.refresh(None)`. Both
 * hot-swap a freshly read data set while in-flight detections keep using the data
 * set they snapshotted.
 *
 * The free Lite data file cannot actually be auto-updated: automatic updates
 * require an Enterprise license and a downloadable file. So with no license key
 * the example documents the wiring and runs only the safe no-network paths. Supply
 * a key (second argument or the `_51DEGREES_LICENSE_KEY` environment variable) and
 * point at an Enterprise file to perform a real download. With no key the
 * example says that automatic updates need a license key for the Enterprise
 * data file, with a link to contact 51Degrees. It then lists the properties
 * only the Enterprise data file carries (HardwareVendor, HardwareModel,
 * DeviceType, PriceBand and ScreenInchesDiagonal), saying for each whether the
 * engine's data file carries it, names those it lacks with the same link, and
 * ends with a "Find out more" section. An Enterprise license is
 * available from https://51degrees.com/pricing?utm_source=code&utm_medium=example&utm_campaign=51degrees-rust&utm_content=examples-device-detection-examples-src-bin-dd-onprem-update-data-file.rs&utm_term=dd-onprem-update-data-file.
 *
 * The data file is read from the first command-line argument, the
 * `_51DEGREES_DD_PATH` environment variable, or the Lite Hash file in the data
 * repository inside the cxx submodule, which the example downloads when it is
 * missing.
 */
