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

//! The [`IpIntelligenceOnPremiseEngine`] flow element.
//!
//! The engine wraps a native IP Intelligence [`Manager`](fiftyone_native::ipi::Manager)
//! and turns each pipeline request into a native lookup, returning an
//! [`IpIntelligenceDataBase`] that reads each property from the lookup's native
//! results the first time it is requested. See the crate-level documentation
//! for the design rationale.

use std::collections::HashMap;
use std::ops::Range;
use std::os::raw::c_int;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};

use arc_swap::ArcSwap;

use fiftyone_ip_intelligence_shared::{
    declared_property_value_type, IpIntelligenceDataBase, IpIntelligencePropertyReader,
    IP_DATA_KEY, IP_DATA_KEY_NAME, TYPED_PROPERTY_NAMES, WEIGHTED_PROPERTY_VALUE_TYPE,
};
use fiftyone_native::ipi::{
    ip_address_text, ClientIpEvidence, DataFileComponent, DataFileProperty, DataFileValue, Manager,
    PropertyRead, Results,
};
use fiftyone_native::PerformanceProfile;
use fiftyone_pipeline_core::{
    constants, round_angle, Error, EvidenceKeyFilter, EvidenceKeyFilterWhitelist, FlowData,
    FlowElement, PropertyMetaData, PropertyValueType, Result, WeightedValue,
};
use fiftyone_pipeline_engines::{
    AspectEngine, AspectEngineBase, AspectEngineDataFile, AspectPropertyMetaData, EngineDeployment,
    MissingPropertySource, OnPremiseAspectEngine,
};
use fiftyone_pipeline_engines_fiftyone::{
    ComponentMetaData, FiftyOneAspectPropertyMetaData, ValueMetaData,
};

use crate::builder::IpIntelligenceOnPremiseEngineBuilder;
use crate::operational_copy::OperationalCopy;
use crate::retained::Retained;

/// The data-source tier the engine reports when the data file's header names
/// none and the builder sets none.
pub const DEFAULT_DATA_SOURCE_TIER: &str = "Lite";

/// The engine's type name, which the message for a property that needs a data
/// file upgrade names.
const ENGINE_TYPE_NAME: &str = "IpIntelligenceOnPremiseEngine";

/// A fixed list of client IP keys. The engine does not read it.
///
/// The engine accepts the keys its data file defines, which
/// [`IpIntelligenceOnPremiseEngine::evidence_keys`] lists. This list holds
/// two of those and names keys no data file defines.
#[deprecated(note = "The engine reads its evidence keys from its data file. Use \
            `IpIntelligenceOnPremiseEngine::evidence_keys`.")]
pub const IP_EVIDENCE_KEYS: &[&str] = &[
    constants::EVIDENCE_CLIENT_IP_KEY,
    "query.client-ip",
    "51d.client-ip",
    "query.51d.client-ip",
    "server.51d.client-ip",
];

/// The id .NET gives a component the data file does not define, being the
/// `Metrics` component and the component of the `Ip` and `IpV6` properties.
const ADDED_COMPONENT_ID: u8 = 255;

/// The name of the component .NET lists after the data file's, which holds no
/// property.
const METRICS_COMPONENT_NAME: &str = "Metrics";

/// The name of the component, and the category, of the `Ip` and `IpV6`
/// properties.
const NETWORK: &str = "Network";

/// The display order of the `Ip` and `IpV6` properties.
const ADDED_DISPLAY_ORDER: u8 = 255;

/// The default value, and only value, of the `Ip` and `IpV6` properties.
const ADDED_DEFAULT_VALUE: &str = "N/A";

/// The properties .NET lists after the data file's, which hold the address
/// looked up, with their descriptions.
const ADDED_PROPERTIES: [(&str, &str); 2] = [
    ("Ip", "The IPv4 address of the request as a string."),
    ("IpV6", "The IPv6 address of the request as a string."),
];

/// The place of `Ip`, which holds an IPv4 address, in [`ADDED_PROPERTIES`].
const ADDED_IPV4_PLACE: usize = 0;

/// The reason .NET gives each of [`ADDED_PROPERTIES`], in the same order,
/// when the evidence holds no address of its kind.
const ADDED_NOT_SUPPLIED: [&str; 2] = [
    "IPv4 was not supplied as evidence.",
    "IPv6 was not supplied as evidence.",
];

/// The reason .NET gives both [`ADDED_PROPERTIES`] when the evidence holds a
/// client IP value and none reads as an address.
const ADDED_NOT_VALID: &str = "The IP supplied as evidence was not valid.";

/// The property .NET gives a weighted type when the data file names it a
/// string, matched ignoring case.
const MCC_PROPERTY: &str = "Mcc";

/// An on-premise IP-intelligence aspect engine.
///
/// Reads IP-intelligence properties for the client IP carried in a request's
/// evidence from a local `.ipi` data file, populating an
/// [`IpIntelligenceDataBase`] under the shared [`IP_DATA_KEY`]. Implements
/// [`FlowElement`], [`AspectEngine`] and [`OnPremiseAspectEngine`].
///
/// Build one with [`IpIntelligenceOnPremiseEngine::builder`]. The engine is
/// shared (`Arc`) across threads, and the loaded native manager lives behind an
/// [`ArcSwap`] so [`OnPremiseAspectEngine::refresh`] can hot-swap a reloaded
/// data set without blocking concurrent requests.
pub struct IpIntelligenceOnPremiseEngine {
    /// The reusable engine base. No cache is attached, because each result
    /// keeps its native results, and the data set they came from, until it is
    /// dropped.
    base: AspectEngineBase<IpIntelligenceDataBase>,
    /// The loaded native data set, the properties a lookup reads from it and
    /// its metadata, swapped together atomically on refresh.
    loaded: ArcSwap<LoadedDataSet>,
    /// The performance profile used to (re)open the data file on refresh.
    profile: PerformanceProfile,
    /// The expected concurrency used to (re)open the data file on refresh, or
    /// `None` for the profile's own.
    concurrency: Option<u16>,
    /// The property names the engine was built with. Every refresh opens the
    /// data file with these names, so each keeps its index. Empty when the
    /// engine was built with no name, which opens the data file with every
    /// property.
    requested: Vec<String>,
    /// The names a read looks a property up in. Built once at build from
    /// `requested`, or rebuilt from each data set when the engine was built
    /// with no name, because then the indexes are the data set's own order.
    names: Arc<BuildNames>,
    /// The evidence keys of the loaded data file, replaced on refresh.
    evidence_key_filter: DataFileEvidenceKeys,
    /// The property metadata and tier of the loaded data set, for
    /// [`FlowElement::properties`], [`AspectEngine::aspect_properties`] and
    /// [`AspectEngine::data_source_tier`].
    metadata: Retained<LentMetadata>,
    /// The tier the builder set, which every data set loaded reports, or
    /// `None` to report the tier each data file names.
    tier_override: Option<String>,
    /// Held while a refresh loads a data set, so refreshes run one at a time.
    refresh_lock: Mutex<()>,
    /// The folder a refresh copies the data file into, or `None` for the
    /// system's temporary folder.
    temp_dir: Option<PathBuf>,
    /// Whether a refresh reads a copy of the data file under every profile,
    /// as the builder's `create_temp_data_copy` asked.
    temp_data_copy: bool,
    /// Whether a refresh reads a copy of the data file already in the
    /// temporary folder, as the builder's `reuse_temp_file` asked.
    reuse_temp_file: bool,
    /// The single data file this engine reads, as shared run-time state for the
    /// data update service.
    data_files: Vec<Arc<AspectEngineDataFile>>,
}

/// The part of a loaded data set's metadata the pipeline traits lend by
/// reference, which [`Retained`] keeps for the engine's life.
#[derive(PartialEq)]
struct LentMetadata {
    properties: Vec<PropertyMetaData>,
    aspect_properties: Vec<AspectPropertyMetaData>,
    tier: String,
    /// The names of the properties a lookup reads, in the data file's order.
    populated: Vec<String>,
}

/// The property and component metadata and tier of a loaded data set.
struct EngineMetadata {
    lent: LentMetadata,
    fiftyone_properties: Vec<FiftyOneAspectPropertyMetaData>,
    components: Vec<ComponentMetaData>,
}

