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
use std::os::raw::c_int;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, PoisonError};

use arc_swap::ArcSwap;
use chrono::{TimeZone, Utc};

use fiftyone_ip_intelligence_shared::{
    declared_property_value_type, generated_property_names, IpIntelligenceDataBase,
    IpIntelligencePropertyReader, IP_DATA_KEY, IP_DATA_KEY_NAME, WEIGHTED_PROPERTY_VALUE_TYPE,
};
use fiftyone_native::ipi::{Manager, PropertyRead, Results};
use fiftyone_native::PerformanceProfile;
use fiftyone_pipeline_core::{
    constants, Error, EvidenceKeyFilter, EvidenceKeyFilterWhitelist, FlowData, FlowElement,
    PropertyMetaData, PropertyValueType, Result, WeightedValue,
};
use fiftyone_pipeline_engines::{
    AspectEngine, AspectEngineBase, AspectEngineDataFile, AspectPropertyMetaData, EngineDeployment,
    MissingPropertySource, OnPremiseAspectEngine,
};

use crate::builder::IpIntelligenceOnPremiseEngineBuilder;
use crate::operational_copy::OperationalCopy;
use crate::retained::Retained;

/// The default data-source tier reported by the engine.
///
/// The native IP Intelligence wrapper does not expose the data set's tier
/// string, so the engine reports a fixed tier. `Lite` is the freely
/// distributed tier the Lite `.ipi` file ships as, matching the file the
/// workspace bundles for tests.
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
    /// The loaded native data set and the populated properties it carries,
    /// swapped together atomically on refresh.
    loaded: ArcSwap<LoadedDataSet>,
    /// The performance profile used to (re)open the data file on refresh.
    profile: PerformanceProfile,
    /// The expected concurrency used to (re)open the data file on refresh, or
    /// `None` for the profile's own.
    concurrency: Option<u16>,
    /// The properties the engine was restricted to, lowercased. Empty means all
    /// typed properties are populated.
    requested_properties: Vec<String>,
    /// The names of the properties actually populated, in declaration order.
    populated_property_names: Vec<String>,
    /// The evidence keys of the loaded data file, replaced on refresh.
    evidence_key_filter: DataFileEvidenceKeys,
    /// Core property metadata for the populated properties.
    properties: Vec<PropertyMetaData>,
    /// The tier of the loaded data set and the aspect property metadata, which
    /// names that tier, for [`AspectEngine::data_source_tier`] and
    /// [`AspectEngine::aspect_properties`].
    metadata: Retained<TierMetadata>,
    /// The tier the builder set, which every data set loaded reports, or
    /// `None` to report the tier each data file names.
    tier_override: Option<String>,
    /// Held while a refresh loads a data set, so refreshes run one at a time.
    refresh_lock: Mutex<()>,
    /// The folder a refresh copies the data file into, or `None` for the
    /// system's temporary folder.
    temp_dir: Option<PathBuf>,
    /// The single data file this engine reads, as shared run-time state for the
    /// data update service.
    data_files: Vec<Arc<AspectEngineDataFile>>,
}

/// The tier of a loaded data set and the aspect property metadata that names
/// it.
#[derive(PartialEq)]
struct TierMetadata {
    tier: String,
    aspect_properties: Vec<AspectPropertyMetaData>,
}

impl TierMetadata {
    /// The metadata of the data set `manager` holds, reporting `tier_override`
    /// when it is set, otherwise the tier the data file names, then
    /// [`DEFAULT_DATA_SOURCE_TIER`] when it names none.
    fn new(manager: &Manager, tier_override: &Option<String>, populated: &[String]) -> Self {
        let tier = tier_override
            .clone()
            .or_else(|| manager.data_set_name())
            .unwrap_or_else(|| DEFAULT_DATA_SOURCE_TIER.to_owned());
        TierMetadata {
            aspect_properties: build_aspect_metadata(populated, &tier),
            tier,
        }
    }
}

