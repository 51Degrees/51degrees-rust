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

//! The on-premise Hash device-detection engine.
//!
//! [`DeviceDetectionOnPremiseEngine`] loads a Hash data file through the safe
//! [`fiftyone_native::dd`] wrapper, runs detection on each request, and writes a
//! [`DeviceDataBase`] (the shared type both the on-premise and cloud engines
//! populate) into the flow data under
//! [`DEVICE_DATA_KEY`](fiftyone_device_detection_shared::DEVICE_DATA_KEY).

use std::collections::HashMap;
use std::ffi::CStr;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};

use arc_swap::ArcSwap;
use chrono::{DateTime, TimeZone, Utc};

use fiftyone_device_detection_shared::{
    declared_property_value_type, DeviceDataBase, DevicePropertyReader, UachJsConversionElement,
    DEVICE_DATA_KEY, DEVICE_ELEMENT_DATA_KEY, UACH_EVIDENCE_COOKIE_KEY, UACH_EVIDENCE_QUERY_KEY,
};
use fiftyone_native::dd::{Manager, Results};
use fiftyone_native::PerformanceProfile;
use fiftyone_pipeline_core::{
    constants, Error, Evidence, EvidenceKeyFilter, EvidenceKeyFilterWhitelist, FlowData,
    FlowElement, PropertyMetaData, PropertyValue, PropertyValueType, Result,
};
use fiftyone_pipeline_engines::{
    AspectEngine, AspectEngineDataFile, AspectPropertyMetaData, DataFileConfiguration,
    EngineDeployment, MissingPropertySource, OnPremiseAspectEngine,
};

use crate::operational_copy::OperationalCopy;
use crate::retained::Retained;

/// The separator used to join multiple values of a list-typed property into a
/// single string when reading from the native results. The other ports use a
/// vertical bar between values, which the shared device-data list accessor then
/// splits back out if needed.
const VALUE_SEPARATOR: &str = "|";

/// [`VALUE_SEPARATOR`] as the C string the native reader takes.
const NATIVE_VALUE_SEPARATOR: &CStr = c"|";

/// The data-source tier reported when the native tier could not be read from the
/// data set. The Lite data file is the free tier, so this is the safe default
/// for a freely distributed engine.
const DEFAULT_DATA_SOURCE_TIER: &str = "Lite";

/// The engine's type name, which the message for a property that needs a data
/// file upgrade names.
const ENGINE_TYPE_NAME: &str = "DeviceDetectionOnPremiseEngine";

/// The match-metric pseudo-properties the Hash engine exposes in addition to the
/// data file's own properties. Each is read from the native results just like a
/// data-file property and carries the value type used to store it.
///
/// `DeviceId` and `Method` are strings, `UserAgents` is a list of strings and
/// the remaining metrics are integers. They are advertised so a caller
/// enumerating [`AspectEngine::aspect_properties`] sees them, and so the typed
/// match-metric accessors on [`fiftyone_device_detection_shared::DeviceData`]
/// resolve.
const METRIC_PROPERTIES: &[(&str, PropertyValueType, &str)] = &[
    (
        "MatchedNodes",
        PropertyValueType::Integer,
        "Indicates the number of hash nodes matched within the evidence.",
    ),
    (
        "Difference",
        PropertyValueType::Integer,
        "Used when the detection method is not Exact or None. The larger the \
         value the less confident the detector is in the result.",
    ),
    (
        "Drift",
        PropertyValueType::Integer,
        "Total difference in character positions where the substring hashes \
         were found away from where they were expected.",
    ),
    (
        "Iterations",
        PropertyValueType::Integer,
        "The number of graph nodes visited in order to find a match.",
    ),
    (
        "DeviceId",
        PropertyValueType::String,
        "Four profile ids separated by hyphens in the form \
         Hardware-Platform-Browser-IsCrawler.",
    ),
    (
        "Method",
        PropertyValueType::String,
        "The method used to determine the match result: NONE, PERFORMANCE, \
         COMBINED or PREDICTIVE.",
    ),
    (
        "UserAgents",
        PropertyValueType::StringList,
        "The matched User-Agents.",
    ),
];

