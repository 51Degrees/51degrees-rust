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

use std::any::Any;
use std::collections::hash_map::RandomState;
use std::collections::HashMap;
use std::hash::BuildHasher;
use std::ops::Range;
use std::os::raw::c_int;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};

use arc_swap::ArcSwap;
use chrono::{TimeZone, Utc};

use fiftyone_device_detection_shared::{
    declared_property_value_type, DeviceDataBase, DevicePropertyReader, UachJsConversionElement,
    DEVICE_DATA_KEY, DEVICE_ELEMENT_DATA_KEY, UACH_EVIDENCE_COOKIE_KEY, UACH_EVIDENCE_QUERY_KEY,
};
use fiftyone_native::dd::{
    DataFileComponent, DataFileProperty, DataFileValue, HashOptions, Manager, MatchMetrics, Results,
};
use fiftyone_native::PerformanceProfile;
use fiftyone_pipeline_core::{
    constants, Error, Evidence, EvidenceKeyFilter, EvidenceKeyFilterWhitelist, FlowData,
    FlowElement, PropertyMetaData, PropertyValue, PropertyValueType, Result,
};
use fiftyone_pipeline_engines::{
    AspectEngine, AspectEngineDataFile, AspectPropertyMetaData, DataFileConfiguration,
    EngineDeployment, MissingPropertyResult, MissingPropertySource, OnPremiseAspectEngine,
};
use fiftyone_pipeline_engines_fiftyone::{
    ComponentMetaData, FiftyOneAspectPropertyMetaData, ValueMetaData,
};

use crate::operational_copy::OperationalCopy;
use crate::retained::Retained;

mod profile_lookup;

/// The separator between the values of a string or JavaScript property that
/// holds several, as the C++ `ResultsBase` in 51degrees-cxx joins them.
const VALUE_SEPARATOR: &str = "|";

/// The data-source tier reported when the native tier could not be read from the
/// data set. The Lite data file is the free tier, so this is the safe default
/// for a freely distributed engine.
const DEFAULT_DATA_SOURCE_TIER: &str = "Lite";

/// The engine's type name, which the message for a property that needs a data
/// file upgrade names.
const ENGINE_TYPE_NAME: &str = "DeviceDetectionOnPremiseEngine";

/// A match-metric pseudo-property, which the Hash engine gives in addition to
/// the data file's own properties. Its value is read from the detection's
/// results when it is first requested, not from the data file.
struct MetricProperty {
    name: &'static str,
    value_type: PropertyValueType,
    description: &'static str,
    default_value: &'static str,
    values: &'static [&'static str],
}

/// The match metrics, in the order and with the descriptions, default values
/// and values the .NET engine gives them. They are listed so a caller
/// enumerating [`AspectEngine::aspect_properties`] sees them, and so the typed
/// match-metric accessors on [`fiftyone_device_detection_shared::DeviceData`]
/// resolve.
const METRIC_PROPERTIES: &[MetricProperty] = &[
    MetricProperty {
        name: "MatchedNodes",
        value_type: PropertyValueType::Integer,
        description: "Indicates the number of hash nodes matched within the evidence.",
        default_value: "0",
        values: &[],
    },
    MetricProperty {
        name: "Difference",
        value_type: PropertyValueType::Integer,
        description: "Used when detection method is not Exact or None. This is an \
                      integer value and the larger the value the less confident the \
                      detector is in this result.",
        default_value: "0",
        values: &[],
    },
    MetricProperty {
        name: "Drift",
        value_type: PropertyValueType::Integer,
        description: "Total difference in character positions where the substrings \
                      hashes were found away from where they were expected.",
        default_value: "0",
        values: &[],
    },
    MetricProperty {
        name: "DeviceId",
        value_type: PropertyValueType::String,
        description: "Consists of four components separated by a hyphen symbol: \
                      Hardware-Platform-Browser-IsCrawler where each Component \
                      represents an ID of the corresponding Profile.",
        default_value: "0-0-0-0",
        values: &[],
    },
    MetricProperty {
        name: "UserAgents",
        value_type: PropertyValueType::StringList,
        description: "The matched User-Agents.",
        default_value: "n/a",
        values: &[],
    },
    MetricProperty {
        name: "Iterations",
        value_type: PropertyValueType::Integer,
        description: "The number of iterations carried out in order to find a match. \
                      This is the number of nodes in the graph which have been visited.",
        default_value: "0",
        values: &[],
    },
    MetricProperty {
        name: "Method",
        value_type: PropertyValueType::String,
        description: "The method used to determine the match result.",
        default_value: "NONE",
        values: &["NONE", "PERFORMANCE", "COMBINED", "PREDICTIVE"],
    },
];

/// The data tiers every match metric is listed in.
const METRIC_DATA_TIERS: [&str; 5] = ["Lite", "Premium", "Enterprise", "TAC", "CloudFree"];

/// The category of every match metric.
const METRIC_CATEGORY: &str = "Device Metrics";

/// The id of the component the match metrics belong to, which no data file
/// component has.
const METRICS_COMPONENT_ID: u8 = 255;

/// The name of the component the match metrics belong to.
const METRICS_COMPONENT_NAME: &str = "Metrics";

/// The display order of every match metric.
const METRIC_DISPLAY_ORDER: u8 = 255;

/// The message a match metric's no-value carries when the results cannot
/// give the metrics.
const METRICS_UNREADABLE: &str =
    "The match metrics could not be read, because a graph could not be read from the data file.";

/// The outcome of reading one property from a detection's results.
type ReadOutcome = std::result::Result<Option<PropertyValue>, String>;

/// A match metric, which a detection reads from its results rather than from
/// the data file's values. Reading one walks every graph not yet walked.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Metric {
    DeviceId,
    MatchedNodes,
    Difference,
    Drift,
    Iterations,
    Method,
    UserAgents,
}

impl Metric {
    /// Every match metric, in the order a detection's data lists them after
    /// the data file's properties, which is the order of
    /// [`METRIC_PROPERTIES`] and of the .NET engine.
    const READ_ORDER: [Metric; 7] = [
        Metric::MatchedNodes,
        Metric::Difference,
        Metric::Drift,
        Metric::DeviceId,
        Metric::UserAgents,
        Metric::Iterations,
        Metric::Method,
    ];