/// Record the publish date in `manager`'s data file header on `data_file`,
/// for the metadata and the update service's `If-Modified-Since` header.
pub(crate) fn record_published(manager: &Manager, data_file: &AspectEngineDataFile) {
    if let Some((year, month, day)) = manager.data_set_published() {
        if let Some(published) = Utc.with_ymd_and_hms(year, month, day, 0, 0, 0).single() {
            data_file.set_data_published(published);
        }
    }
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
    /// it has opened the data file and derived the metadata.
    pub(crate) fn from_parts(
        manager: Arc<Manager>,
        copy: Option<OperationalCopy>,
        profile: PerformanceProfile,
        requested_properties: Vec<String>,
        tier_override: Option<String>,
        data_file: Arc<AspectEngineDataFile>,
    ) -> Self {
        let populated_property_names = populated_property_names(&requested_properties);
        let properties = build_core_metadata(&populated_property_names);
        let metadata = TierMetadata::new(&manager, &tier_override, &populated_property_names);

        IpIntelligenceOnPremiseEngine {
            base: AspectEngineBase::new(),
            evidence_key_filter: DataFileEvidenceKeys::new(&manager),
            loaded: ArcSwap::from_pointee(LoadedDataSet::new(
                manager,
                copy,
                &populated_property_names,
                &metadata,
            )),
            profile,
            concurrency: None,
            requested_properties,
            populated_property_names,
            properties,
            metadata: Retained::new(metadata),
            tier_override,
            refresh_lock: Mutex::new(()),
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

    /// The names of the properties this engine populates, in declaration order.
    ///
    /// The full typed set from the shared model, or the subset that intersects
    /// the requested properties when the engine was restricted.
    pub fn populated_property_names(&self) -> &[String] {
        &self.populated_property_names
    }

    /// The performance profile the engine opens its data file with.
    pub fn performance_profile(&self) -> PerformanceProfile {
        self.profile
    }

    /// Every property the loaded data file carries, read from the data set.
    ///
    /// Unlike [`populated_property_names`](Self::populated_property_names) (the
    /// curated typed set the engine maps results into), this is the data file's
    /// own catalogue, so it reflects the tier: an ASN file lists its
    /// autonomous-system and registered-range properties, while an Enterprise
    /// file additionally lists the location and behavioural ones. Intended for
    /// introspection, such as the metadata example.
    pub fn available_property_names(&self) -> Vec<String> {
        self.loaded.load().manager.available_property_names()
    }

    /// Look up the client IP in `data`'s evidence, through
    /// [`Results::process_evidence`](fiftyone_native::ipi::Results::process_evidence),
    /// into an [`IpIntelligenceDataBase`] that reads each property from the
    /// native results the first time it is requested.
    ///
    /// Returns an error when the evidence carries no client IP address that
    /// reads as one. The native results stay with the returned data until it is
    /// dropped.
    fn lookup(&self, data: &FlowData) -> Result<IpIntelligenceDataBase> {
        // Load the current data set without blocking a concurrent refresh.
        let loaded = self.loaded.load_full();
        let mut results = loaded.manager.create_results()?;
        results.process_evidence(data.evidence())?;
        let mut ip = IpIntelligenceDataBase::new(IP_DATA_KEY_NAME);
        ip.base_mut()
            .add_missing_property_source(Arc::clone(&loaded.missing));
        Ok(ip.with_reader(LookupReader {
            results: Mutex::new(results),
            loaded,
        }))
    }

    /// Reopen the data file from disk and atomically swap it in.
    ///
    /// Shared by [`OnPremiseAspectEngine::refresh`]. A concurrent `process` call
    /// keeps using the old data set until this swap completes. The evidence
    /// keys, the tier and the publish date follow the new data file, which can
    /// differ from the old one. A failed reload changes nothing.
    fn reload_from_disk(&self) -> Result<()> {
        let _refreshing = self
            .refresh_lock
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let path = self
            .data_files
            .first()
            .and_then(|file| file.data_file_path().map(|p| p.to_path_buf()))
            .ok_or_else(|| {
                Error::configuration(
                    "Cannot refresh an IP-intelligence engine that has no data file path.",
                )
            })?;

        let property_refs: Option<Vec<&str>> = if self.requested_properties.is_empty() {
            None
        } else {
            Some(
                self.requested_properties
                    .iter()
                    .map(String::as_str)
                    .collect(),
            )
        };
        let copy = self.data_files.first().and_then(|file| {
            OperationalCopy::when_needed(
                &path,
                self.temp_dir.as_deref(),
                self.profile,
                file.configuration(),
            )
        });
        let manager = Manager::open_with_options(
            copy.as_ref().map_or(path.as_path(), OperationalCopy::path),
            self.profile,
            property_refs.as_deref(),
            self.concurrency,
        )?;
        let evidence_keys = DataFileEvidenceKeys::whitelist(&manager);
        let metadata = TierMetadata::new(
            &manager,
            &self.tier_override,
            &self.populated_property_names,
        );
        if let Some(data_file) = self.data_files.first() {
            record_published(&manager, data_file);
        }
        self.loaded.store(Arc::new(LoadedDataSet::new(
            manager,
            copy,
            &self.populated_property_names,
            &metadata,
        )));
        self.evidence_key_filter.keys.store(Arc::new(evidence_keys));
        self.metadata.replace(metadata);
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

/// Resolve the populated property names from the requested set.
///
/// An empty requested set means every typed property. Otherwise the typed
/// properties that intersect the requested set (case-insensitively, in the
/// shared model's declaration order) come first, then any explicitly requested
/// name that is not one of the typed properties is appended in requested order.
/// Honouring a requested non-typed name lets the engine surface properties a
/// specialised data file carries that the curated typed set does not model, such
/// as the `Asn` and `AsnName` properties in an ASN data file. A requested name
/// that the data file does not contain simply yields no value, the same as a
/// typed property with no candidates.
fn populated_property_names(requested: &[String]) -> Vec<String> {
    let generated = generated_property_names();
    if requested.is_empty() {
        return generated.iter().map(|s| s.to_string()).collect();
    }
    let mut names: Vec<String> = generated
        .iter()
        .filter(|name| {
            requested
                .iter()
                .any(|wanted| wanted.eq_ignore_ascii_case(name))
        })
        .map(|s| s.to_string())
        .collect();
    for wanted in requested {
        let already_typed = generated
            .iter()
            .any(|name| wanted.eq_ignore_ascii_case(name));
        let already_added = names.iter().any(|name| name.eq_ignore_ascii_case(wanted));
        if !already_typed && !already_added {
            names.push(wanted.clone());
        }
    }
    names
}

/// The metadata value type for a property.
///
/// Uses the declared type for the generated model's properties (string, bool,
/// double, integer or the weighted key-value list). For any other requested
/// property the type is not known without a lookup, so it defaults to the
/// weighted key-value-list type, which is how a weighted property such as
/// `CountryCodesGeographical` is published.
fn metadata_value_type(name: &str) -> PropertyValueType {
    declared_property_value_type(name)
        .unwrap_or(fiftyone_ip_intelligence_shared::WEIGHTED_PROPERTY_VALUE_TYPE)
}

/// Build the core property metadata for the populated properties.
fn build_core_metadata(names: &[String]) -> Vec<PropertyMetaData> {
    names
        .iter()
        .map(|name| {
            PropertyMetaData::new(name.clone(), IP_DATA_KEY_NAME, metadata_value_type(name))
        })
        .collect()
}

/// Build the aspect property metadata for the populated properties, tagging
/// each with the engine's data-source tier so missing-property reasoning works.
fn build_aspect_metadata(names: &[String], tier: &str) -> Vec<AspectPropertyMetaData> {
    names
        .iter()
        .map(|name| {
            AspectPropertyMetaData::new(name.clone(), IP_DATA_KEY_NAME, metadata_value_type(name))
                .with_data_tiers([tier])
        })
        .collect()
}

/// A loaded data set, with the populated properties it carries and the copy
/// of the data file it was read from, if any.
struct LoadedDataSet {
    manager: Arc<Manager>,
    readable: Arc<ReadableNames>,
    /// Why a property is missing, from the metadata of this data set, added to
    /// every result a lookup writes.
    missing: Arc<MissingPropertySource>,
    /// Held only to be dropped with the data set, and declared after the
    /// manager, so it is removed after the data set that has it open is freed.
    _copy: Option<OperationalCopy>,
}

impl LoadedDataSet {
    fn new(
        manager: Arc<Manager>,
        copy: Option<OperationalCopy>,
        populated: &[String],
        metadata: &TierMetadata,
    ) -> Self {
        let readable = Arc::new(ReadableNames::new(populated, &manager));
        let missing = Arc::new(
            MissingPropertySource::new(
                IP_DATA_KEY_NAME,
                ENGINE_TYPE_NAME,
                EngineDeployment::OnPremise,
                metadata.tier.clone(),
                metadata.aspect_properties.clone(),
            )
            .with_typed_properties(is_typed_property),
        );
        LoadedDataSet {
            manager,
            readable,
            missing,
            _copy: copy,
        }
    }
}

/// The populated properties a data set carries, each with its index in that
/// data set, found by lowercase name. A property the data file does not carry
/// has no value, so a lookup never reads it.
struct ReadableNames {
    /// Each property's name as the engine populates it.
    names: Vec<String>,
    /// Each property's name in lowercase, in the same order.
    lowercase_names: Vec<String>,
    /// Each property's required property index in the data set.
    indexes: Vec<c_int>,
    /// The position of each property in `names`, by lowercase name.
    by_name: HashMap<String, usize>,
}

impl ReadableNames {
    fn new(populated: &[String], manager: &Manager) -> Self {
        let (names, indexes): (Vec<String>, Vec<c_int>) = populated
            .iter()
            .filter_map(|name| {
                manager
                    .required_property_index(name)
                    .map(|index| (name.clone(), index))
            })
            .unzip();
        let lowercase_names: Vec<String> = names.iter().map(|name| name.to_lowercase()).collect();
        ReadableNames {
            by_name: lowercase_names
                .iter()
                .enumerate()
                .map(|(position, name)| (name.clone(), position))
                .collect(),
            names,
            lowercase_names,
            indexes,
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
    /// The data set the results came from, with its properties.
    loaded: Arc<LoadedDataSet>,
}

impl IpIntelligencePropertyReader for LookupReader {
    fn property_names(&self) -> &[String] {
        &self.loaded.readable.lowercase_names
    }

    fn position(&self, name: &str) -> Option<usize> {
        self.loaded.readable.by_name.get(name).copied()
    }

    fn read_property(
        &self,
        position: usize,
        data: &mut IpIntelligenceDataBase,
    ) -> std::result::Result<(), String> {
        let name = &self.loaded.readable.names[position];
        let index = self.loaded.readable.indexes[position];
        let results = self.results.lock().unwrap_or_else(PoisonError::into_inner);
        populate_property(data, &results, name, index).map_err(|error| error.to_string())
    }
}

/// Read one property from the results and store it on `ip_data`, typed by the
/// property's declared value type.
///
/// The native [`Results::read_property`] decides plain versus weighted from the
/// value type the data file stores, so a weighted property is stored as a
/// weighted string list. For a plain property the *declared* metadata type is
/// the source of truth, because the boolean properties are stored as their
/// rendered string (`StoredValueType` is `string`) yet are declared `bool`: a
/// declared-boolean property is converted from its rendered value and stored
/// through [`IpIntelligenceDataBase::set_boolean`] so its strongly-typed
/// accessor reads back a [`bool`]. A property with no value is left unset, so
/// its accessor reports the default no-value.
fn populate_property(
    ip_data: &mut IpIntelligenceDataBase,
    results: &Results,
    name: &str,
    index: c_int,
) -> Result<()> {
    let declared = declared_property_value_type(name);

    // A property declared weighted (the country-code distributions and `Mcc`) is
    // always read through the weighted collection, even when the data file stores
    // its candidates as plain strings (their `StoredValueType` is `string`, so the
    // native value type alone would misclassify them as plain). The declared
    // metadata type is the source of truth for weighted-ness.
    if declared == Some(WEIGHTED_PROPERTY_VALUE_TYPE) {
        let pairs = results.values_weighted_by_index(index)?;
        if !pairs.is_empty() {
            let values = pairs
                .into_iter()
                .map(|(value, weighting)| WeightedValue::new(weighting, value))
                .collect();
            ip_data.set_weighted_string(name, values);
        }
        return Ok(());
    }

    match results.read_property_by_index(index)? {
        PropertyRead::Absent => {}
        PropertyRead::Weighted(pairs) => {
            let values = pairs
                .into_iter()
                .map(|(value, weighting)| WeightedValue::new(weighting, value))
                .collect();
            ip_data.set_weighted_string(name, values);
        }
        PropertyRead::Boolean(value) => ip_data.set_boolean(name, value),
        PropertyRead::Integer(value) => {
            if declared == Some(PropertyValueType::Bool) {
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
            Some(PropertyValueType::Bool) => {
                if let Some(flag) = parse_bool(&value) {
                    ip_data.set_boolean(name, flag);
                }
            }
            Some(PropertyValueType::Integer) => {
                if let Ok(int) = value.trim().parse::<i32>() {
                    ip_data.set_integer(name, int);
                }
            }
            Some(PropertyValueType::Double) => {
                if let Ok(float) = value.trim().parse::<f32>() {
                    ip_data.set_float(name, float);
                }
            }
            _ => ip_data.set_string(name, value),
        },
    }
    Ok(())
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
        &self.properties
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
        // Single-file engine: the identifier is ignored. Reopen from disk and
        // swap the new data set in atomically.
        self.reload_from_disk()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    use fiftyone_pipeline_core::{Evidence, Pipeline};
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

    /// Asserts the reader of the loaded data set covers exactly the populated
    /// properties the data file carries, each with its index in the data set.
    fn assert_reader_covers_carried_properties(engine: &IpIntelligenceOnPremiseEngine) {
        let loaded = engine.loaded.load();
        let readable = &loaded.readable;
        for (name, index) in readable.names.iter().zip(&readable.indexes) {
            assert_eq!(
                loaded.manager.required_property_index(name),
                Some(*index),
                "{name}"
            );
        }
        for name in engine.populated_property_names() {
            let carried = loaded.manager.required_property_index(name).is_some();
            assert_eq!(readable.names.contains(name), carried, "{name}");
        }
    }

    /// With no properties requested the engine populates the whole typed set,
    /// and each lookup reads only the part of it the data file carries.
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
    }

    /// The most expected concurrency the Balanced profile opens with. Its
    /// smallest cache, the profiles, holds 10,000 entries, and a cache needs at
    /// least as many entries in each of its parts as it has parts.
    const BALANCED_MOST_CONCURRENCY: u16 = 100;

    /// True when opening the data set was refused for a cache that cannot be
    /// split into the parts the expected concurrency asks for.
    fn refused_for_concurrency(result: Result<()>) -> bool {
        matches!(
            result,
            Err(Error::Native { status, .. }) if status == "InvalidCollectionConfig"
        )
    }

    /// The expected concurrency reaches the native data set when the engine
    /// is built and when it is refreshed, as one more than the Balanced
    /// profile allows is refused only if it gets there.
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

        let too_many = builder()
            .concurrency(BALANCED_MOST_CONCURRENCY + 1)
            .build()
            .map(drop);
        assert!(refused_for_concurrency(too_many));
        let most = builder()
            .concurrency(BALANCED_MOST_CONCURRENCY)
            .build()
            .expect("the most concurrency Balanced allows opens");
        drop(most);

        let engine = builder()
            .build()
            .expect("the profile's own concurrency opens")
            .with_concurrency(Some(BALANCED_MOST_CONCURRENCY + 1));
        assert!(refused_for_concurrency(engine.refresh(None)));
        let engine = engine.with_concurrency(Some(BALANCED_MOST_CONCURRENCY));
        engine
            .refresh(None)
            .expect("a refresh with the most concurrency Balanced allows opens");
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

    #[test]
    fn requested_properties_restrict_populated_set() {
        // The generated set is ordered by name, so an intersection preserves that
        // order: "Country" sorts before "RegisteredCountry".
        let restricted =
            populated_property_names(&["RegisteredCountry".to_owned(), "country".to_owned()]);
        assert_eq!(restricted, vec!["Country", "RegisteredCountry"]);

        let all = populated_property_names(&[]);
        assert_eq!(all.len(), generated_property_names().len());
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

    /// Evidence with no client IP is a clean processing error.
    #[test]
    fn missing_client_ip_is_an_error() {
        let Some(engine) = asn_engine() else {
            eprintln!("no usable IP-intelligence data file; skipping no-IP test");
            return;
        };
        // The engine drives the FlowData, so add it to the pipeline (a pipeline
        // must contain at least one element). Keep a concrete handle to the same
        // engine so the no-IP lookup can be called directly and its error type
        // asserted. The pipeline takes a trait-object clone of that handle.
        let engine = Arc::new(engine);
        let element: Arc<dyn fiftyone_pipeline_core::FlowElement> = Arc::clone(&engine) as _;
        let pipeline = Pipeline::builder()
            .add_element(element)
            .build()
            .expect("pipeline should build");
        let data = pipeline
            .create_flow_data_with(Evidence::builder().add("header.user-agent", "x").build());
        let err = engine
            .lookup(&data)
            .expect_err("a lookup with no client IP should error");
        assert!(matches!(err, Error::Native { .. }));
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