/// An on-premise Hash device-detection engine.
///
/// Holds a loaded Hash data set (behind an [`ArcSwap`] so [`Self::refresh`] can
/// hot-swap a reloaded data file while in-flight detections keep the old data
/// set alive), the engine's published property and evidence-key metadata, and
/// the [`AspectEngineDataFile`] run-time state the
/// [`fiftyone_pipeline_engines::DataUpdateService`] tracks.
///
/// Build one with [`DeviceDetectionOnPremiseEngineBuilder`](crate::DeviceDetectionOnPremiseEngineBuilder).
///
/// # Interface compatibility with the cloud engine
///
/// On `process` this engine writes a
/// [`DeviceDataBase`](fiftyone_device_detection_shared::DeviceDataBase) under
/// [`DEVICE_DATA_KEY`](fiftyone_device_detection_shared::DEVICE_DATA_KEY), the
/// same type and key the cloud engine uses. A consuming application can swap one
/// engine for the other without changing how it reads the result.
///
/// # Values read on request
///
/// The device data keeps the native results of its detection and reads each
/// property from them the first time it is requested, so a caller that reads
/// one property does not pay to read the rest. The results, and the data set
/// they came from, stay alive until the device data is dropped, so this engine
/// never caches results. Only the cloud path, whose data is plain owned values,
/// may use a results cache.
pub struct DeviceDetectionOnPremiseEngine {
    /// The loaded Hash data set and the properties a detection reads from it,
    /// swapped together atomically on refresh. In-flight
    /// [`fiftyone_native::dd::Results`] hold their own [`Arc`] to the manager
    /// they were created from, so a swap does not invalidate them.
    loaded: ArcSwap<LoadedDataSet>,

    /// The performance profile used to (re)load the data file, retained so a
    /// refresh reloads with the same profile.
    profile: PerformanceProfile,

    /// The properties the engine was restricted to, or empty for all
    /// properties. Used when reloading the data set on refresh.
    requested_properties: Vec<String>,

    /// The property metadata and tier of the loaded data set, for
    /// [`FlowElement::properties`], [`AspectEngine::aspect_properties`] and
    /// [`AspectEngine::data_source_tier`].
    metadata: Retained<EngineMetadata>,

    /// The tier the builder set, which every data set loaded reports, or
    /// `None` to report the tier each data file names.
    tier_override: Option<String>,

    /// Held while a refresh loads a data set, so refreshes run one at a time.
    refresh_lock: Mutex<()>,

    /// The evidence keys this engine reads, replaced on refresh.
    evidence_key_filter: HashEvidenceKeyFilter,

    /// The expected concurrency the data set's handle pool was sized for, or
    /// `None` for the profile default. Carried so a reload re-opens the data set
    /// with the same pool size.
    concurrency: Option<u16>,

    /// The folder a refresh copies the data file into, or `None` for the
    /// system's temporary folder.
    temp_dir: Option<PathBuf>,

    /// The single data file's run-time state, shared with the update service.
    data_files: Vec<Arc<AspectEngineDataFile>>,
}

/// A loaded Hash data set, the properties a detection reads from it and the
/// copy of the data file it was read from, if any.
struct LoadedDataSet {
    manager: Arc<Manager>,
    readable: Arc<ReadableProperties>,
    /// Why a property is missing, from the metadata of this data set, added to
    /// every device data a detection writes.
    missing: Arc<MissingPropertySource>,
    /// Held only to be dropped with the data set, and declared after the
    /// manager, so it is removed after the data set that has it open is freed.
    _copy: Option<OperationalCopy>,
}

/// The property metadata and tier of a loaded data set.
#[derive(PartialEq)]
struct EngineMetadata {
    properties: Vec<PropertyMetaData>,
    aspect_properties: Vec<AspectPropertyMetaData>,
    tier: String,
}

/// The data set `manager` holds, read from `copy` when there is one, and its
/// metadata, reporting `tier_override` when it is set, otherwise the tier the
/// data file names.
fn describe(
    manager: Arc<Manager>,
    copy: Option<OperationalCopy>,
    tier_override: &Option<String>,
) -> (LoadedDataSet, EngineMetadata) {
    // An explicit builder override wins, then the tier read from the native
    // data file header (Lite, Enterprise, TAC and so on), then the safe Lite
    // default when the native name is absent.
    let tier = tier_override
        .clone()
        .or_else(|| manager.data_set_name())
        .unwrap_or_else(|| DEFAULT_DATA_SOURCE_TIER.to_owned());
    let (properties, aspect_properties) = build_property_metadata(&manager, &Some(tier.clone()));
    let readable = Arc::new(ReadableProperties::new(&aspect_properties));
    let missing = Arc::new(
        MissingPropertySource::new(
            DEVICE_ELEMENT_DATA_KEY,
            ENGINE_TYPE_NAME,
            EngineDeployment::OnPremise,
            tier.clone(),
            aspect_properties.clone(),
        )
        .with_typed_properties(is_typed_property),
    );
    (
        LoadedDataSet {
            manager,
            readable,
            missing,
            _copy: copy,
        },
        EngineMetadata {
            properties,
            aspect_properties,
            tier,
        },
    )
}