    /// The metric's property name.
    fn name(self) -> &'static str {
        match self {
            Metric::DeviceId => "DeviceId",
            Metric::MatchedNodes => "MatchedNodes",
            Metric::Difference => "Difference",
            Metric::Drift => "Drift",
            Metric::Iterations => "Iterations",
            Metric::Method => "Method",
            Metric::UserAgents => "UserAgents",
        }
    }

    /// Read the metric from `results`. The device id has no value when the
    /// results give none, and the others fail only when a graph cannot be
    /// read from the data file.
    fn read(self, results: &Results) -> ReadOutcome {
        let value: fn(&MatchMetrics) -> PropertyValue = match self {
            Metric::DeviceId => {
                return results
                    .device_id()
                    .map(|id| id.map(PropertyValue::String))
                    .map_err(|error| error.to_string());
            }
            Metric::UserAgents => {
                return Ok(Some(PropertyValue::StringList(
                    results.matched_user_agents(),
                )));
            }
            Metric::MatchedNodes => {
                |metrics| PropertyValue::Integer(i64::from(metrics.matched_nodes))
            }
            Metric::Difference => |metrics| PropertyValue::Integer(i64::from(metrics.difference)),
            Metric::Drift => |metrics| PropertyValue::Integer(i64::from(metrics.drift)),
            Metric::Iterations => |metrics| PropertyValue::Integer(i64::from(metrics.iterations)),
            Metric::Method => |metrics| PropertyValue::String(metrics.method_name().to_owned()),
        };
        results
            .match_metrics()
            .map(|metrics| Some(value(&metrics)))
            .ok_or_else(|| METRICS_UNREADABLE.to_owned())
    }
}

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
    metadata: Retained<LentMetadata>,

    /// The tier the builder set, which every data set loaded reports, or
    /// `None` to report the tier each data file names.
    tier_override: Option<String>,

    /// Held while a refresh loads a data set, so refreshes run one at a time.
    refresh_lock: Mutex<()>,

    /// The evidence keys this engine reads, replaced on refresh.
    evidence_key_filter: HashEvidenceKeyFilter,

    /// The settings the data set opens with over the profile's own, the
    /// expected concurrency always among them. Carried so a reload opens the
    /// data set with the same settings.
    options: HashOptions,

    /// The folder a refresh copies the data file into, or `None` for the
    /// system's temporary folder.
    temp_dir: Option<PathBuf>,

    /// Whether a refresh reads a copy of the data file under every profile,
    /// as the builder's `create_temp_data_copy` asked.
    temp_data_copy: bool,

    /// Whether a refresh reads a copy of the data file already in the
    /// temporary folder, as the builder's `reuse_temp_file` asked.
    reuse_temp_file: bool,

    /// The single data file's run-time state, shared with the update service.
    data_files: Vec<Arc<AspectEngineDataFile>>,
}

/// A loaded Hash data set, the properties a detection reads from it and the
/// copy of the data file it was read from, if any.
struct LoadedDataSet {
    manager: Arc<Manager>,
    readable: Arc<ReadableProperties>,
    /// Each data file property's place among the data file's properties and
    /// its number of values, by name.
    file_properties: HashMap<String, FilePropertyPlace>,
    /// Why a property is missing, from the metadata of this data set, added to
    /// every device data a detection writes.
    missing: Arc<MissingPropertySource>,
    /// The 51Degrees metadata of each property, freed with the data set once
    /// no caller holds it.
    fiftyone_properties: Arc<[FiftyOneAspectPropertyMetaData]>,
    /// The components, each with its properties, freed in the same way.
    components: Arc<[ComponentMetaData]>,
    /// Held only to be dropped with the data set, and declared after the
    /// manager, so it is removed after the data set that has it open is freed.
    _copy: Option<OperationalCopy>,
}

/// Where a data file property is among the data file's properties, and how
/// many values the data file holds for it.
#[derive(Clone, Copy)]
struct FilePropertyPlace {
    /// The index [`Manager::data_file_property_value_range`] takes.
    index: u32,
    value_count: u32,
}

/// The property and component metadata and tier of a loaded data set.
struct EngineMetadata {
    lent: LentMetadata,
    fiftyone_properties: Vec<FiftyOneAspectPropertyMetaData>,
    components: Vec<ComponentMetaData>,
}

/// The part of [`EngineMetadata`] the pipeline traits lend by reference,
/// which [`Retained`] keeps for the engine's life.
#[derive(PartialEq)]
struct LentMetadata {
    properties: Vec<PropertyMetaData>,
    aspect_properties: Vec<AspectPropertyMetaData>,
    tier: String,
}

/// The data set `manager` holds, read from `copy` when there is one, and its
/// metadata, reporting `tier_override` when it is set, otherwise the tier the
/// data file names. The missing property rules always compare with the tier
/// the data file names, because that is what each property's tiers list.
///
/// `copy` comes before `manager` because Rust drops later parameters first,
/// so on an error the data set is freed, closing the copy, before the copy
/// is removed, as [`LoadedDataSet`] orders them.
fn describe(
    copy: Option<OperationalCopy>,
    manager: Arc<Manager>,
    tier_override: &Option<String>,
) -> Result<(LoadedDataSet, LentMetadata)> {
    // An explicit builder override wins, then the tier read from the native
    // data file header (Lite, Enterprise, TAC and so on), then the safe Lite
    // default when the native name is absent.
    let file_tier = manager
        .data_set_name()
        .unwrap_or_else(|| DEFAULT_DATA_SOURCE_TIER.to_owned());
    let tier = tier_override.clone().unwrap_or_else(|| file_tier.clone());
    let file_properties = manager.data_file_properties()?;
    let EngineMetadata {
        lent,
        fiftyone_properties,
        components,
    } = build_metadata(&file_properties, &manager.data_file_components()?, tier);
    let readable = Arc::new(ReadableProperties::new(&file_properties));
    let file_property_places = file_properties
        .iter()
        .map(|property| {
            let place = FilePropertyPlace {
                index: property.index,
                value_count: property.value_count,
            };
            (property.name.clone(), place)
        })
        .collect();
    let missing = Arc::new(
        MissingPropertySource::new(
            DEVICE_ELEMENT_DATA_KEY,
            ENGINE_TYPE_NAME,
            EngineDeployment::OnPremise,
            file_tier,
            lent.aspect_properties.clone(),
        )
        .with_typed_properties(is_typed_property),
    );
    Ok((
        LoadedDataSet {
            manager,
            readable,
            file_properties: file_property_places,
            missing,
            fiftyone_properties: fiftyone_properties.into(),
            components: components.into(),
            _copy: copy,
        },
        lent,
    ))
}