/// Where a data file property is among the data file's properties, and how
/// many values the data file holds for it.
#[derive(Clone, Copy)]
struct FilePropertyPlace {
    /// The index [`Manager::data_file_property_value_range`] takes.
    index: u32,
    value_count: u32,
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
    names: &Arc<BuildNames>,
) -> Result<(LoadedDataSet, LentMetadata)> {
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
    let readable = Arc::new(ReadableNames::new(&file_properties, names));
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
            IP_DATA_KEY_NAME,
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

/// Record on `data_file` the dates in `manager`'s data file header, as .NET
/// does after each load, with
/// [`AspectEngineDataFile::record_header_dates`].
pub(crate) fn record_dates(manager: &Manager, data_file: &AspectEngineDataFile) {
    data_file.record_header_dates(manager.data_set_published(), manager.data_set_next_update());
}

impl IpIntelligenceOnPremiseEngine {
    /// Start building an engine for the `.ipi` data file at `data_file_path`.
    ///
    /// Convenience entry point to [`IpIntelligenceOnPremiseEngineBuilder::new`].
    pub fn builder(
        data_file_path: impl Into<std::path::PathBuf>,
    ) -> IpIntelligenceOnPremiseEngineBuilder {
        IpIntelligenceOnPremiseEngineBuilder::new(data_file_path)
    }

    /// Assemble an engine from its pre-built parts. Called by the builder once
    /// it has opened the data file. Fails when the data file's metadata cannot
    /// be read. `copy` comes before `manager` so it is dropped after it, as in
    /// `describe`.
    pub(crate) fn from_parts(
        copy: Option<OperationalCopy>,
        manager: Arc<Manager>,
        profile: PerformanceProfile,
        requested: Vec<String>,
        tier_override: Option<String>,
        data_file: Arc<AspectEngineDataFile>,
    ) -> Result<Self> {
        let evidence_key_filter = DataFileEvidenceKeys::new(&manager);
        // The names the manager was opened with, which every refresh opens it
        // with, so each name keeps its index.
        let names = Arc::new(BuildNames::new(manager.available_property_names()));
        let (loaded, metadata) = describe(copy, manager, &tier_override, &names)?;
        Ok(IpIntelligenceOnPremiseEngine {
            base: AspectEngineBase::new(),
            evidence_key_filter,
            loaded: ArcSwap::from_pointee(loaded),
            profile,
            concurrency: None,
            requested,
            names,
            metadata: Retained::new(metadata),
            tier_override,
            refresh_lock: Mutex::new(()),
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

    /// Open the data file on refresh with the expected concurrency the builder
    /// opened it with, or the profile's own when `None`.
    pub(crate) fn with_concurrency(mut self, concurrency: Option<u16>) -> Self {
        self.concurrency = concurrency;
        self
    }

    /// The evidence keys the loaded data file defines, in the order the native
    /// lookup tries them, lowercased.
    ///
    /// Each header the data file names is accepted as a query value and as a
    /// server value. Every query value is tried before any server value, and
    /// the first that reads as an IP address is looked up.
    pub fn evidence_keys(&self) -> Vec<String> {
        self.loaded
            .load()
            .manager
            .evidence_keys()
            .map(str::to_owned)
            .collect()
    }

    /// The names of the data file properties a lookup reads, being every
    /// property the loaded data file defines that the engine was opened with,
    /// in the data file's order and spelling. A name the engine was
    /// restricted to that the data file lacks is not among them.
    pub fn populated_property_names(&self) -> &[String] {
        &self.metadata.current().populated
    }

    /// The performance profile the engine opens its data file with.
    pub fn performance_profile(&self) -> PerformanceProfile {
        self.profile
    }

    /// The expected concurrency the engine opens its data file with, or
    /// `None` for the profile's own.
    pub fn concurrency(&self) -> Option<u16> {
        self.concurrency
    }

    /// The names of the properties the data set was opened with, in the order
    /// it holds them. With properties named, these are the names given, in
    /// the order given, including any the data file has no property for, so
    /// [`Self::populated_property_names`] gives the data file properties a
    /// lookup reads. Without, this is every property the data file defines.
    /// Intended for introspection, such as the metadata example.
    pub fn available_property_names(&self) -> Vec<String> {
        self.loaded.load().manager.available_property_names()
    }

    /// The 51Degrees metadata of each property in
    /// [`AspectEngine::aspect_properties`], in the same order, with the URL,
    /// display order, flags, component and default value the data file gives
    /// it.
    ///
    /// A data file property's values are not copied into its metadata,
    /// because a data file can hold millions of them, so
    /// [`Self::property_values`] reads them when they are asked for. The `Ip`
    /// and `IpV6` properties, which .NET adds after the data file's, list
    /// their one value.
    ///
    /// The list is the loaded data set's. A refresh loads a new list and frees
    /// this one once no caller holds it.
    pub fn fiftyone_properties(&self) -> Arc<[FiftyOneAspectPropertyMetaData]> {
        Arc::clone(&self.loaded.load().fiftyone_properties)
    }

    /// The components the data file defines, such as the location and the
    /// network, each with every property the data file gives it, then the
    /// `Metrics` component, which holds no property, as .NET lists them.
    ///
    /// The list is the loaded data set's. A refresh loads a new list and frees
    /// this one once no caller holds it.
    pub fn components(&self) -> Arc<[ComponentMetaData]> {
        Arc::clone(&self.loaded.load().components)
    }

    /// The values the property named `property_name`, matched exactly, can
    /// have, or [`None`] when the engine lists no such property. Fails when
    /// the data file's values cannot be read.
    ///
    /// A data file property's values are read from the loaded data file on
    /// each call, in the data file's order, each written as text as .NET
    /// gives it, so a coordinate has three decimal places and an IP address is
    /// written in full. A lookup gives an IPv6 range compressed, as
    /// [`fiftyone_native::ipi::ip_address_text`] writes it, so its text can
    /// differ from the value listed here. The `Ip` and `IpV6` properties give
    /// the one value their metadata lists. A data file can hold millions of
    /// values for one property, so [`Self::property_value_range`] reads part
    /// of the list.
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

    /// The value `value_name` names among those [`Self::property_values`]
    /// gives for the property named `property_name`, matched as .NET's
    /// `GetValue(valueName)` matches them, or [`None`] when the engine lists
    /// no such property or the property has no such value. Fails when the
    /// data file's values cannot be read.
    ///
    /// This finds the value in the loaded data file without reading the
    /// property's other values, by the C library's binary search over values
    /// in order, as .NET finds it. The property's name is matched exactly,
    /// as is a value of `Ip` or `IpV6`. The search compares a number written
    /// to as many places as the text of `value_name` before any null has
    /// characters plus three, at most 15, so the value given can be written
    /// otherwise, as `0.063` is given for `0.0625`. A data file property's
    /// value is searched with the text of `value_name` before any null, as
    /// .NET searches it, so `AS13335\0x` finds `AS13335`.
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

    /// Look up the client IP in `data`'s evidence, through
    /// [`Results::process_client_ip_evidence`](fiftyone_native::ipi::Results::process_client_ip_evidence),
    /// into an [`IpIntelligenceDataBase`] that reads each property from the
    /// native results the first time it is requested.
    ///
    /// Evidence with no client IP address that reads as one gives data in
    /// which no property has a value, as .NET's engine gives. The native
    /// results stay with the returned data until it is dropped.
    fn lookup(&self, data: &FlowData) -> Result<IpIntelligenceDataBase> {
        // Load the current data set without blocking a concurrent refresh.
        let loaded = self.loaded.load_full();
        let mut results = loaded.manager.create_results()?;
        let client_ip = results.process_client_ip_evidence(data.evidence())?;
        let mut ip = IpIntelligenceDataBase::new(IP_DATA_KEY_NAME);
        ip.base_mut()
            .add_missing_property_source(Arc::clone(&loaded.missing));
        Ok(ip.with_reader(LookupReader {
            results: Mutex::new(results),
            client_ip,
            loaded,
        }))
    }

    /// The names the engine was built with, as the native open takes them,
    /// or [`None`] when there are none.
    fn property_refs(&self) -> Option<Vec<&str>> {
        if self.requested.is_empty() {
            None
        } else {
            Some(self.requested.iter().map(String::as_str).collect())
        }
    }

    /// The names a read of `manager`'s data set looks a property up in.
    ///
    /// An engine built with names keeps the table it was built with, because
    /// a name's index is its place in those names whatever the new data file
    /// holds. An engine built with no name takes the new data set's names,
    /// because its indexes are that data set's own order, which a new data
    /// file can change. The C library is changing to keep the first load's
    /// names, and the table here becomes the built one for both.
    fn names_for(&self, manager: &Arc<Manager>) -> Arc<BuildNames> {
        if self.requested.is_empty() {
            Arc::new(BuildNames::new(manager.available_property_names()))
        } else {
            Arc::clone(&self.names)
        }
    }

    /// Reopen the data file at `path` and atomically swap it in. See
    /// [`Self::swap_in`].
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
        let manager = Manager::open_with_options(
            copy.as_ref().map_or(path, OperationalCopy::path),
            self.profile,
            self.property_refs().as_deref(),
            self.concurrency,
        )?;
        self.swap_in(copy, manager)
    }

    /// Load the data set from `data`, the whole of a data file, and
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
            self.property_refs().as_deref(),
        )?;
        self.swap_in(None, manager)
    }

    /// Replace the live data set with `manager`'s, which reads `copy` when
    /// there is one. A concurrent `process` call keeps using the old data set
    /// until the swap completes. The evidence keys, the metadata, the tier
    /// and the dates in the data file header follow the new data, which can
    /// differ from the old. A failure, including metadata that cannot be
    /// read, changes nothing. The caller holds the refresh lock.
    fn swap_in(&self, copy: Option<OperationalCopy>, manager: Arc<Manager>) -> Result<()> {
        let evidence_keys = DataFileEvidenceKeys::whitelist(&manager);
        let names = self.names_for(&manager);
        let (loaded, metadata) = describe(copy, manager, &self.tier_override, &names)?;
        let loaded = Arc::new(loaded);
        self.loaded.store(Arc::clone(&loaded));
        self.evidence_key_filter.keys.store(Arc::new(evidence_keys));
        self.metadata.replace(metadata);
        // Recorded only once the new data set is in use, so the update
        // service never skips a file the engine failed to load.
        if let Some(data_file) = self.data_files.first() {
            record_dates(&loaded.manager, data_file);
        }
        Ok(())
    }
}

/// The evidence keys of the loaded data file, as the engine's evidence key
/// filter. A refresh replaces them, because a new data file can define
/// different keys. Each key's order is its place in the native lookup's order,
/// so a lower order is tried first.
struct DataFileEvidenceKeys {
    keys: ArcSwap<EvidenceKeyFilterWhitelist>,
}

impl DataFileEvidenceKeys {
    fn new(manager: &Manager) -> Self {
        DataFileEvidenceKeys {
            keys: ArcSwap::from_pointee(Self::whitelist(manager)),
        }
    }

    fn whitelist(manager: &Manager) -> EvidenceKeyFilterWhitelist {
        EvidenceKeyFilterWhitelist::with_orders(manager.evidence_keys().zip(0..))
    }
}

impl EvidenceKeyFilter for DataFileEvidenceKeys {
    fn include(&self, key: &str) -> bool {
        self.keys.load().include(key)
    }

    fn order(&self, key: &str) -> Option<i32> {
        self.keys.load().order(key)
    }

    fn named_keys(&self) -> Option<Vec<String>> {
        self.keys.load().named_keys()
    }
}

/// Build the engine's metadata from the data file's properties and components.
///
/// Every property the data file defines is listed, in the data file's order,
/// with the type, category, description, data tiers and the rest the data
/// file gives it, and one the data set was opened without is marked
/// unavailable, as the C++ `PropertyMetaDataBuilderIpi` in 51degrees-cxx
/// does. The `Ip` and `IpV6` properties follow, in a `Network` component of
/// their own, then the components, each with its properties, and the empty
/// `Metrics` component last, as the .NET engine lists them.
fn build_metadata(
    file_properties: &[DataFileProperty],
    file_components: &[DataFileComponent],
    tier: String,
) -> EngineMetadata {
    let mut fiftyone_properties: Vec<FiftyOneAspectPropertyMetaData> = file_properties
        .iter()
        .map(|property| file_property_metadata(property, file_components))
        .collect();
    fiftyone_properties.extend(
        ADDED_PROPERTIES
            .iter()
            .map(|(name, description)| added_property_metadata(name, description)),
    );

    let aspect_properties: Vec<AspectPropertyMetaData> = fiftyone_properties
        .iter()
        .map(|property| property.aspect().clone())
        .collect();
    let properties = aspect_properties
        .iter()
        .map(|property| property.core().clone())
        .collect();
    let populated = file_properties
        .iter()
        .filter(|property| property.available)
        .map(|property| property.name.clone())
        .collect();

    // A component holds the properties with its id and name, because .NET
    // gives the Ip and IpV6 properties a Network component with the Metrics
    // component's id, which neither the data file's Network component nor the
    // Metrics component holds.
    let component = |id: u8, name: &str| {
        ComponentMetaData::new(id, name).with_properties(
            fiftyone_properties
                .iter()
                .filter(|property| {
                    property.component_id() == Some(id) && property.component_name() == name
                })
                .cloned(),
        )
    };
    let mut components: Vec<ComponentMetaData> = file_components
        .iter()
        .map(|file_component| {
            let metadata = component(file_component.component_id, &file_component.name);
            match file_component.default_profile_offset {
                Some(offset) => metadata.with_default_profile_id(offset),
                None => metadata,
            }
        })
        .collect();
    components.push(component(ADDED_COMPONENT_ID, METRICS_COMPONENT_NAME));

    EngineMetadata {
        lent: LentMetadata {
            properties,
            aspect_properties,
            tier,
            populated,
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
    let aspect = AspectPropertyMetaData::new(
        &property.name,
        IP_DATA_KEY_NAME,
        metadata_value_type(&property.name, property.value_type),
    )
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

/// The type of the values of the data file property `name`, being the
/// `value_type` the data file names, except that `Mcc` named as a string is
/// weighted, as .NET's `PropertyMetaDataIpi` types it.
fn metadata_value_type(name: &str, value_type: PropertyValueType) -> PropertyValueType {
    if value_type == PropertyValueType::String && name.eq_ignore_ascii_case(MCC_PROPERTY) {
        WEIGHTED_PROPERTY_VALUE_TYPE
    } else {
        value_type
    }
}

/// The metadata of the `Ip` or `IpV6` property, with the category, component,
/// display order, flags and value the .NET engine gives both.
fn added_property_metadata(name: &str, description: &str) -> FiftyOneAspectPropertyMetaData {
    let aspect = AspectPropertyMetaData::new(name, IP_DATA_KEY_NAME, PropertyValueType::String)
        .with_description(description)
        .map_core(|core| core.with_category(NETWORK));
    FiftyOneAspectPropertyMetaData::from_aspect(aspect)
        .with_component_id(ADDED_COMPONENT_ID)
        .with_component_name(NETWORK)
        .with_display_order(ADDED_DISPLAY_ORDER)
        .with_default_value(ValueMetaData::new(ADDED_DEFAULT_VALUE))
        .with_values([ValueMetaData::new(ADDED_DEFAULT_VALUE)])
}

/// The metadata of a data file value.
fn value_metadata(value: &DataFileValue) -> ValueMetaData {
    ValueMetaData::new(&value.name)
        .with_description(&value.description)
        .with_url(&value.url)
}

/// A loaded data set, with the properties a lookup reads from it, its
/// metadata and the copy of the data file it was read from, if any.
struct LoadedDataSet {
    manager: Arc<Manager>,
    readable: Arc<ReadableNames>,
    /// Each data file property's place among the data file's properties and
    /// its number of values, by name.
    file_properties: HashMap<String, FilePropertyPlace>,
    /// Why a property is missing, from the metadata of this data set, added to
    /// every result a lookup writes.
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

/// The properties a lookup reads, being every data file property the data
/// set was opened with, each with the index the results getters take for it.
/// The C library finds a property ignoring case and gives the data file's
/// spelling, so a property named in another case is read under that spelling.
///
/// A lookup also reads [`ADDED_PROPERTIES`], after the data file's, which
/// hold the address looked up rather than anything in the data file.
struct ReadableNames {
    /// Each property's name in the data file's spelling, in the order of
    /// the names the engine was built with.
    names: Vec<String>,
    /// Each property's name in lowercase, in the same order, then those of
    /// [`ADDED_PROPERTIES`].
    lowercase_names: Vec<String>,
    /// Each property's required property index in the data set.
    indexes: Vec<c_int>,
    /// Each property's type, as the engine's metadata gives it from the data
    /// file.
    value_types: Vec<PropertyValueType>,
    /// The names the engine was built with.
    build: Arc<BuildNames>,
    /// The place in `names` of the property at each index, or `None` when
    /// this data set does not have it.
    places: Vec<Option<usize>>,
    /// The place of the property each typed accessor reads, by the
    /// accessor's slot in [`TYPED_PROPERTY_NAMES`], resolved once when the
    /// data set loads.
    typed: Vec<Option<usize>>,
}

/// The property names the engine was built with, each with its index, which
/// is its place in the list, and [`ADDED_PROPERTIES`]. Built once when the
/// engine is built and never from a data set, so a name keeps its index
/// across every refresh.
struct BuildNames {
    /// The names, in the order the engine was built with them.
    list: Vec<String>,
    /// Where each lowercase name is read.
    by_name: HashMap<String, NameSlot>,
}

/// Where a property named in a read is read.
#[derive(Clone, Copy)]
enum NameSlot {
    /// The property at this index among the names the engine was built with.
    Index(usize),
    /// The property at this place in [`ADDED_PROPERTIES`].
    Added(usize),
}

impl BuildNames {
    /// The table for `list`. A name given twice keeps its first index, as
    /// the C library gives it, and the name of an added property reads it.
    fn new(list: Vec<String>) -> Self {
        let mut by_name = HashMap::new();
        for (index, name) in list.iter().enumerate() {
            by_name
                .entry(name.to_lowercase())
                .or_insert(NameSlot::Index(index));
        }
        for (offset, (name, _)) in ADDED_PROPERTIES.iter().enumerate() {
            by_name.insert(name.to_lowercase(), NameSlot::Added(offset));
        }
        BuildNames { list, by_name }
    }
}

impl ReadableNames {
    /// Every property the engine was built with a name for that the data
    /// set has, in the order of the names. The C library gives each
    /// property the index of its name.
    fn new(file_properties: &[DataFileProperty], build: &Arc<BuildNames>) -> Self {
        let count = build.list.len();
        let mut at_index: Vec<Option<&DataFileProperty>> = vec![None; count];
        for property in file_properties.iter().filter(|property| property.available) {
            let Some(index) = property
                .required_property_index
                .and_then(|index| usize::try_from(index).ok())
            else {
                continue;
            };
            if let Some(slot) = at_index.get_mut(index).filter(|slot| slot.is_none()) {
                *slot = Some(property);
            }
        }
        let mut names = Vec::new();
        let mut indexes = Vec::new();
        let mut value_types = Vec::new();
        let mut places = vec![None; count];
        for (index, property) in at_index.into_iter().enumerate() {
            let Some(property) = property else {
                continue;
            };
            places[index] = Some(names.len());
            names.push(property.name.clone());
            indexes.push(index as c_int);
            value_types.push(metadata_value_type(&property.name, property.value_type));
        }
        let lowercase_names: Vec<String> = names
            .iter()
            .map(String::as_str)
            .chain(ADDED_PROPERTIES.iter().map(|(name, _)| *name))
            .map(str::to_lowercase)
            .collect();
        let mut readable = ReadableNames {
            names,
            lowercase_names,
            indexes,
            value_types,
            build: Arc::clone(build),
            places,
            typed: Vec::new(),
        };
        readable.typed = TYPED_PROPERTY_NAMES
            .iter()
            .map(|name| readable.position(&name.to_lowercase()))
            .collect();
        readable
    }

    /// The place of the property named, found in the table of the names the
    /// engine was built with, or `None` when this data set does not have it.
    fn position(&self, name: &str) -> Option<usize> {
        match *self.build.by_name.get(name)? {
            NameSlot::Index(index) => self.places.get(index).copied().flatten(),
            NameSlot::Added(offset) => Some(self.names.len() + offset),
        }
    }
}

/// Reads the properties of one lookup from its native results when they are
/// requested.
struct LookupReader {
    /// The native results. Reading a value writes into them, so each read
    /// holds the lock. Declared first, so they are freed before the data set
    /// they came from.
    results: Mutex<Results>,
    /// The client IP evidence the lookup read, for [`ADDED_PROPERTIES`].
    client_ip: ClientIpEvidence,
    /// The data set the results came from, with its properties.
    loaded: Arc<LoadedDataSet>,
}

impl IpIntelligencePropertyReader for LookupReader {
    fn property_names(&self) -> &[String] {
        &self.loaded.readable.lowercase_names
    }

    fn position(&self, name: &str) -> Option<usize> {
        self.loaded.readable.position(name)
    }

    fn typed_positions(&self) -> Option<&[Option<usize>]> {
        Some(&self.loaded.readable.typed)
    }

    fn read_property(
        &self,
        position: usize,
        data: &mut IpIntelligenceDataBase,
    ) -> std::result::Result<(), String> {
        let readable = &self.loaded.readable;
        let Some(&index) = readable.indexes.get(position) else {
            populate_added_property(data, position - readable.indexes.len(), self.client_ip);
            return Ok(());
        };
        let name = &readable.names[position];
        let value_type = readable.value_types[position];
        let results = self.results.lock().unwrap_or_else(PoisonError::into_inner);
        populate_property(data, &results, name, index, value_type)
            .map_err(|error| error.to_string())
    }
}

/// Store the property at `added` in [`ADDED_PROPERTIES`], `Ip` or `IpV6`, as
/// the address looked up when it is of that property's kind, as .NET does,
/// written as [`ip_address_text`] writes it.
/// An IPv4-mapped IPv6 address is the IPv4 address it holds, as
/// [`ClientIpEvidence::address`] gives it. Otherwise the property has no
/// value, with .NET's reason, and is listed among the keys, so the JSON
/// builder writes it as null with the reason, as the JSON builder
/// specification asks of every property.
fn populate_added_property(
    data: &mut IpIntelligenceDataBase,
    added: usize,
    client_ip: ClientIpEvidence,
) {
    let (name, _) = ADDED_PROPERTIES[added];
    let ipv4 = added == ADDED_IPV4_PLACE;
    match client_ip.address {
        Some(address) if address.is_ipv4() == ipv4 => {
            data.set_string(name, ip_address_text(address))
        }
        Some(_) => data.set_string_no_value(name, ADDED_NOT_SUPPLIED[added]),
        None if client_ip.supplied => data.set_string_no_value(name, ADDED_NOT_VALID),
        None => data.set_string_no_value(name, ADDED_NOT_SUPPLIED[added]),
    }
}

/// Read one property from the results and store it on `ip_data`, typed by the
/// property's declared value type, or by `value_type`, the type the data file
/// names for it, when the shared model declares none.
///
/// The native [`Results::read_property`] decides plain versus weighted from the
/// value type the data file stores, so a weighted property is stored as a
/// weighted string list. For a plain property the *declared* metadata type is
/// the source of truth, because the boolean properties are stored as their
/// rendered string (`StoredValueType` is `string`) yet are declared `bool`: a
/// declared-boolean property is converted from its rendered value and stored
/// through [`IpIntelligenceDataBase::set_boolean`] so its strongly-typed
/// accessor reads back a [`bool`].
///
/// A property the C library says has no value, as .NET asks it before every
/// read, has the reason the C library gives, as in .NET. Asking first matters
/// for a lookup that found nothing, where reading a mandatory property's values
/// would give its default.
fn populate_property(
    ip_data: &mut IpIntelligenceDataBase,
    results: &Results,
    name: &str,
    index: c_int,
    value_type: PropertyValueType,
) -> Result<()> {
    let declared = declared_property_value_type(name).unwrap_or(value_type);
    if !results.has_values_by_index(index)? {
        let message = results.no_value_message_by_index(index)?;
        set_no_value(ip_data, name, declared, message);
        return Ok(());
    }

    // A property declared weighted (the country-code distributions, `Mcc` and
    // `StateGeoNameIdsGeographical`) is always read through the weighted
    // collection, even when the data file stores its candidates as plain
    // strings (their `StoredValueType` is `string`, so the native value type
    // alone would misclassify them as plain). The declared metadata type is
    // the source of truth for weighted-ness.
    if declared == WEIGHTED_PROPERTY_VALUE_TYPE {
        let pairs = results.values_weighted_by_index(index)?;
        if pairs.is_empty() {
            let message = results.no_value_message_by_index(index)?;
            set_no_value(ip_data, name, declared, message);
        } else {
            let values = pairs
                .into_iter()
                .map(|(value, weighting)| WeightedValue::new(weighting, value))
                .collect();
            ip_data.set_weighted_string(name, values);
        }
        return Ok(());
    }

    match results.read_property_by_index(index)? {
        PropertyRead::Absent => {
            let message = results.no_value_message_by_index(index)?;
            set_no_value(ip_data, name, declared, message);
        }
        PropertyRead::Weighted(pairs) => {
            let values = pairs
                .into_iter()
                .map(|(value, weighting)| WeightedValue::new(weighting, value))
                .collect();
            ip_data.set_weighted_string(name, values);
        }
        PropertyRead::Boolean(value) => ip_data.set_boolean(name, value),
        PropertyRead::Integer(value) => {
            if declared == PropertyValueType::Bool {
                ip_data.set_boolean(name, value != 0);
            } else {
                ip_data.set_integer(name, value);
            }
        }
        PropertyRead::Float(value) => ip_data.set_float(name, value),
        // A typed property whose rendered value does not parse (the `Unknown` /
        // `N/A` no-value sentinels, say) is left unset, so its strongly-typed
        // accessor reports a clean no-value rather than carrying a string the
        // typed accessor could not return. A string property keeps its value
        // verbatim, including such a sentinel, matching the other SDKs.
        PropertyRead::String(value) => match declared {
            PropertyValueType::Bool => {
                if let Some(flag) = parse_bool(&value) {
                    ip_data.set_boolean(name, flag);
                }
            }
            PropertyValueType::Integer => {
                if let Ok(int) = value.trim().parse::<i32>() {
                    ip_data.set_integer(name, int);
                }
            }
            PropertyValueType::Float => {
                if let Some(float) = float_from_text(&value) {
                    ip_data.set_float(name, float);
                }
            }
            PropertyValueType::WktString => ip_data.set_wkt_string(name, value),
            _ => ip_data.set_string(name, value),
        },
    }
    Ok(())
}

/// Record that a property has no value, with `message`, in the store of its
/// `declared` type, so its typed accessor gives the reason, and list it among
/// the keys, so the JSON builder writes it as null with the reason, as .NET's
/// does.
fn set_no_value(
    ip_data: &mut IpIntelligenceDataBase,
    name: &str,
    declared: PropertyValueType,
    message: String,
) {
    if declared == WEIGHTED_PROPERTY_VALUE_TYPE {
        ip_data.set_weighted_string_no_value(name, message);
        return;
    }
    match declared {
        PropertyValueType::Bool => ip_data.set_boolean_no_value(name, message),
        PropertyValueType::Integer => ip_data.set_integer_no_value(name, message),
        PropertyValueType::Float => ip_data.set_float_no_value(name, message),
        PropertyValueType::WktString => ip_data.set_wkt_string_no_value(name, message),
        _ => ip_data.set_string_no_value(name, message),
    }
}

/// Parse a float property the data file gives as text, or [`None`] when the
/// text is not a number. The value is given to 3 decimal places by
/// [`round_angle`], because the IP Intelligence floats are the latitude and
/// the longitude, as the cloud engine gives them.
fn float_from_text(text: &str) -> Option<f32> {
    text.trim().parse::<f64>().ok().map(round_angle)
}

/// Parse a rendered boolean value (`true`/`false`, `1`/`0`, `yes`/`no`, matched
/// case-insensitively) into a [`bool`], or [`None`] for any other text.
fn parse_bool(text: &str) -> Option<bool> {
    match text.trim().to_ascii_lowercase().as_str() {
        "true" | "1" | "yes" => Some(true),
        "false" | "0" | "no" => Some(false),
        _ => None,
    }
}

impl FlowElement for IpIntelligenceOnPremiseEngine {
    fn process(&self, data: &mut FlowData) -> Result<()> {
        // No cache is attached, so `process_with_cache` simply runs the closure
        // once. Using the base keeps the process flow identical to the cloud
        // engine and the rest of the engine layer.
        self.base
            .process_with_cache(data, &self.evidence_key_filter, IP_DATA_KEY, |data| {
                self.lookup(data)
            })
    }

    fn data_key(&self) -> &str {
        IP_DATA_KEY_NAME
    }

    fn evidence_key_filter(&self) -> &dyn EvidenceKeyFilter {
        &self.evidence_key_filter
    }

    fn properties(&self) -> &[PropertyMetaData] {
        &self.metadata.current().properties
    }
}

impl AspectEngine for IpIntelligenceOnPremiseEngine {
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

    /// True for every property `IpIntelligenceData` has a typed accessor for.
    fn has_typed_property(&self, property_name: &str) -> bool {
        is_typed_property(property_name)
    }
}

/// Whether `IpIntelligenceData` has a typed accessor for the property, matched
/// ignoring case.
fn is_typed_property(property_name: &str) -> bool {
    declared_property_value_type(property_name).is_some()
}

impl OnPremiseAspectEngine for IpIntelligenceOnPremiseEngine {
    fn data_files(&self) -> &[Arc<AspectEngineDataFile>] {
        &self.data_files
    }

    fn refresh(&self, _data_file_identifier: Option<&str>) -> Result<()> {
        // Single-file engine: the identifier is ignored. Reopen the file the
        // engine was configured with and swap the new data set in atomically.
        let path = self
            .data_files
            .first()
            .and_then(|file| file.data_file_path().map(Path::to_path_buf))
            .ok_or_else(|| {
                Error::configuration(
                    "Cannot refresh an IP-intelligence engine that has no data file path.",
                )
            })?;
        self.reload_from_path(&path)
    }

    fn refresh_from_memory(&self, _data_file_identifier: Option<&str>, data: &[u8]) -> Result<()> {
        // Single-file engine: the identifier is ignored. Load the data given
        // and swap the new data set in atomically.
        self.reload_from_memory(data)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    use fiftyone_pipeline_core::{ElementData, Evidence, Pipeline};
    use fiftyone_pipeline_engines::AspectData;

    #[test]
    fn the_engine_type_name_is_the_name_of_the_type() {
        let full = std::any::type_name::<IpIntelligenceOnPremiseEngine>();
        assert!(full.ends_with(&format!("::{ENGINE_TYPE_NAME}")), "{full}");
    }

    /// The autonomous system properties an ASN data file carries. The
    /// real-lookup tests request these so the engine populates them for the ASN
    /// data file used below.
    const ASN_PROPERTIES: &[&str] = &["Asn", "AsnName"];

    /// Resolve an IP-intelligence data file at run time, searching the
    /// environment override, the `cxx` submodule and the
    /// wider Workspace tree. Returns [`None`] when none is present so the tests
    /// can skip cleanly only on a machine without the data file.
    ///
    /// This resolves the ASN file (`51Degrees-IPIV4AsnIpiV41.ipi`) from the data
    /// repository, which loads against this source revision and lets the
    /// real-lookup tests run an actual lookup. Set `51DEGREES_IPI_PATH` to
    /// override the path.
    fn ipi_data_file() -> Option<PathBuf> {
        if let Ok(path) = std::env::var("51DEGREES_IPI_PATH") {
            let path = PathBuf::from(path);
            if path.is_file() {
                return Some(path);
            }
        }
        let workspace = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()?
            .to_path_buf();
        let name = "51Degrees-IPIV4AsnIpiV41.ipi";
        // The data repository is in the `cxx` submodule, or checked out in the
        // wider Workspace tree beside the Rust workspace.
        let candidates = [
            workspace
                .join("cxx")
                .join("data")
                .join("ip-intelligence")
                .join(name),
            workspace
                .parent()
                .map(|p| p.join("data").join("ip-intelligence").join(name))
                .unwrap_or_default(),
        ];
        candidates.into_iter().find(|p| p.is_file())
    }

    /// Build an engine over the ASN data file requesting the autonomous system
    /// properties. The file presence is environmental, so the caller skips only
    /// when it is absent. Once found the engine must build, so a build failure
    /// is a hard panic rather than a soft-skip.
    fn asn_engine() -> Option<IpIntelligenceOnPremiseEngine> {
        let data_file = ipi_data_file()?;
        let engine = IpIntelligenceOnPremiseEngine::builder(&data_file)
            .performance_profile(PerformanceProfile::HighPerformance)
            .properties(ASN_PROPERTIES.iter().copied())
            // Off so the build needs no licence key.
            .auto_update(false)
            .build()
            .unwrap_or_else(|err| {
                panic!("the ASN IP Intelligence data file should build an engine: {err}")
            });
        Some(engine)
    }

    #[test]
    fn data_key_name() {
        assert_eq!(IP_DATA_KEY_NAME, "ip");
    }

    /// .NET types `Mcc` as weighted when the data file names it a string, in
    /// any case, and takes every other type as the data file names it.
    #[test]
    fn mcc_named_as_a_string_is_weighted() {
        use PropertyValueType::{Integer, KeyValueList, String};
        for name in ["Mcc", "MCC", "mcc"] {
            assert_eq!(
                metadata_value_type(name, String),
                WEIGHTED_PROPERTY_VALUE_TYPE
            );
            assert_eq!(metadata_value_type(name, Integer), Integer);
        }
        assert_eq!(metadata_value_type("Mcc", KeyValueList), KeyValueList);
        assert_eq!(metadata_value_type("Asn", String), String);
    }

    /// Asserts the reader of the loaded data set covers exactly the populated
    /// properties, each with its index in the data set.
    fn assert_reader_covers_carried_properties(engine: &IpIntelligenceOnPremiseEngine) {
        let loaded = engine.loaded.load();
        let readable = &loaded.readable;
        let mut names = readable.names.clone();
        let mut populated = engine.populated_property_names().to_vec();
        names.sort();
        populated.sort();
        assert_eq!(names, populated);
        for (name, index) in readable.names.iter().zip(&readable.indexes) {
            assert_eq!(
                loaded.manager.required_property_index(name),
                Some(*index),
                "{name}"
            );
        }
    }

    /// With no properties requested the engine populates every property the
    /// data file defines, in the data file's order.
    #[test]
    fn reader_covers_only_the_populated_properties_the_file_carries() {
        let Some(data_file) = ipi_data_file() else {
            eprintln!("no usable IP-intelligence data file; skipping readable properties");
            return;
        };
        let engine = IpIntelligenceOnPremiseEngine::builder(&data_file)
            .performance_profile(PerformanceProfile::HighPerformance)
            .auto_update(false)
            .build()
            .expect("the IP Intelligence data file should build an engine");
        assert_reader_covers_carried_properties(&engine);
        let file_names: Vec<String> = engine
            .loaded
            .load()
            .manager
            .data_file_properties()
            .expect("the data file's properties should be read")
            .into_iter()
            .map(|property| property.name)
            .collect();
        assert_eq!(engine.populated_property_names(), file_names);
    }

    /// A name given twice keeps the index of its first place, as the C
    /// library gives it.
    #[test]
    fn a_name_given_twice_keeps_its_first_index() {
        let names = BuildNames::new(vec![
            "Asn".to_owned(),
            "AsnName".to_owned(),
            "ASN".to_owned(),
        ]);
        assert!(matches!(names.by_name.get("asn"), Some(NameSlot::Index(0))));
    }

    /// The reader lists the data file properties in the order the engine was
    /// built with their names, then `Ip` and `IpV6`, because each name's
    /// index is its place among those names.
    #[test]
    fn the_reader_lists_the_names_in_the_order_the_engine_was_built_with() {
        let Some(data_file) = ipi_data_file() else {
            eprintln!("no usable IP-intelligence data file; skipping the order of the names");
            return;
        };
        let names = ["AsnName", "Asn"];
        let engine = IpIntelligenceOnPremiseEngine::builder(&data_file)
            .performance_profile(PerformanceProfile::HighPerformance)
            .properties(names.iter().copied())
            .auto_update(false)
            .build()
            .expect("the IP Intelligence data file should build an engine");
        let expected: Vec<String> = names
            .iter()
            .copied()
            .chain(ADDED_PROPERTIES.iter().map(|(name, _)| *name))
            .map(str::to_lowercase)
            .collect();
        assert_eq!(engine.loaded.load().readable.lowercase_names, expected);
    }

    /// An engine built with no names reads each property at the index of its
    /// name among the data set's names at build, before and after a refresh.
    #[test]
    fn an_engine_with_no_names_reads_by_the_index_of_its_names_at_build() {
        let Some(data_file) = ipi_data_file() else {
            eprintln!("no usable IP-intelligence data file; skipping the indexes of no names");
            return;
        };
        let engine = IpIntelligenceOnPremiseEngine::builder(&data_file)
            .performance_profile(PerformanceProfile::HighPerformance)
            .auto_update(false)
            .build()
            .expect("the IP Intelligence data file should build an engine");
        let names = engine.loaded.load().manager.available_property_names();
        assert!(names.len() > 1, "{names:?}");
        for refreshed in [false, true] {
            if refreshed {
                engine.refresh(None).expect("the data file loads again");
            }
            let loaded = engine.loaded.load();
            let readable = &loaded.readable;
            assert!(!readable.names.is_empty(), "refreshed {refreshed}");
            for (name, index) in readable.names.iter().zip(&readable.indexes) {
                let at = names
                    .iter()
                    .position(|given| given.eq_ignore_ascii_case(name))
                    .expect(name);
                assert_eq!(*index, at as c_int, "{name}, refreshed {refreshed}");
            }
        }
    }

    /// A reader over the current data set for a lookup of `ip`, built as
    /// [`IpIntelligenceOnPremiseEngine::lookup`] builds it.
    fn lookup_reader(engine: &IpIntelligenceOnPremiseEngine, ip: &str) -> LookupReader {
        let loaded = engine.loaded.load_full();
        let mut results = loaded
            .manager
            .create_results()
            .expect("results are created");
        let evidence = Evidence::builder().add("query.client-ip", ip).build();
        let client_ip = results
            .process_client_ip_evidence(&evidence)
            .expect("the address is read");
        LookupReader {
            results: Mutex::new(results),
            client_ip,
            loaded,
        }
    }

    /// The reader resolves each typed accessor's position once, being the
    /// position a read by that property's name finds, before and after a
    /// refresh, and a typed read gives what a read by name gives.
    #[test]
    fn each_typed_slot_reads_where_its_name_does() {
        use fiftyone_ip_intelligence_shared::IpIntelligenceData;
        let Some(data_file) = ipi_data_file() else {
            eprintln!("no usable IP-intelligence data file; skipping the typed slots");
            return;
        };
        // Built with the names in an order that is not the data file's, so
        // the positions follow the names rather than the data set.
        let engine = IpIntelligenceOnPremiseEngine::builder(&data_file)
            .performance_profile(PerformanceProfile::HighPerformance)
            .properties(["AsnName", "Asn"].iter().copied())
            .auto_update(false)
            .build()
            .expect("the IP Intelligence data file should build an engine");
        for refreshed in [false, true] {
            if refreshed {
                engine.refresh(None).expect("the data file loads again");
            }
            let reader = lookup_reader(&engine, "1.1.1.1");
            let typed = reader
                .typed_positions()
                .expect("the reader resolved the typed slots");
            assert_eq!(typed.len(), TYPED_PROPERTY_NAMES.len());
            let mut resolved = 0;
            for (slot, name) in TYPED_PROPERTY_NAMES.iter().enumerate() {
                let by_name = reader.position(&name.to_lowercase());
                assert_eq!(typed[slot], by_name, "{name}, refreshed {refreshed}");
                resolved += usize::from(by_name.is_some());
            }
            // The two names the engine was built with, and the two the
            // reader adds, all read somewhere.
            for name in ["Asn", "AsnName", "Ip", "IpV6"] {
                let slot = TYPED_PROPERTY_NAMES
                    .iter()
                    .position(|typed| *typed == name)
                    .expect(name);
                assert!(typed[slot].is_some(), "{name}, refreshed {refreshed}");
            }
            assert_eq!(resolved, 4, "refreshed {refreshed}");
        }

        let pipeline = Pipeline::builder()
            .add_element(Arc::new(engine))
            .build()
            .expect("pipeline builds");
        let mut data = pipeline.create_flow_data_with(
            Evidence::builder()
                .add("query.client-ip", "1.1.1.1")
                .build(),
        );
        data.process().expect("processing succeeds");
        let ip = data.get(IP_DATA_KEY).expect("ip data is present");
        for (typed, by_name) in [
            (format!("{:?}", ip.asn()), format!("{:?}", ip.string("Asn"))),
            (
                format!("{:?}", ip.asn_name()),
                format!("{:?}", ip.string("AsnName")),
            ),
            (format!("{:?}", ip.ip()), format!("{:?}", ip.string("Ip"))),
            (
                format!("{:?}", ip.country()),
                format!("{:?}", ip.string("Country")),
            ),
        ] {
            assert_eq!(typed, by_name);
        }
        let asn = ip.asn();
        let asn = asn.value().expect("Asn has a value");
        assert!(asn.contains("AS13335"), "{asn}");
    }

    /// The handles to the data file the engine's current data set holds.
    fn file_pool_size(engine: &IpIntelligenceOnPremiseEngine) -> u16 {
        engine.loaded.load().manager.file_pool_size()
    }

    /// The expected concurrency reaches the native data set when the engine
    /// is built and when it is refreshed, sizing its file handle pool, and
    /// more than the square root of the Balanced profiles cache of 20,000
    /// entries, 141, opens.
    #[test]
    fn the_expected_concurrency_opens_the_data_set_and_its_refresh() {
        let Some(data_file) = ipi_data_file() else {
            eprintln!("no usable IP-intelligence data file; skipping concurrency");
            return;
        };
        let builder = || {
            IpIntelligenceOnPremiseEngine::builder(&data_file)
                .performance_profile(PerformanceProfile::Balanced)
                // Off so the build needs no licence key.
                .auto_update(false)
        };

        let many = builder()
            .concurrency(500)
            .build()
            .expect("more than the profiles cache's square root opens");
        assert_eq!(file_pool_size(&many), 64);
        drop(many);

        let engine = builder()
            .concurrency(3)
            .build()
            .expect("the data set opens");
        assert_eq!(file_pool_size(&engine), 3);
        let engine = engine.with_concurrency(Some(5));
        engine.refresh(None).expect("the refresh opens");
        assert_eq!(file_pool_size(&engine), 5);
    }

    /// With properties requested each lookup reads only those.
    #[test]
    fn reader_covers_only_the_requested_properties() {
        let Some(engine) = asn_engine() else {
            eprintln!("no usable IP-intelligence data file; skipping requested properties");
            return;
        };
        assert_reader_covers_carried_properties(&engine);
        assert_eq!(engine.loaded.load().readable.names, ASN_PROPERTIES);
    }

    /// The engine accepts the keys its data file defines, ignoring case, in
    /// the order the native lookup tries them.
    #[test]
    fn evidence_keys_come_from_the_data_file() {
        let Some(engine) = asn_engine() else {
            eprintln!("no usable IP-intelligence data file; skipping evidence keys");
            return;
        };
        let keys = engine.evidence_keys();
        assert!(
            keys.contains(&String::from("query.client-ip-51d")),
            "{keys:?}"
        );
        let filter = engine.evidence_key_filter();
        for (order, key) in keys.iter().enumerate() {
            assert!(filter.include(key), "{key} should be accepted");
            assert_eq!(filter.order(key), Some(order as i32), "{key}");
        }
        assert!(filter.include("Query.Client-IP-51d"));
        assert!(!filter.include("header.user-agent"));
        // No data file defines this key.
        assert!(!filter.include("query.51d.client-ip"));
    }

    /// A client IP under a `51d` key alone is looked up.
    #[test]
    fn a_client_ip_under_a_51d_key_alone_is_looked_up() {
        let Some(engine) = asn_engine() else {
            eprintln!("no usable IP-intelligence data file; skipping 51d key");
            return;
        };
        let pipeline = Pipeline::builder()
            .add_element(Arc::new(engine))
            .build()
            .expect("pipeline should build");
        let mut data = pipeline.create_flow_data_with(
            Evidence::builder()
                .add("query.client-ip-51d", "1.1.1.1")
                .build(),
        );
        data.process().expect("processing should not error");
        let ip = data.get(IP_DATA_KEY).expect("ip data should be present");
        let asn = ip.string("Asn");
        let value = asn.value().expect("Asn should resolve to a value");
        assert!(value.contains("AS13335"), "got {value}");
    }

    /// A latitude or longitude the data file gives as text is given to 3
    /// decimal places, rounded before it is narrowed.
    #[test]
    fn a_float_given_as_text_is_given_to_three_decimal_places() {
        assert_eq!(float_from_text("18.542741172521133"), Some(18.543));
        assert_eq!(float_from_text(" -0.0625 "), Some(-0.063));
        // The f32 nearest this value would round to 21.212.
        assert_eq!(float_from_text("21.21250038148137"), Some(21.213));
        assert_eq!(float_from_text("51.5"), Some(51.5));
        assert_eq!(float_from_text("Unknown"), None);
    }

    /// A restricted engine populates the requested properties the data file
    /// defines, in the data file's spelling, and still lists every property,
    /// marking the others unavailable.
    #[test]
    fn requested_properties_restrict_populated_set() {
        let Some(data_file) = ipi_data_file() else {
            eprintln!("no usable IP-intelligence data file; skipping restricted set");
            return;
        };
        let engine = IpIntelligenceOnPremiseEngine::builder(&data_file)
            .properties(["asnname", "Country"])
            .auto_update(false)
            .build()
            .expect("the IP Intelligence data file should build an engine");
        assert_eq!(engine.populated_property_names(), ["AsnName"]);
        let listed: Vec<(&str, bool)> = engine
            .aspect_properties()
            .iter()
            .map(|property| (property.name(), property.available()))
            .collect();
        assert_eq!(
            listed,
            [
                ("Asn", false),
                ("AsnName", true),
                ("Ip", true),
                ("IpV6", true)
            ]
        );
    }

    /// Build the engine and run a public IPv4 lookup through a pipeline,
    /// asserting the autonomous system value reads back. `Asn` is a plain string
    /// property, so it is read through the plain string accessor.
    #[test]
    fn ipv4_lookup_yields_asn_value() {
        let Some(engine) = asn_engine() else {
            eprintln!("no usable IP-intelligence data file; skipping IPv4 lookup");
            return;
        };

        let pipeline = Pipeline::builder()
            .add_element(Arc::new(engine))
            .build()
            .expect("pipeline should build");
        let mut data = pipeline.create_flow_data_with(
            Evidence::builder()
                // A Cloudflare public IPv4, mapped to autonomous system AS13335.
                .add("server.client-ip", "1.1.1.1")
                .build(),
        );
        data.process().expect("processing should not error");

        let ip = data.get(IP_DATA_KEY).expect("ip data should be present");
        assert_eq!(ip.engine_keys(), ["ip"]);

        // The ASN data file maps the IP to its autonomous system number, a plain
        // string value.
        let asn = ip.string("Asn");
        let value = asn.value().expect("Asn should resolve to a value");
        eprintln!("Asn = {value}");
        assert!(
            value.contains("AS13335"),
            "the Cloudflare IPv4 should resolve to AS13335, got {value}"
        );
    }

    /// Build the engine and run a public IPv6 lookup, asserting the autonomous
    /// system value reads back.
    #[test]
    fn ipv6_lookup_yields_asn_value() {
        let Some(engine) = asn_engine() else {
            eprintln!("no usable IP-intelligence data file; skipping IPv6 lookup");
            return;
        };

        let pipeline = Pipeline::builder()
            .add_element(Arc::new(engine))
            .build()
            .expect("pipeline should build");
        let mut data = pipeline.create_flow_data_with(
            Evidence::builder()
                // A Cloudflare public IPv6, mapped to autonomous system AS13335.
                .add("server.client-ip", "2606:4700:4700::1111")
                .build(),
        );
        data.process().expect("processing should not error");

        let ip = data.get(IP_DATA_KEY).expect("ip data should be present");
        let asn = ip.string("Asn");
        let value = asn.value().expect("Asn should resolve to a value");
        eprintln!("IPv6 Asn = {value}");
        assert!(
            !value.is_empty(),
            "the public IPv6 should resolve to a non-empty autonomous system value"
        );
    }

    /// The reason the C library, and so .NET, gives every data file property
    /// of a lookup that found nothing.
    const NO_RESULTS_MESSAGE: &str =
        "The results are empty. This is probably because we don't have this data in our database.";

    /// The IP data `engine` gives for `evidence` in a pipeline of its own,
    /// which must record no error.
    fn ip_data_for(
        engine: &Arc<IpIntelligenceOnPremiseEngine>,
        evidence: &[(&str, &str)],
    ) -> IpIntelligenceDataBase {
        let element: Arc<dyn FlowElement> = Arc::clone(engine) as _;
        let pipeline = Pipeline::builder()
            .add_element(element)
            .build()
            .expect("pipeline should build");
        let mut builder = Evidence::builder();
        for (key, value) in evidence {
            builder = builder.add(*key, *value);
        }
        let mut data = pipeline.create_flow_data_with(builder.build());
        data.process().expect("processing should not error");
        assert!(data.errors().is_empty(), "{evidence:?}");
        data.get(IP_DATA_KEY)
            .expect("ip data should be present")
            .clone()
    }

    /// Evidence with no client IP address gives data in which no property has
    /// a value, as .NET gives. Each data file property has the C library's
    /// reason, and `Ip` and `IpV6` have .NET's reason, which says whether a
    /// value was supplied. Every one is listed, so the JSON builder writes it
    /// as null with the reason.
    #[test]
    fn evidence_with_no_client_ip_gives_data_with_no_values() {
        let Some(engine) = asn_engine() else {
            eprintln!("no usable IP-intelligence data file; skipping no-IP test");
            return;
        };
        let engine = Arc::new(engine);
        for (evidence, added_reasons) in [
            (&[("header.user-agent", "x")][..], ADDED_NOT_SUPPLIED),
            (&[("server.client-ip", "")][..], ADDED_NOT_SUPPLIED),
            (&[("server.client-ip", " ")][..], ADDED_NOT_SUPPLIED),
            (
                &[("server.client-ip", "not-an-ip")][..],
                [ADDED_NOT_VALID; 2],
            ),
            (
                &[("server.client-ip", "1.1.1.1, 10.0.0.1")][..],
                [ADDED_NOT_VALID; 2],
            ),
            (
                &[("server.client-ip", " 1.1.1.1")][..],
                [ADDED_NOT_VALID; 2],
            ),
            (
                &[("server.client-ip", "fe80::1%1")][..],
                [ADDED_NOT_VALID; 2],
            ),
        ] {
            let ip = ip_data_for(&engine, evidence);
            for name in ASN_PROPERTIES {
                assert_eq!(
                    ip.string(name).no_value_message(),
                    Some(NO_RESULTS_MESSAGE),
                    "{name} for {evidence:?}"
                );
                assert_eq!(
                    ElementData::get(&ip, name).unwrap_err().message,
                    NO_RESULTS_MESSAGE,
                    "{name} for {evidence:?}"
                );
            }
            for ((name, _), reason) in ADDED_PROPERTIES.iter().zip(added_reasons) {
                assert_eq!(
                    ip.string(name).no_value_message(),
                    Some(reason),
                    "{name} for {evidence:?}"
                );
                assert_eq!(
                    ElementData::get(&ip, name).unwrap_err().message,
                    reason,
                    "{name} for {evidence:?}"
                );
            }
            assert_eq!(
                ElementData::keys(&ip),
                ["asn", "asnname", "ip", "ipv6"],
                "{evidence:?}"
            );
        }
    }

    /// `Ip` holds the IPv4 address looked up and `IpV6` the IPv6 address, an
    /// IPv4-mapped address being IPv4, each written as .NET writes it. The
    /// other has .NET's reason that no address of its kind was supplied, even
    /// when a value before the one looked up did not read as an address, or
    /// one after it was an address of the other kind. Both are listed after
    /// the data file's properties, with a value or without.
    #[test]
    fn ip_and_ipv6_hold_the_address_looked_up() {
        let Some(engine) = asn_engine() else {
            eprintln!("no usable IP-intelligence data file; skipping Ip and IpV6");
            return;
        };
        let engine = Arc::new(engine);
        for (evidence, ipv4, ipv6) in [
            (
                &[("server.client-ip", "1.1.1.1")][..],
                Some("1.1.1.1"),
                None,
            ),
            (
                &[("server.client-ip", "::ffff:1.1.1.1")][..],
                Some("1.1.1.1"),
                None,
            ),
            (
                &[("server.client-ip", "[2606:4700:4700::1111]:443")][..],
                None,
                Some("2606:4700:4700::1111"),
            ),
            (
                &[(
                    "server.client-ip",
                    "2606:4700:4700:0000:0000:0000:0000:1111",
                )][..],
                None,
                Some("2606:4700:4700::1111"),
            ),
            (
                &[("server.client-ip", "::808:808")][..],
                None,
                Some("::8.8.8.8"),
            ),
            (
                &[("server.client-ip", "::ffff:0:8.8.8.8")][..],
                None,
                Some("::ffff:0:8.8.8.8"),
            ),
            (
                &[("server.client-ip", "fe80::5efe:808:808")][..],
                None,
                Some("fe80::5efe:8.8.8.8"),
            ),
            (
                &[
                    ("query.client-ip-51d", "not-an-ip"),
                    ("server.client-ip", "1.1.1.1"),
                ][..],
                Some("1.1.1.1"),
                None,
            ),
            (
                &[
                    ("query.client-ip-51d", "2606:4700:4700::1111"),
                    ("server.client-ip", "1.1.1.1"),
                ][..],
                None,
                Some("2606:4700:4700::1111"),
            ),
            (
                &[("server.client-ip", "1.1.1.1:443")][..],
                Some("1.1.1.1"),
                None,
            ),
        ] {
            let ip = ip_data_for(&engine, evidence);
            let listed: Vec<String> = ASN_PROPERTIES
                .iter()
                .copied()
                .chain(ADDED_PROPERTIES.iter().map(|(name, _)| *name))
                .map(str::to_lowercase)
                .collect();
            for (((name, _), reason), address) in ADDED_PROPERTIES
                .iter()
                .zip(ADDED_NOT_SUPPLIED)
                .zip([ipv4, ipv6])
            {
                let value = ip.string(name);
                match address {
                    Some(address) => assert_eq!(
                        value.value().ok().map(String::as_str),
                        Some(address),
                        "{name} for {evidence:?}"
                    ),
                    None => assert_eq!(
                        value.no_value_message(),
                        Some(reason),
                        "{name} for {evidence:?}"
                    ),
                }
            }
            assert_eq!(ElementData::keys(&ip), listed, "{evidence:?}");
        }
    }

    /// Refreshing reopens the data file and keeps the engine usable.
    #[test]
    fn refresh_reloads_and_keeps_working() {
        let Some(engine) = asn_engine() else {
            eprintln!("no usable IP-intelligence data file; skipping refresh test");
            return;
        };
        engine
            .refresh(None)
            .expect("refresh should reload the file");
        assert!(
            engine.evidence_key_filter().include("query.client-ip-51d"),
            "the reloaded file's keys should be accepted"
        );

        // The engine still resolves after the swap.
        let pipeline = Pipeline::builder()
            .add_element(Arc::new(engine))
            .build()
            .expect("pipeline should build");
        let mut data = pipeline.create_flow_data_with(
            Evidence::builder()
                .add("server.client-ip", "1.1.1.1")
                .build(),
        );
        data.process()
            .expect("processing after refresh should not error");
        assert!(data.get(IP_DATA_KEY).is_some());
    }
}