impl DeviceDetectionOnPremiseEngine {
    /// Build an engine from an already-loaded manager and its configuration.
    ///
    /// Called by the builder once it has opened the data file, or `copy` of
    /// it. Derives the property and evidence-key metadata from the data set,
    /// records the data-file publish time from the file on disk, and assembles
    /// the engine.
    pub(crate) fn from_manager(
        manager: Arc<Manager>,
        copy: Option<OperationalCopy>,
        profile: PerformanceProfile,
        requested_properties: Vec<String>,
        data_file_config: DataFileConfiguration,
        data_source_tier: Option<String>,
        concurrency: Option<u16>,
    ) -> Self {
        let evidence_key_filter = HashEvidenceKeyFilter::new(named_evidence_keys(&manager));
        let (loaded, metadata) = describe(manager, copy, &data_source_tier);

        let data_file = Arc::new(AspectEngineDataFile::new(data_file_config));
        // Record the publish time from the data file on disk, the best signal
        // available without a native published-time accessor. The update
        // service reads it for the `If-Modified-Since` header.
        if let Some(published) = data_file_published_from_disk(data_file.data_file_path()) {
            data_file.set_data_published(published);
        }

        DeviceDetectionOnPremiseEngine {
            loaded: ArcSwap::from_pointee(loaded),
            profile,
            requested_properties,
            metadata: Retained::new(metadata),
            tier_override: data_source_tier,
            refresh_lock: Mutex::new(()),
            evidence_key_filter,
            concurrency,
            temp_dir: None,
            data_files: vec![data_file],
        }
    }

    /// Copy the data file into `temp_dir` on refresh, the folder the builder
    /// copied it into, or the system's temporary folder when `None`.
    pub(crate) fn with_temp_dir(mut self, temp_dir: Option<PathBuf>) -> Self {
        self.temp_dir = temp_dir;
        self
    }

    /// The performance profile the engine loads its data file with.
    pub fn performance_profile(&self) -> PerformanceProfile {
        self.profile
    }

    /// The evidence keys the engine accepts by name, lowercased, being each
    /// header the loaded data file reads as a `header.` and a `query.` key, and
    /// the high-entropy values keys. The engine also accepts any `query.51d_`
    /// or `cookie.51d_` value, which the client-side JavaScript sends back.
    pub fn evidence_keys(&self) -> Vec<String> {
        named_evidence_keys(&self.loaded.load().manager)
    }

    /// The names of the available (required) properties in the loaded data set,
    /// including the match-metric pseudo-properties.
    pub fn available_properties(&self) -> Vec<String> {
        self.metadata
            .current()
            .aspect_properties
            .iter()
            .map(|p| p.name().to_owned())
            .collect()
    }

    /// Run a detection for `data` into new device data.
    ///
    /// Builds a native evidence set from the flow data (preferring the decoded
    /// UACH element-data `sec-ch-ua*` values over the raw headers) and processes
    /// it. The match metrics are stored straight away, and each data file
    /// property is read from the results the first time it is requested.
    fn detect(&self, data: &FlowData) -> Result<DeviceDataBase> {
        // Snapshot the current data set. The snapshot is an `Arc`, so an
        // in-flight detection keeps the data set alive even if `refresh` swaps a
        // new one in mid-flight.
        let loaded = self.loaded.load_full();
        let mut results = loaded.manager.create_results()?;

        // Marshal the evidence the engine understands, preferring decoded UACH
        // values, into one merged evidence set, then process it.
        let evidence = self.collect_evidence(data);
        results.process_evidence(&evidence)?;

        let mut device = DeviceDataBase::new();
        device
            .base_mut()
            .add_missing_property_source(Arc::clone(&loaded.missing));

        // The device id is a match metric, not a data-file property, so the
        // reader does not return it. Read it through the dedicated native
        // getter and store it under its metric name so the shared `device_id`
        // accessor resolves it. An absent id (no match) is simply not written.
        if let Some(device_id) = results.device_id()? {
            device.insert("DeviceId", PropertyValue::String(device_id));
        }

        // The numeric match metrics and the match method are also computed
        // during detection and live on the result, not in the data file's
        // property values, so the reader does not return them.
        // Read them from the result and store them under their metric names so
        // the shared accessors (difference, method, and so on) resolve them.
        if let Some(metrics) = results.match_metrics() {
            device.insert(
                "MatchedNodes",
                PropertyValue::Integer(metrics.matched_nodes as i64),
            );
            device.insert(
                "Difference",
                PropertyValue::Integer(metrics.difference as i64),
            );
            device.insert("Drift", PropertyValue::Integer(metrics.drift as i64));
            device.insert(
                "Iterations",
                PropertyValue::Integer(metrics.iterations as i64),
            );
            device.insert(
                "Method",
                PropertyValue::String(metrics.method_name().to_owned()),
            );
        }
        device.insert(
            "UserAgents",
            PropertyValue::StringList(results.matched_user_agents()),
        );

        Ok(device.with_reader(DetectionReader {
            results: Mutex::new(results),
            loaded,
        }))
    }