impl DeviceDetectionOnPremiseEngine {
    /// Build an engine from an already-loaded manager and its configuration.
    ///
    /// Called by the builder once it has opened the data file, or `copy` of
    /// it. Derives the property and evidence-key metadata from the data set,
    /// records the dates in the data file header, and assembles the engine.
    /// Fails when the data file's metadata cannot be read. `copy` comes before
    /// `manager` so it is dropped after it, as in `describe`.
    pub(crate) fn from_manager(
        copy: Option<OperationalCopy>,
        manager: Arc<Manager>,
        profile: PerformanceProfile,
        requested_properties: Vec<String>,
        data_file_config: DataFileConfiguration,
        data_source_tier: Option<String>,
        options: HashOptions,
    ) -> Result<Self> {
        let evidence_key_filter = HashEvidenceKeyFilter::new(named_evidence_keys(&manager));
        let (loaded, metadata) = describe(copy, manager, &data_source_tier)?;

        let data_file = Arc::new(AspectEngineDataFile::new(data_file_config));
        record_dates(&loaded.manager, &data_file);

        Ok(DeviceDetectionOnPremiseEngine {
            loaded: ArcSwap::from_pointee(loaded),
            profile,
            requested_properties,
            metadata: Retained::new(metadata),
            tier_override: data_source_tier,
            refresh_lock: Mutex::new(()),
            evidence_key_filter,
            options,
            temp_dir: None,
            temp_data_copy: false,
            reuse_temp_file: false,
            data_files: vec![data_file],
        })
    }

    /// Copy the data file into `temp_dir` on refresh, the folder the builder
    /// copied it into, or the system's temporary folder when `None`, under
    /// every profile when `always` is true, and read a copy already there
    /// when `reuse` is true.
    pub(crate) fn with_temp_dir(
        mut self,
        temp_dir: Option<PathBuf>,
        always: bool,
        reuse: bool,
    ) -> Self {
        self.temp_dir = temp_dir;
        self.temp_data_copy = always;
        self.reuse_temp_file = reuse;
        self
    }

    /// The performance profile the engine loads its data file with.
    pub fn performance_profile(&self) -> PerformanceProfile {
        self.profile
    }

    /// The expected concurrency the engine opens its data file with, being
    /// the one the builder named or else
    /// [`default_concurrency`](fiftyone_native::dd::default_concurrency).
    pub fn concurrency(&self) -> u16 {
        self.options
            .concurrency
            .unwrap_or_else(|| fiftyone_native::dd::default_concurrency(self.profile))
    }

    /// The settings the engine opens its data file with over the profile's
    /// own. The concurrency is always set, to the one the builder named or
    /// else [`default_concurrency`](fiftyone_native::dd::default_concurrency),
    /// each other setting is [`None`] where the builder named none, and
    /// [`HashOptions::indexed_properties`] lists the properties the builder
    /// named to index.
    pub fn hash_options(&self) -> &HashOptions {
        &self.options
    }

    /// The evidence keys the engine accepts by name, lowercased, being each
    /// header the loaded data file reads as a `header.` and a `query.` key, and
    /// the high-entropy values keys. The engine also accepts any `query.51d_`
    /// or `cookie.51d_` value, which the client-side JavaScript sends back.
    pub fn evidence_keys(&self) -> Vec<String> {
        named_evidence_keys(&self.loaded.load().manager)
    }

    /// The names of the properties the loaded data set was opened with, being
    /// those [`AspectEngine::aspect_properties`] marks available, including the
    /// match-metric pseudo-properties.
    pub fn available_properties(&self) -> Vec<String> {
        self.metadata
            .current()
            .aspect_properties
            .iter()
            .filter(|p| p.available())
            .map(|p| p.name().to_owned())
            .collect()
    }

    /// The 51Degrees metadata of each property in
    /// [`AspectEngine::aspect_properties`], in the same order, with the URL,
    /// display order, flags, component and default value the data file gives
    /// it.
    ///
    /// A data file property's values are not copied into its metadata, because
    /// a data file can hold hundreds of thousands of them, so
    /// [`Self::property_values`] reads them when they are asked for. A match
    /// metric's metadata lists its values.
    ///
    /// The list is the loaded data set's. A refresh loads a new list and frees
    /// this one once no caller holds it.
    pub fn fiftyone_properties(&self) -> Arc<[FiftyOneAspectPropertyMetaData]> {
        Arc::clone(&self.loaded.load().fiftyone_properties)
    }

    /// The components the data file defines, such as the hardware and the
    /// browser, each with every property the data file gives it, then the
    /// `Metrics` component, which holds the match metrics.
    ///
    /// The list is the loaded data set's. A refresh loads a new list and frees
    /// this one once no caller holds it.
    pub fn components(&self) -> Arc<[ComponentMetaData]> {
        Arc::clone(&self.loaded.load().components)
    }

    /// The native data set the engine has loaded, for an element that reads
    /// the data file's profiles and their values, as the robots.txt engine
    /// reads the crawlers.
    ///
    /// The data set is the loaded one. A refresh loads a new data set and
    /// frees this one once no caller holds it.
    pub fn data_set(&self) -> Arc<Manager> {
        Arc::clone(&self.loaded.load().manager)
    }

    /// The values the property named `property_name`, matched exactly, can
    /// have, or [`None`] when the engine lists no such property. Fails when
    /// the data file's values cannot be read.
    ///
    /// A data file property's values are read from the loaded data file on
    /// each call, in the data file's order. A match metric gives the values its
    /// metadata lists, which is none for all but `Method`. A data file can
    /// hold over a hundred thousand values for one property, so
    /// [`Self::property_value_range`] reads part of the list.
    pub fn property_values(&self, property_name: &str) -> Result<Option<Vec<ValueMetaData>>> {
        self.property_value_range(property_name, 0..usize::MAX)
    }

    /// The values at the places `range` covers among those
    /// [`Self::property_values`] gives for the property named
    /// `property_name`, so a caller that shows the first 20 reads only 20.
    /// The part of `range` past the last value is left out. [`None`] when
    /// the engine lists no such property.
    pub fn property_value_range(
        &self,
        property_name: &str,
        range: Range<usize>,
    ) -> Result<Option<Vec<ValueMetaData>>> {
        let loaded = self.loaded.load();
        if let Some(place) = loaded.file_properties.get(property_name) {
            let start = u32::try_from(range.start).unwrap_or(u32::MAX);
            let end = u32::try_from(range.end).unwrap_or(u32::MAX);
            let values = loaded
                .manager
                .data_file_property_value_range(place.index, start..end)?;
            return Ok(Some(values.iter().map(value_metadata).collect()));
        }
        Ok(loaded
            .fiftyone_properties
            .iter()
            .find(|property| property.name() == property_name)
            .map(|property| {
                let values = property.values();
                let end = range.end.min(values.len());
                values[range.start.min(end)..end].to_vec()
            }))
    }