    /// Collect the evidence the engine reads into a single [`Evidence`] set.
    ///
    /// The decoded UACH `sec-ch-ua*` values produced by
    /// [`UachJsConversionElement`] take precedence over the raw request headers,
    /// matching the high-entropy decoder specification: a client that supplied
    /// the high-entropy blob gets the richer decoded hints. Raw evidence the
    /// engine understands (the User-Agent and any header the data set reads) is
    /// added first, then the decoded UACH values overwrite the matching headers.
    fn collect_evidence(&self, data: &FlowData) -> Evidence {
        let mut builder = Evidence::builder();

        // Raw request evidence the engine's filter accepts.
        for (key, value) in data.evidence().iter() {
            if self.evidence_key_filter.include(key) {
                builder = builder.add(key, value);
            }
        }

        // Decoded UACH values, written as header evidence so they override the
        // matching raw headers. The element stores them under its own data key.
        if let Some(uach) = data.get_str(UachJsConversionElement::DATA_KEY.name()) {
            for header in uach.keys() {
                if let Ok(value) = uach.get(&header) {
                    if let Some(text) = value.as_str() {
                        let key = format!(
                            "{}{}{}",
                            constants::EVIDENCE_HTTP_HEADER_PREFIX,
                            constants::EVIDENCE_SEPARATOR,
                            header
                        );
                        builder = builder.add(key, text.to_owned());
                    }
                }
            }
        }

        builder.build()
    }

    /// Reload the data set from the data file on disk and atomically swap it in.
    ///
    /// Shared by [`OnPremiseAspectEngine::refresh`] and the builder's reload
    /// path. A successful reload replaces the live data set, the properties a
    /// detection reads, the evidence keys and the metadata, because a new data
    /// file can carry different properties or a different tier. In-flight
    /// detections keep using the data set they snapshotted. A failed reload
    /// changes nothing.
    fn reload_from_path(&self, path: &Path) -> Result<()> {
        let _refreshing = self
            .refresh_lock
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let copy = self.data_files.first().and_then(|file| {
            OperationalCopy::when_needed(
                path,
                self.temp_dir.as_deref(),
                self.profile,
                file.configuration(),
            )
        });
        let manager = open_manager(
            copy.as_ref().map_or(path, OperationalCopy::path),
            self.profile,
            &self.requested_properties,
            self.concurrency,
        )?;
        self.evidence_key_filter
            .replace(named_evidence_keys(&manager));
        let (loaded, metadata) = describe(manager, copy, &self.tier_override);
        self.loaded.store(Arc::new(loaded));
        self.metadata.replace(metadata);

        // Refresh the recorded publish time from the new file.
        if let Some(file) = self.data_files.first() {
            if let Some(published) = data_file_published_from_disk(file.data_file_path()) {
                file.set_data_published(published);
            }
        }
        Ok(())
    }
}

impl FlowElement for DeviceDetectionOnPremiseEngine {
    fn process(&self, data: &mut FlowData) -> Result<()> {
        // Build the device data outside the get_or_add closure so the detection
        // (which borrows the flow data immutably for evidence) finishes before
        // the mutable element-data borrow begins.
        let device = self.detect(data)?;
        data.get_or_add(DEVICE_DATA_KEY, || device)?;
        Ok(())
    }

    fn data_key(&self) -> &str {
        DEVICE_ELEMENT_DATA_KEY
    }

    fn evidence_key_filter(&self) -> &dyn EvidenceKeyFilter {
        &self.evidence_key_filter
    }

    fn properties(&self) -> &[PropertyMetaData] {
        &self.metadata.current().properties
    }
}

impl AspectEngine for DeviceDetectionOnPremiseEngine {
    fn data_source_tier(&self) -> &str {
        &self.metadata.current().tier
    }

    fn deployment(&self) -> EngineDeployment {
        EngineDeployment::OnPremise
    }

    fn aspect_properties(&self) -> &[AspectPropertyMetaData] {
        &self.metadata.current().aspect_properties
    }

    fn engine_type_name(&self) -> &str {
        ENGINE_TYPE_NAME
    }

    /// True for every property `DeviceData` has a typed accessor for.
    fn has_typed_property(&self, property_name: &str) -> bool {
        is_typed_property(property_name)
    }
}

/// Whether `DeviceData` has a typed accessor for the property, matched ignoring
/// case.
fn is_typed_property(property_name: &str) -> bool {
    declared_property_value_type(property_name).is_some()
}

impl OnPremiseAspectEngine for DeviceDetectionOnPremiseEngine {
    fn data_files(&self) -> &[Arc<AspectEngineDataFile>] {
        &self.data_files
    }

    fn refresh(&self, _data_file_identifier: Option<&str>) -> Result<()> {
        // A single-file engine ignores the identifier. Reload from the file's
        // configured path on disk and hot-swap it in. A reload that fails is
        // logged at error level, as .NET does, and returned.
        let path = self
            .data_files
            .first()
            .and_then(|file| file.data_file_path().map(Path::to_path_buf));
        match path {
            Some(path) => self.reload_from_path(&path).inspect_err(|error| {
                log::error!(
                    "Failed to refresh data from '{}'. The engine keeps the data it had. \
                     If this persists, please contact support@51degrees.com. {error}",
                    path.display()
                );
            }),
            None => Err(Error::configuration(format!(
                "Engine '{}' has no data file path to refresh from.",
                self.data_key()
            ))),
        }
    }

    fn refresh_from_memory(&self, _data_file_identifier: Option<&str>, _data: &[u8]) -> Result<()> {
        // The native Hash manager loads from a file path, so an in-memory
        // refresh is not supported by this engine. The data update service
        // routes file-based updates through `refresh` instead.
        Err(Error::configuration(format!(
            "Engine '{}' loads from a data file and does not support refreshing \
             from an in-memory data source.",
            self.data_key()
        )))
    }
}

/// Open the Hash data set with the requested property restriction.
///
/// Shared by the builder's initial build and the engine's reload path so both
/// turn the requested-property list into the native call the same way: an empty
/// list means every property the data file supports, otherwise the data set is
/// restricted to exactly the named properties.
pub(crate) fn open_manager(
    path: &Path,
    profile: PerformanceProfile,
    requested_properties: &[String],
    concurrency: Option<u16>,
) -> Result<Arc<Manager>> {
    let requested: Option<Vec<&str>> = if requested_properties.is_empty() {
        None
    } else {
        Some(requested_properties.iter().map(String::as_str).collect())
    };
    Manager::open_with_options(path, profile, requested.as_deref(), concurrency)
}

/// The data file properties a detection can read, each with the value type its
/// rendered value is converted to.
struct ReadableProperties {
    /// Each property's name as the metadata spells it, and its value type.
    properties: Vec<(String, PropertyValueType)>,
    /// Each property's name in lowercase, in the same order.
    lowercase_names: Vec<String>,
    /// The position of each property in `properties`, by lowercase name.
    by_name: HashMap<String, usize>,
}

impl ReadableProperties {
    /// Every property in the engine's metadata except the match metrics, which
    /// the engine reads from each result straight away.
    fn new(aspect_properties: &[AspectPropertyMetaData]) -> Self {
        let properties: Vec<(String, PropertyValueType)> = aspect_properties
            .iter()
            .filter(|meta| {
                !METRIC_PROPERTIES
                    .iter()
                    .any(|(metric, _, _)| metric.eq_ignore_ascii_case(meta.name()))
            })
            .map(|meta| (meta.name().to_owned(), meta.core().value_type))
            .collect();
        let lowercase_names: Vec<String> = properties
            .iter()
            .map(|(name, _)| name.to_lowercase())
            .collect();
        let by_name = lowercase_names
            .iter()
            .enumerate()
            .map(|(position, name)| (name.clone(), position))
            .collect();
        ReadableProperties {
            properties,
            lowercase_names,
            by_name,
        }
    }
}

/// Reads the properties of one detection from its native results when they are
/// requested.
struct DetectionReader {
    /// The native results. Reading a value writes into them, so each read
    /// holds the lock. Declared first, so they are freed before the data set
    /// they came from.
    results: Mutex<Results>,
    /// The data set snapshot the results came from, which the property indexes
    /// belong to.
    loaded: Arc<LoadedDataSet>,
}

impl DevicePropertyReader for DetectionReader {
    fn property_names(&self) -> &[String] {
        &self.loaded.readable.lowercase_names
    }

    fn position(&self, name: &str) -> Option<usize> {
        self.loaded.readable.by_name.get(name).copied()
    }

    fn read_property(&self, position: usize) -> std::result::Result<Option<PropertyValue>, String> {
        let (name, value_type) = &self.loaded.readable.properties[position];
        let Some(index) = self.loaded.manager.required_property_index(name) else {
            return Ok(None);
        };
        let results = self.results.lock().unwrap_or_else(PoisonError::into_inner);
        let raw = results
            .value_as_string_by_index(index, NATIVE_VALUE_SEPARATOR)
            .map_err(|error| error.to_string())?;
        Ok(raw.and_then(|raw| {
            let raw = if name.eq_ignore_ascii_case(JAVASCRIPT_HARDWARE_PROFILE) {
                wrap_hardware_profile(raw)
            } else {
                raw
            };
            // A typed property whose value does not parse (the `Unknown` or
            // `N/A` no-value sentinels, say) has no value, so its accessor
            // reports a clean no-value.
            native_value(&raw, *value_type)
        }))
    }
}