    /// The number of values [`Self::property_values`] gives for the property
    /// named `property_name`, or [`None`] when the engine lists no such
    /// property. No value is read to count them.
    pub fn property_value_count(&self, property_name: &str) -> Option<usize> {
        let loaded = self.loaded.load();
        if let Some(place) = loaded.file_properties.get(property_name) {
            return Some(place.value_count as usize);
        }
        loaded
            .fiftyone_properties
            .iter()
            .find(|property| property.name() == property_name)
            .map(|property| property.values().len())
    }

    /// The value named `value_name` among those [`Self::property_values`]
    /// gives for the property named `property_name`, matched as .NET's
    /// `GetValue(valueName)` matches them, or [`None`] when the engine lists
    /// no such property or the property has no such value. Fails when the
    /// data file's values cannot be read.
    ///
    /// [`FiftyOneAspectPropertyMetaData::value`] finds nothing for a data file
    /// property, because its metadata lists no values. This finds the value in
    /// the loaded data file without reading the property's other values, by
    /// the C library's binary search over values in order of their names, as
    /// .NET finds it. So a value the data file holds out of that order can be
    /// missed. The property's name is matched exactly, and a data file
    /// property's value by the text of `value_name` before any null, so
    /// `True\0x` finds `True`.
    pub fn property_value(
        &self,
        property_name: &str,
        value_name: &str,
    ) -> Result<Option<ValueMetaData>> {
        let loaded = self.loaded.load();
        if let Some(place) = loaded.file_properties.get(property_name) {
            let value = loaded
                .manager
                .data_file_property_value(place.index, value_name)?;
            return Ok(value.as_ref().map(value_metadata));
        }
        Ok(loaded
            .fiftyone_properties
            .iter()
            .find(|property| property.name() == property_name)
            .and_then(|property| {
                property
                    .values()
                    .iter()
                    .find(|value| value.name == value_name)
                    .cloned()
            }))
    }

    /// Run a detection for `data` into new device data.
    ///
    /// Builds a native evidence set from the flow data (preferring the decoded
    /// UACH element-data `sec-ch-ua*` values over the raw headers) and processes
    /// it. Each property, the match metrics among them, is read from the
    /// results the first time it is requested, as .NET's `DeviceDataHash`
    /// reads them. With no properties named, the C library walks a
    /// component's graph on the first read of one of its properties, so a
    /// caller reading only some properties walks only the graphs they need.
    fn detect(&self, data: &FlowData) -> Result<DeviceDataBase> {
        let reader = self.detection_reader(data)?;
        let mut device = DeviceDataBase::new();
        device
            .base_mut()
            .add_missing_property_source(Arc::clone(&reader.loaded.missing));
        Ok(device.with_reader(reader))
    }