/// Build the core and aspect property metadata for the loaded data set.
///
/// Combines the data file's own properties (read from the native data set) with
/// the match-metric pseudo-properties the Hash engine always exposes. The data
/// tier (when known) is recorded on each property so missing-property reasoning
/// can tell a data-file upgrade apart from a configuration exclusion.
fn build_property_metadata(
    manager: &Manager,
    data_source_tier: &Option<String>,
) -> (Vec<PropertyMetaData>, Vec<AspectPropertyMetaData>) {
    let tier = data_source_tier
        .clone()
        .unwrap_or_else(|| DEFAULT_DATA_SOURCE_TIER.to_owned());

    let mut aspect = Vec::new();

    // The data file's own properties. They are strings as far as the native
    // string reader is concerned, but the shared device-data accessors coerce
    // the well-known ones (IsMobile, the screen dimensions) to their natural
    // type, so the declared value type here matches that coercion where it is
    // known and defaults to string otherwise.
    for name in manager.property_names() {
        let value_type = known_property_type(&name);
        aspect.push(
            AspectPropertyMetaData::new(&name, DEVICE_ELEMENT_DATA_KEY, value_type)
                .with_data_tiers([tier.clone()]),
        );
    }

    // The match-metric pseudo-properties.
    for (name, value_type, description) in METRIC_PROPERTIES {
        // Skip a metric that the data file already declares as a real property
        // so it is not advertised twice.
        if aspect.iter().any(|p| p.name().eq_ignore_ascii_case(name)) {
            continue;
        }
        aspect.push(
            AspectPropertyMetaData::new(*name, DEVICE_ELEMENT_DATA_KEY, *value_type)
                .with_description(*description)
                .with_data_tiers(["Lite", "Premium", "Enterprise", "TAC", "CloudFree"])
                .map_core(|c| c.with_category("Device Metrics")),
        );
    }

    let core = aspect.iter().map(|p| p.core().clone()).collect();
    (core, aspect)
}

/// The natural value type for a device-detection property name, so the declared
/// metadata matches how the shared accessors read the value back.
///
/// Looked up in the generated property-type table, which the
/// `PropertyGenerator` tool derives from the common metadata, so the full
/// property set is typed (the booleans, the integer and double measurements, and
/// the string lists), not just a hand-picked few. Anything not in the generated
/// set (for example a property only a newer data file carries) is treated as a
/// string, the lossless default for a value read from the native string reader.
fn known_property_type(name: &str) -> PropertyValueType {
    declared_property_value_type(name).unwrap_or(PropertyValueType::String)
}

/// Convert a native string value into the property value of the declared type,
/// or [`None`] when a typed property has no real value.
///
/// The native results reader returns every value as a string. A boolean comes
/// back as the words `True` or `False`; an integer or double as its decimal
/// form. A property with no value for this device comes back as a sentinel such
/// as `Unknown` or `N/A`. For a boolean, integer or double property that
/// sentinel (or any other text that does not parse as the declared type) is
/// surfaced as a no-value by returning [`None`] (the property is then simply not
/// written, so its strongly-typed accessor reports a clean no-value) rather than
/// a wrong-typed string the typed accessor could not unpack. String and
/// string-list properties take the value verbatim, so a string property that
/// resolves to `Unknown` keeps that as its value, matching the other SDKs.
fn native_value(raw: &str, value_type: PropertyValueType) -> Option<PropertyValue> {
    match value_type {
        PropertyValueType::Bool => match raw.trim().to_ascii_lowercase().as_str() {
            "true" | "1" | "yes" => Some(PropertyValue::Bool(true)),
            "false" | "0" | "no" => Some(PropertyValue::Bool(false)),
            _ => None,
        },
        PropertyValueType::Integer => raw.trim().parse::<i64>().ok().map(PropertyValue::Integer),
        PropertyValueType::Double => raw.trim().parse::<f64>().ok().map(PropertyValue::Double),
        PropertyValueType::StringList => {
            // The native reader joins list values with the separator. Split them
            // back into a list so the list accessors see every value.
            let parts: Vec<String> = raw
                .split(VALUE_SEPARATOR)
                .filter(|s| !s.is_empty())
                .map(str::to_owned)
                .collect();
            Some(PropertyValue::StringList(parts))
        }
        _ => Some(PropertyValue::String(raw.to_owned())),
    }
}

/// The property whose data file value is the body of a script that finds the
/// Apple device model on the client.
const JAVASCRIPT_HARDWARE_PROFILE: &str = "JavascriptHardwareProfile";

/// Turn the stored `JavascriptHardwareProfile` body into the script the client
/// runs, as the C++ results wrapper in 51degrees-cxx does.
///
/// The stored body pushes the profile ids it finds onto a `profileIds` array
/// that it does not declare, and does not send them anywhere. The wrapper
/// declares the array first and afterwards writes the ids to the
/// `51D_ProfileIds` cookie, which the engine reads back as evidence. Without
/// the wrapper the script fails on the undeclared array, and the include that
/// carries it stops running. An empty body, which a profile that needs no
/// script has, is left empty.
fn wrap_hardware_profile(body: String) -> String {
    if body.is_empty() {
        return body;
    }
    format!(
        "var profileIds = []\n{body}\n\
         document.cookie = \"51D_ProfileIds=\" + profileIds.join(\"|\")"
    )
}

/// The evidence key filter for the Hash engine.
///
/// It accepts the keys [`named_evidence_keys`] lists for the loaded data set,
/// and also any query or cookie value whose name starts with `51d_`. Those are
/// the values the client-side JavaScript sends back, being a property override
/// such as `query.51d_screenpixelswidth` or the `query.51d_profileids` profile
/// ids. The native engine applies them itself, and ignores a `51d_` name it
/// does not know, so they are passed through by prefix rather than listed one
/// by one. Without them the screen size and other values the browser reports
/// never reach the engine. A refresh replaces the named keys, because a new
/// data file can read different headers.
struct HashEvidenceKeyFilter {
    named: ArcSwap<EvidenceKeyFilterWhitelist>,
}

impl HashEvidenceKeyFilter {
    fn new(keys: Vec<String>) -> Self {
        HashEvidenceKeyFilter {
            named: ArcSwap::from_pointee(EvidenceKeyFilterWhitelist::new(keys)),
        }
    }

    fn replace(&self, keys: Vec<String>) {
        self.named
            .store(Arc::new(EvidenceKeyFilterWhitelist::new(keys)));
    }

    /// True when the key is `query.51d_*` or `cookie.51d_*`, ignoring case.
    fn is_client_side_value(key: &str) -> bool {
        let Some((prefix, field)) = key.split_once(constants::EVIDENCE_SEPARATOR) else {
            return false;
        };
        let known_prefix = prefix.eq_ignore_ascii_case(constants::EVIDENCE_QUERY_PREFIX)
            || prefix.eq_ignore_ascii_case(constants::EVIDENCE_COOKIE_PREFIX);
        known_prefix
            && field
                .get(..constants::FIFTYONE_COOKIE_PREFIX.len())
                .is_some_and(|start| start.eq_ignore_ascii_case(constants::FIFTYONE_COOKIE_PREFIX))
    }
}

impl EvidenceKeyFilter for HashEvidenceKeyFilter {
    fn include(&self, key: &str) -> bool {
        self.named.load().include(key) || Self::is_client_side_value(key)
    }

    fn order(&self, key: &str) -> Option<i32> {
        self.named
            .load()
            .order(key)
            .or_else(|| Self::is_client_side_value(key).then_some(0))
    }

    /// The named keys alone. The client-side values are accepted by pattern
    /// and are query keys, which the list is not used for.
    fn named_keys(&self) -> Option<Vec<String>> {
        self.named.load().named_keys()
    }
}

/// The evidence keys the engine accepts by name for `manager`'s data set, being
/// each header the data set reads as a `header.` and a `query.` key (see
/// [`Manager::evidence_keys`]), and the high-entropy values keys the
/// [`UachJsConversionElement`] consumes.
fn named_evidence_keys(manager: &Manager) -> Vec<String> {
    let mut keys = manager.evidence_keys();
    keys.push(UACH_EVIDENCE_QUERY_KEY.to_owned());
    keys.push(UACH_EVIDENCE_COOKIE_KEY.to_owned());
    keys
}