    /// Process the evidence for `data` into new results, and give the reader
    /// that reads each property from them when it is first requested.
    fn detection_reader(&self, data: &FlowData) -> Result<DetectionReader> {
        // Snapshot the current data set. The snapshot is an `Arc`, so an
        // in-flight detection keeps the data set alive even if `refresh` swaps a
        // new one in mid-flight.
        let loaded = self.loaded.load_full();
        let mut results = loaded.manager.create_results()?;

        // Marshal the evidence the engine understands, preferring decoded UACH
        // values, into one merged evidence set, then process it.
        let evidence = self.collect_evidence(data);
        results.process_evidence(&evidence)?;

        Ok(DetectionReader {
            results: Mutex::new(results),
            loaded,
        })
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
    /// path. See [`Self::swap_in`].
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
                self.temp_data_copy,
                self.reuse_temp_file,
            )
        });
        let manager = open_manager(
            copy.as_ref().map_or(path, OperationalCopy::path),
            self.profile,
            &self.requested_properties,
            &self.options,
        )?;
        self.swap_in(copy, manager)
    }

    /// Reload the data set from `data`, the whole of a data file, and
    /// atomically swap it in. See [`Self::swap_in`].
    fn reload_from_memory(&self, data: &[u8]) -> Result<()> {
        let _refreshing = self
            .refresh_lock
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        // The data set reads the data where it lies, so the manager keeps a
        // copy for as long as the data set.
        let manager = Manager::open_from_memory(
            data.to_vec(),
            self.profile,
            requested(&self.requested_properties).as_deref(),
            &self.options,
        )?;
        self.swap_in(None, manager)
    }

    /// Replace the live data set with `manager`'s, which reads `copy` when
    /// there is one, together with the properties a detection reads, the
    /// evidence keys, the metadata and the dates in the data file header,
    /// because a new data file can carry different properties or a different
    /// tier. In-flight detections keep using the data set they snapshotted. A
    /// failure changes nothing. The caller holds the refresh lock.
    fn swap_in(&self, copy: Option<OperationalCopy>, manager: Arc<Manager>) -> Result<()> {
        let evidence_keys = named_evidence_keys(&manager);
        let (loaded, metadata) = describe(copy, manager, &self.tier_override)?;
        if let Some(file) = self.data_files.first() {
            record_dates(&loaded.manager, file);
        }
        self.evidence_key_filter.replace(evidence_keys);
        self.loaded.store(Arc::new(loaded));
        self.metadata.replace(metadata);
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

    /// The engine itself, so an element added after it, such as the
    /// robots.txt engine, can find it in the pipeline and read its data set.
    fn into_any(self: Arc<Self>) -> Option<Arc<dyn Any + Send + Sync>> {
        Some(self)
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

    /// The reason from the loaded data set's missing property source, the same
    /// one device data gives, which compares each property's tiers with the
    /// data file's own tier even when the builder set another to report.
    fn missing_property_reason(&self, property_name: &str) -> MissingPropertyResult {
        self.loaded.load().missing.reason(property_name)
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
        // logged at error level, as .NET does, and returned. The message keeps
        // .NET's "If issue persists" so support can search for one phrase
        // across languages.
        let path = self
            .data_files
            .first()
            .and_then(|file| file.data_file_path().map(Path::to_path_buf));
        match path {
            Some(path) => self.reload_from_path(&path).inspect_err(|error| {
                log::error!(
                    "Failed to refresh data from '{}'. The engine keeps the data it had. \
                     If issue persists, please contact support@51degrees.com. {error}",
                    path.display()
                );
            }),
            None => Err(Error::configuration(format!(
                "Engine '{}' has no data file path to refresh from.",
                self.data_key()
            ))),
        }
    }

    fn refresh_from_memory(&self, _data_file_identifier: Option<&str>, data: &[u8]) -> Result<()> {
        // A single-file engine ignores the identifier. A refresh that fails is
        // logged as a refresh from the file is.
        self.reload_from_memory(data).inspect_err(|error| {
            log::error!(
                "Failed to refresh data from memory. The engine keeps the data it had. \
                 If issue persists, please contact support@51degrees.com. {error}"
            );
        })
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
    options: &HashOptions,
) -> Result<Arc<Manager>> {
    Manager::open_with_hash_options(
        path,
        profile,
        requested(requested_properties).as_deref(),
        options,
    )
}

/// The property names to open a data set with, or [`None`] for every property
/// when `requested_properties` is empty.
fn requested(requested_properties: &[String]) -> Option<Vec<&str>> {
    if requested_properties.is_empty() {
        None
    } else {
        Some(requested_properties.iter().map(String::as_str).collect())
    }
}

/// Where a detection reads a property's value from.
enum ReadableProperty {
    /// A match metric, read from the results.
    Metric(Metric),
    /// A data file property.
    File {
        /// The property's name as the data file spells it.
        name: String,
        /// The type the property's rendered value is converted to.
        value_type: PropertyValueType,
        /// The index the results' value reader takes for the property.
        index: c_int,
    },
}

impl ReadableProperty {
    /// The property's name.
    fn name(&self) -> &str {
        match self {
            ReadableProperty::Metric(metric) => metric.name(),
            ReadableProperty::File { name, .. } => name.as_str(),
        }
    }
}

/// The properties a detection can read.
struct ReadableProperties {
    /// Each property a detection can read.
    properties: Vec<ReadableProperty>,
    /// Each property's name in lowercase, in the same order.
    lowercase_names: Vec<String>,
    /// The position of each property in `properties`, by lowercase name.
    by_name: HashMap<String, usize>,
}

impl ReadableProperties {
    /// Every data file property the data set was opened with and results
    /// hold a value for, except any with a match metric's name, then the
    /// match metrics, as the .NET engine lists them.
    fn new(file_properties: &[DataFileProperty]) -> Self {
        let metrics = Metric::READ_ORDER.into_iter().map(ReadableProperty::Metric);
        let files = file_properties
            .iter()
            .filter(|property| property.available)
            .filter(|property| {
                !METRIC_PROPERTIES
                    .iter()
                    .any(|metric| metric.name.eq_ignore_ascii_case(&property.name))
            })
            .filter_map(|property| {
                property
                    .required_property_index
                    .map(|index| ReadableProperty::File {
                        name: property.name.clone(),
                        value_type: property.value_type,
                        index,
                    })
            });
        let properties: Vec<ReadableProperty> = files.chain(metrics).collect();
        let lowercase_names: Vec<String> = properties
            .iter()
            .map(|property| property.name().to_lowercase())
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

    fn read_property(&self, position: usize) -> ReadOutcome {
        let results = self.results.lock().unwrap_or_else(PoisonError::into_inner);
        let (name, value_type, index) = match &self.loaded.readable.properties[position] {
            ReadableProperty::Metric(metric) => return metric.read(&results),
            ReadableProperty::File {
                name,
                value_type,
                index,
            } => (name, *value_type, *index),
        };
        let values = results
            .values_by_index(index)
            .map_err(|error| error.to_string())?;
        let values = if name.eq_ignore_ascii_case(JAVASCRIPT_HARDWARE_PROFILE) {
            values.map(|values| values.into_iter().map(wrap_hardware_profile).collect())
        } else {
            values
        };
        Ok(native_value(values, value_type))
    }
}

/// Build the engine's metadata from the data file's properties and components.
///
/// Every property the data file defines is listed, in the data file's order,
/// with the type, category, description, data tiers and the rest the data
/// file gives it, and one the data set was opened without is marked
/// unavailable, as the C++ `PropertyMetaDataBuilderHash` in 51degrees-cxx
/// does. The match metrics follow, then the components, each with its
/// properties, and the `Metrics` component last, as the .NET engine lists
/// them.
fn build_metadata(
    file_properties: &[DataFileProperty],
    file_components: &[DataFileComponent],
    tier: String,
) -> EngineMetadata {
    let mut fiftyone_properties: Vec<FiftyOneAspectPropertyMetaData> = file_properties
        .iter()
        .map(|property| file_property_metadata(property, file_components))
        .collect();
    for metric in METRIC_PROPERTIES {
        // A metric the data file already defines is not listed twice.
        if fiftyone_properties
            .iter()
            .any(|property| property.name().eq_ignore_ascii_case(metric.name))
        {
            continue;
        }
        fiftyone_properties.push(metric_property_metadata(metric));
    }

    let aspect_properties: Vec<AspectPropertyMetaData> = fiftyone_properties
        .iter()
        .map(|property| property.aspect().clone())
        .collect();
    let properties = aspect_properties
        .iter()
        .map(|property| property.core().clone())
        .collect();

    let component = |id: u8, name: &str| {
        ComponentMetaData::new(id, name).with_properties(
            fiftyone_properties
                .iter()
                .filter(|property| property.component_id() == Some(id))
                .cloned(),
        )
    };
    let mut components: Vec<ComponentMetaData> = file_components
        .iter()
        .map(|file_component| {
            component(file_component.component_id, &file_component.name)
                .with_default_profile_id(file_component.default_profile_id)
        })
        .collect();
    components.push(component(METRICS_COMPONENT_ID, METRICS_COMPONENT_NAME));

    EngineMetadata {
        lent: LentMetadata {
            properties,
            aspect_properties,
            tier,
        },
        fiftyone_properties,
        components,
    }
}

/// The metadata of a data file property.
fn file_property_metadata(
    property: &DataFileProperty,
    file_components: &[DataFileComponent],
) -> FiftyOneAspectPropertyMetaData {
    let aspect =
        AspectPropertyMetaData::new(&property.name, DEVICE_ELEMENT_DATA_KEY, property.value_type)
            .with_description(&property.description)
            .with_data_tiers(property.data_tiers.iter().cloned())
            .map_core(|core| {
                core.with_category(&property.category)
                    .with_available(property.available)
            });
    let mut metadata = FiftyOneAspectPropertyMetaData::from_aspect(aspect)
        .with_url(&property.url)
        .with_display_order(property.display_order)
        .with_mandatory(property.mandatory)
        .with_list(property.list)
        .with_obsolete(property.obsolete)
        .with_show(property.show)
        .with_show_values(property.show_values);
    if let Some(component) = file_components
        .iter()
        .find(|component| component.component_id == property.component_id)
    {
        metadata = metadata
            .with_component_id(component.component_id)
            .with_component_name(&component.name);
    }
    if let Some(default_value) = &property.default_value {
        metadata = metadata.with_default_value(value_metadata(default_value));
    }
    metadata
}

/// The metadata of a match metric, with the display order and flags the .NET
/// engine gives every metric.
fn metric_property_metadata(metric: &MetricProperty) -> FiftyOneAspectPropertyMetaData {
    let aspect =
        AspectPropertyMetaData::new(metric.name, DEVICE_ELEMENT_DATA_KEY, metric.value_type)
            .with_description(metric.description)
            .with_data_tiers(METRIC_DATA_TIERS)
            .map_core(|core| core.with_category(METRIC_CATEGORY));
    FiftyOneAspectPropertyMetaData::from_aspect(aspect)
        .with_component_id(METRICS_COMPONENT_ID)
        .with_component_name(METRICS_COMPONENT_NAME)
        .with_display_order(METRIC_DISPLAY_ORDER)
        .with_default_value(ValueMetaData::new(metric.default_value))
        .with_values(metric.values.iter().map(|value| ValueMetaData::new(*value)))
}

/// The metadata of a data file value.
fn value_metadata(value: &DataFileValue) -> ValueMetaData {
    ValueMetaData::new(&value.name)
        .with_description(&value.description)
        .with_url(&value.url)
}

/// The value of a property from the values the results hold for it, or
/// [`None`] when it has none, as the C++ `ResultsBase` in 51degrees-cxx gives
/// it to the .NET engine.
///
/// `values` is [`None`] when the results hold no value for the property, such
/// as with no evidence, and then no type has a value. Otherwise a list is
/// every value, an empty one included, and an empty list when the matched
/// profile holds none. Any other type has no value when the profile holds
/// none. A string or JavaScript property joins several values with
/// [`VALUE_SEPARATOR`]. A boolean, integer or double has no value when there
/// are several, and otherwise reads its one value as C does: a boolean is
/// true only for the exact text `True`, an integer is what `atoi` reads and a
/// double what `strtod` reads. So a value such as `Unknown` or `N/A` gives
/// false or 0.
fn native_value(
    values: Option<Vec<String>>,
    value_type: PropertyValueType,
) -> Option<PropertyValue> {
    let values = values?;
    if value_type == PropertyValueType::StringList {
        return Some(PropertyValue::StringList(values));
    }
    match (value_type, values.as_slice()) {
        (_, []) => None,
        (PropertyValueType::Bool, [value]) => Some(PropertyValue::Bool(value == "True")),
        (PropertyValueType::Integer, [value]) => Some(PropertyValue::Integer(c_atoi(value))),
        (PropertyValueType::Double, [value]) => Some(PropertyValue::Double(c_strtod(value))),
        (PropertyValueType::Bool | PropertyValueType::Integer | PropertyValueType::Double, _) => {
            None
        }
        _ => Some(PropertyValue::String(values.join(VALUE_SEPARATOR))),
    }
}

/// True for the characters C's `isspace` finds in the C locale, which
/// `atoi` and `strtod` skip at the start of their text.
fn is_c_space(character: char) -> bool {
    matches!(character, ' ' | '\t' | '\n' | '\x0b' | '\x0c' | '\r')
}

/// The text after any white space and sign at the start of `text`, and
/// whether the sign was a minus.
fn c_sign(text: &str) -> (bool, &str) {
    let text = text.trim_start_matches(is_c_space);
    match text.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, text.strip_prefix('+').unwrap_or(text)),
    }
}

/// The integer C's `atoi` reads from the start of `text`, being the decimal
/// digits after any white space and sign, or 0 when there are none. C leaves
/// a number too large for its type undefined, and here it stops at the
/// largest.
fn c_atoi(text: &str) -> i64 {
    let (negative, rest) = c_sign(text);
    let magnitude = rest
        .bytes()
        .take_while(u8::is_ascii_digit)
        .fold(0i64, |number, digit| {
            number
                .saturating_mul(10)
                .saturating_add(i64::from(digit - b'0'))
        });
    if negative {
        -magnitude
    } else {
        magnitude
    }
}

/// The number C's `strtod` reads from the start of `text`, or 0 when it does
/// not start with one. After any white space and sign that is `inf`,
/// `infinity` or `nan` in any case, or decimal digits with an optional point
/// and exponent. A hexadecimal number, which `strtod` also reads, gives 0.
fn c_strtod(text: &str) -> f64 {
    let (negative, rest) = c_sign(text);
    let lower = rest.get(..8).unwrap_or(rest).to_ascii_lowercase();
    let magnitude = if lower.starts_with("inf") {
        f64::INFINITY
    } else if lower.starts_with("nan") {
        f64::NAN
    } else {
        let bytes = rest.as_bytes();
        let digits = |from: usize| {
            bytes[from..]
                .iter()
                .take_while(|byte| byte.is_ascii_digit())
                .count()
        };
        let whole = digits(0);
        let mut end = whole;
        let mut fraction = 0;
        if bytes.get(end) == Some(&b'.') {
            fraction = digits(end + 1);
            end += 1 + fraction;
        }
        if whole + fraction == 0 {
            return 0.0;
        }
        if matches!(bytes.get(end), Some(b'e' | b'E')) {
            let sign = usize::from(matches!(bytes.get(end + 1), Some(b'+' | b'-')));
            let exponent = digits(end + 1 + sign);
            if exponent > 0 {
                end += 1 + sign + exponent;
            }
        }
        rest[..end].parse::<f64>().unwrap_or(0.0)
    };
    if negative {
        -magnitude
    } else {
        magnitude
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
/// carries it stops running. Each value is wrapped, an empty one included, as
/// the C++ wraps them.
fn wrap_hardware_profile(body: String) -> String {
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

/// Record on `data_file` the dates in `manager`'s data file header, as .NET
/// does after each load. The date the file was published goes in the update
/// service's `If-Modified-Since` header. The date the next is expected, at noon
/// UTC and a random minute as .NET gives it, is when the service starts to
/// poll. A date the header does not hold is cleared, so that no date of data
/// no longer loaded is kept.
fn record_dates(manager: &Manager, data_file: &AspectEngineDataFile) {
    let published = manager
        .data_set_published()
        .and_then(|(year, month, day)| Utc.with_ymd_and_hms(year, month, day, 0, 0, 0).single());
    match published {
        Some(published) => data_file.set_data_published(published),
        None => data_file.clear_data_published(),
    }
    let next = manager
        .data_set_next_update()
        .and_then(|(year, month, day)| {
            Utc.with_ymd_and_hms(year, month, day, NEXT_UPDATE_HOUR, random_minute(), 0)
                .single()
        });
    match next {
        Some(next) => data_file.set_update_available_time(next),
        None => data_file.clear_update_available_time(),
    }
}

/// The hour, UTC, of the day the data file header gives for the next data
/// file, at which the update service is told to expect it.
const NEXT_UPDATE_HOUR: u32 = 12;

/// The number of minutes in an hour, the range the minute of
/// [`NEXT_UPDATE_HOUR`] is chosen from.
const MINUTES_PER_HOUR: u64 = 60;

/// A minute of the hour chosen at random, so that servers loading the same
/// data file do not all start to poll at once.
fn random_minute() -> u32 {
    let random = RandomState::new().hash_one(std::time::SystemTime::now());
    u32::try_from(random % MINUTES_PER_HOUR).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_engine_type_name_is_the_name_of_the_type() {
        let full = std::any::type_name::<DeviceDetectionOnPremiseEngine>();
        assert!(full.ends_with(&format!("::{ENGINE_TYPE_NAME}")), "{full}");
    }

    /// The values given, as the results would hold them.
    fn values(values: &[&str]) -> Option<Vec<String>> {
        Some(values.iter().map(|value| (*value).to_owned()).collect())
    }

    const EVERY_TYPE: [PropertyValueType; 6] = [
        PropertyValueType::String,
        PropertyValueType::Bool,
        PropertyValueType::Integer,
        PropertyValueType::Double,
        PropertyValueType::StringList,
        PropertyValueType::JavaScript,
    ];

    #[test]
    fn a_property_the_results_hold_no_value_for_has_none() {
        for value_type in EVERY_TYPE {
            assert_eq!(native_value(None, value_type), None, "{value_type:?}");
        }
    }

    #[test]
    fn only_a_list_has_a_value_when_the_profile_holds_none() {
        for value_type in EVERY_TYPE {
            let expected = (value_type == PropertyValueType::StringList)
                .then_some(PropertyValue::StringList(Vec::new()));
            assert_eq!(
                native_value(values(&[]), value_type),
                expected,
                "{value_type:?}"
            );
        }
    }

    #[test]
    fn a_list_keeps_every_value() {
        assert_eq!(
            native_value(
                values(&["iPhone", "", "a|b"]),
                PropertyValueType::StringList
            ),
            Some(PropertyValue::StringList(vec![
                "iPhone".to_owned(),
                String::new(),
                "a|b".to_owned()
            ]))
        );
    }

    #[test]
    fn a_string_joins_its_values_and_keeps_text_as_it_is() {
        for value_type in [PropertyValueType::String, PropertyValueType::JavaScript] {
            assert_eq!(
                native_value(values(&["a", "b"]), value_type),
                Some(PropertyValue::String("a|b".to_owned()))
            );
            assert_eq!(
                native_value(values(&[""]), value_type),
                Some(PropertyValue::String(String::new()))
            );
            assert_eq!(
                native_value(values(&["Unknown"]), value_type),
                Some(PropertyValue::String("Unknown".to_owned()))
            );
        }
    }

    #[test]
    fn a_boolean_is_true_only_for_the_text_true() {
        let read = |value: &str| native_value(values(&[value]), PropertyValueType::Bool);
        assert_eq!(read("True"), Some(PropertyValue::Bool(true)));
        for value in ["False", "true", "TRUE", "1", "Unknown", "N/A", "", " True"] {
            assert_eq!(read(value), Some(PropertyValue::Bool(false)), "{value:?}");
        }
    }

    #[test]
    fn a_number_or_boolean_with_several_values_has_none() {
        for value_type in [
            PropertyValueType::Bool,
            PropertyValueType::Integer,
            PropertyValueType::Double,
        ] {
            assert_eq!(
                native_value(values(&["1", "1"]), value_type),
                None,
                "{value_type:?}"
            );
        }
    }

    #[test]
    fn an_integer_is_what_atoi_reads() {
        let read = |value: &str| native_value(values(&[value]), PropertyValueType::Integer);
        for (value, expected) in [
            ("1170", 1170),
            ("-7", -7),
            ("+5", 5),
            (" \t\x0b\x0c\r\n42", 42),
            ("12.9", 12),
            ("42abc", 42),
            ("Unknown", 0),
            ("N/A", 0),
            ("", 0),
            ("-", 0),
            ("- 3", 0),
            ("99999999999999999999", i64::MAX),
        ] {
            assert_eq!(
                read(value),
                Some(PropertyValue::Integer(expected)),
                "{value:?}"
            );
        }
    }

    #[test]
    fn a_double_is_what_strtod_reads() {
        let read = |value: &str| match native_value(values(&[value]), PropertyValueType::Double) {
            Some(PropertyValue::Double(number)) => number,
            other => panic!("{value:?} gave {other:?}"),
        };
        for (value, expected) in [
            ("71.5", 71.5),
            ("  3.25", 3.25),
            ("-2.5e-1", -0.25),
            ("+.5e+1", 5.0),
            ("5.", 5.0),
            (".5", 0.5),
            ("1e3x", 1000.0),
            ("1e", 1.0),
            ("1e+", 1.0),
            ("2.5.1", 2.5),
            ("Unknown", 0.0),
            ("N/A", 0.0),
            ("", 0.0),
            (".", 0.0),
            ("e5", 0.0),
            ("inf", f64::INFINITY),
            ("-Infinity", f64::NEG_INFINITY),
            ("1e999", f64::INFINITY),
        ] {
            assert_eq!(read(value).to_bits(), expected.to_bits(), "{value:?}");
        }
        assert!(read("NaN").is_nan());
        assert!(read("nan(1)").is_nan());
    }

    #[test]
    fn the_metrics_are_listed_as_the_dotnet_engine_lists_them() {
        let metadata = build_metadata(&[], &[], String::from("Lite"));
        let names: Vec<&str> = metadata
            .fiftyone_properties
            .iter()
            .map(|property| property.name())
            .collect();
        assert_eq!(
            names,
            [
                "MatchedNodes",
                "Difference",
                "Drift",
                "DeviceId",
                "UserAgents",
                "Iterations",
                "Method"
            ]
        );
        for property in &metadata.fiftyone_properties {
            assert_eq!(property.core().category, METRIC_CATEGORY);
            assert_eq!(property.component_id(), Some(METRICS_COMPONENT_ID));
            assert_eq!(property.display_order(), METRIC_DISPLAY_ORDER);
            assert!(property.core().available && property.show() && property.show_values());
            assert!(!property.mandatory() && !property.list() && !property.obsolete());
            assert_eq!(
                property.aspect().data_tiers_where_present(),
                METRIC_DATA_TIERS
            );
            assert!(property.default_value().is_some());
        }
        let method = &metadata.fiftyone_properties[6];
        let values: Vec<&str> = method.values().iter().map(|v| v.name.as_str()).collect();
        assert_eq!(values, ["NONE", "PERFORMANCE", "COMBINED", "PREDICTIVE"]);
        assert_eq!(
            method.default_value().map(|v| v.name.as_str()),
            Some("NONE")
        );
        assert_eq!(
            metadata.fiftyone_properties[1].aspect().description(),
            "Used when detection method is not Exact or None. This is an integer \
             value and the larger the value the less confident the detector is \
             in this result."
        );

        let components: Vec<(u8, &str, usize)> = metadata
            .components
            .iter()
            .map(|c| (c.component_id, c.name.as_str(), c.properties().len()))
            .collect();
        assert_eq!(components, [(METRICS_COMPONENT_ID, "Metrics", 7)]);
        assert_eq!(metadata.components[0].default_profile_id, None);
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
        // An empty value is wrapped too, as the C++ wraps it.
        assert_eq!(
            wrap_hardware_profile(String::new()),
            "var profileIds = []\n\n\
             document.cookie = \"51D_ProfileIds=\" + profileIds.join(\"|\")"
        );
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

    /// The Hash data file `51DEGREES_DD_PATH` names, or the Lite file in the
    /// `cxx` submodule.
    fn data_file() -> Option<PathBuf> {
        if let Ok(path) = std::env::var("51DEGREES_DD_PATH") {
            let path = PathBuf::from(path);
            if path.is_file() {
                return Some(path);
            }
        }
        let workspace = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()?
            .to_path_buf();
        [workspace.clone(), workspace.parent()?.to_path_buf()]
            .into_iter()
            .map(|root| root.join("cxx/data/device-detection/51Degrees-LiteV4.1.hash"))
            .find(|path| path.is_file())
    }

    /// With no concurrency named, the data set opens with the default, not
    /// only the engine's record of it, under the two profiles with no limit,
    /// and a refresh opens the new data set with it too. On a machine with 10
    /// processors the default equals the C library's own, so the check there
    /// cannot tell the two apart.
    #[test]
    fn the_default_concurrency_reaches_the_data_set() {
        let Some(path) = data_file() else {
            eprintln!("no Hash data file found; skipping the default concurrency");
            return;
        };
        for profile in [
            PerformanceProfile::LowMemory,
            PerformanceProfile::HighPerformance,
        ] {
            let engine = crate::DeviceDetectionOnPremiseEngineBuilder::new(&path)
                .performance_profile(profile)
                .build()
                .expect("the data file loads");
            let expected = fiftyone_native::dd::default_concurrency(profile);
            let pool = || engine.loaded.load().manager.file_pool_size();
            assert_eq!(pool(), expected, "{}", profile.as_str());
            engine.refresh(None).expect("the data file loads again");
            assert_eq!(pool(), expected, "{} refreshed", profile.as_str());
        }
    }

    /// Every match metric listed in the metadata is read from the results,
    /// and a detection's data lists them in the metadata's order.
    #[test]
    fn every_listed_metric_is_read_from_the_results() {
        let read: Vec<&str> = Metric::READ_ORDER.iter().map(|m| m.name()).collect();
        let listed: Vec<&str> = METRIC_PROPERTIES.iter().map(|m| m.name).collect();
        assert_eq!(read, listed);
        let readable = ReadableProperties::new(&[]);
        let lowercase: Vec<String> = listed.iter().map(|name| name.to_lowercase()).collect();
        assert_eq!(readable.lowercase_names, lowercase);
        for (position, name) in lowercase.iter().enumerate() {
            assert_eq!(readable.by_name.get(name), Some(&position), "{name}");
        }
    }

    const CHROME_ON_WINDOWS: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) \
        AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0.0.0 Safari/537.36";

    /// An engine over the data file with no properties named, and flow data
    /// holding a User-Agent for it.
    fn engine_and_data(path: &Path) -> (Arc<DeviceDetectionOnPremiseEngine>, FlowData) {
        let engine = crate::DeviceDetectionOnPremiseEngineBuilder::new(path)
            .performance_profile(PerformanceProfile::HighPerformance)
            .build()
            .expect("the data file loads");
        let element: Arc<dyn FlowElement> = engine.clone();
        let pipeline = fiftyone_pipeline_core::Pipeline::builder()
            .add_element(element)
            .build()
            .expect("the pipeline builds");
        let data = pipeline.create_flow_data_with(
            Evidence::builder()
                .add("header.user-agent", CHROME_ON_WINDOWS)
                .build(),
        );
        (engine, data)
    }

    /// A detection walks no graph, reading a property walks only the graph
    /// of its component, and reading a match metric walks the rest, as
    /// .NET's results read the metrics only when they are asked for.
    #[test]
    fn a_detection_walks_only_the_graphs_of_the_properties_read() {
        let Some(path) = data_file() else {
            eprintln!("no Hash data file found; skipping the graph walks");
            return;
        };
        let (engine, data) = engine_and_data(&path);
        for metric in Metric::READ_ORDER {
            let reader = engine.detection_reader(&data).expect("detection succeeds");
            let pending = || {
                reader
                    .results
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .pending_graphs()
            };
            let all = pending();
            assert!(all > 1, "a graph for each component: {all}");
            let position = |name: &str| reader.position(name).expect(name);
            let is_mobile = reader.read_property(position("ismobile"));
            assert!(
                matches!(is_mobile, Ok(Some(PropertyValue::Bool(_)))),
                "{is_mobile:?}"
            );
            assert_eq!(pending(), all - 1, "IsMobile walks one graph");
            let name = metric.name();
            let value = reader.read_property(position(name.to_lowercase().as_str()));
            assert!(matches!(value, Ok(Some(_))), "{name}: {value:?}");
            assert_eq!(pending(), 0, "{name} walks every graph");
        }
    }

    /// A detection stores no value, and its data lists the data file's
    /// properties first, then the match metrics, as the .NET engine does.
    #[test]
    fn a_detection_stores_nothing_until_a_property_is_read() {
        use fiftyone_pipeline_core::ElementData;
        let Some(path) = data_file() else {
            eprintln!("no Hash data file found; skipping the stored values");
            return;
        };
        let (engine, data) = engine_and_data(&path);
        let device = engine.detect(&data).expect("detection succeeds");
        assert!(
            device.base().keys().is_empty(),
            "{:?}",
            device.base().keys()
        );
        let keys = device.keys();
        let (files, metrics) = keys.split_at(keys.len() - Metric::READ_ORDER.len());
        assert_eq!(
            metrics,
            [
                "matchednodes",
                "difference",
                "drift",
                "deviceid",
                "useragents",
                "iterations",
                "method"
            ]
        );
        assert!(files.iter().any(|key| key == "ismobile"), "{keys:?}");
        let method = device.get("Method").expect("Method has a value");
        assert!(
            ["NONE", "PERFORMANCE", "COMBINED", "PREDICTIVE"].contains(&method.as_str().unwrap()),
            "{method:?}"
        );
    }
}