/// Read the publish time of the data file from its filesystem modified time.
///
/// The native wrapper does not expose the data set's embedded published date, so
/// the file's modified time on disk is used as a stand-in. It feeds the update
/// service's `If-Modified-Since` decision, where a recent local file correctly
/// suppresses an immediate re-download. Returns [`None`] when the file has no
/// path or its metadata cannot be read.
fn data_file_published_from_disk(path: Option<&Path>) -> Option<DateTime<Utc>> {
    let modified = std::fs::metadata(path?).ok()?.modified().ok()?;
    let since_epoch = modified.duration_since(std::time::UNIX_EPOCH).ok()?;
    Utc.timestamp_opt(since_epoch.as_secs() as i64, 0).single()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_engine_type_name_is_the_name_of_the_type() {
        let full = std::any::type_name::<DeviceDetectionOnPremiseEngine>();
        assert!(full.ends_with(&format!("::{ENGINE_TYPE_NAME}")), "{full}");
    }

    #[test]
    fn native_bool_words_parse_both_cases() {
        // The native string reader renders a boolean as the word True or False.
        assert_eq!(
            native_value("True", PropertyValueType::Bool),
            Some(PropertyValue::Bool(true))
        );
        assert_eq!(
            native_value("false", PropertyValueType::Bool),
            Some(PropertyValue::Bool(false))
        );
        // An unexpected boolean rendering (including the Unknown no-value
        // sentinel) is a no-value, so it is not written and the typed accessor
        // reports a clean no-value rather than failing to unpack a string.
        assert_eq!(native_value("maybe", PropertyValueType::Bool), None);
        assert_eq!(native_value("Unknown", PropertyValueType::Bool), None);
    }

    #[test]
    fn native_numbers_parse_and_fall_back() {
        assert_eq!(
            native_value("1170", PropertyValueType::Integer),
            Some(PropertyValue::Integer(1170))
        );
        assert_eq!(
            native_value("71.5", PropertyValueType::Double),
            Some(PropertyValue::Double(71.5))
        );
        // A non-numeric value for a numeric property is a no-value, not a string.
        assert_eq!(native_value("n/a", PropertyValueType::Integer), None);
        assert_eq!(native_value("Unknown", PropertyValueType::Double), None);
    }

    #[test]
    fn native_string_keeps_sentinel_verbatim() {
        // A string property takes its value verbatim, including the Unknown
        // sentinel, matching the other SDKs.
        assert_eq!(
            native_value("Unknown", PropertyValueType::String),
            Some(PropertyValue::String("Unknown".to_owned()))
        );
    }

    #[test]
    fn native_list_splits_on_separator() {
        let value = native_value("iPhone|iPhone 15", PropertyValueType::StringList);
        assert_eq!(
            value,
            Some(PropertyValue::StringList(vec![
                "iPhone".to_owned(),
                "iPhone 15".to_owned()
            ]))
        );
    }

    #[test]
    fn the_native_separator_is_the_value_separator() {
        assert_eq!(NATIVE_VALUE_SEPARATOR.to_str(), Ok(VALUE_SEPARATOR));
    }

    #[test]
    fn known_property_types_are_coerced() {
        assert_eq!(known_property_type("IsMobile"), PropertyValueType::Bool);
        assert_eq!(known_property_type("ismobile"), PropertyValueType::Bool);
        assert_eq!(
            known_property_type("ScreenPixelsWidth"),
            PropertyValueType::Integer
        );
        assert_eq!(
            known_property_type("ScreenMMWidth"),
            PropertyValueType::Double
        );
        // An unknown property is a string, the lossless default.
        assert_eq!(
            known_property_type("HardwareVendor"),
            PropertyValueType::String
        );
    }

    #[test]
    fn evidence_filter_takes_the_keys_it_is_given_and_replaces_them() {
        let filter = HashEvidenceKeyFilter::new(vec![String::from("header.user-agent")]);
        assert!(filter.include("Header.User-Agent"));
        assert!(!filter.include("header.x-operamini-phone-ua"));
        filter.replace(vec![String::from("header.x-operamini-phone-ua")]);
        assert!(filter.include("header.x-operamini-phone-ua"));
        assert!(!filter.include("header.user-agent"));
    }

    #[test]
    fn hardware_profile_is_wrapped_as_the_cpp_wrapper_does() {
        assert_eq!(
            wrap_hardware_profile("profileIds.push(1)".to_owned()),
            "var profileIds = []\nprofileIds.push(1)\n\
             document.cookie = \"51D_ProfileIds=\" + profileIds.join(\"|\")"
        );
        // A profile that needs no script keeps an empty body.
        assert_eq!(wrap_hardware_profile(String::new()), "");
    }

    #[test]
    fn evidence_filter_passes_client_side_values() {
        // The values the client-side JavaScript sends back reach the engine,
        // whether posted as query values or kept in cookies.
        let filter = HashEvidenceKeyFilter::new(Vec::new());
        assert!(filter.include("query.51d_screenpixelswidth"));
        assert!(filter.include("query.51D_ProfileIds"));
        assert!(filter.include("cookie.51d_screenpixelsheight"));
        assert_eq!(filter.order("query.51d_screenpixelswidth"), Some(0));
        // Only query and cookie values, and only with the 51d_ prefix.
        assert!(!filter.include("header.51d_screenpixelswidth"));
        assert!(!filter.include("query.screenpixelswidth"));
        assert!(!filter.include("cookie.session"));
        assert!(!filter.include("query.51d"));
        assert_eq!(filter.order("cookie.session"), None);
    }
}
