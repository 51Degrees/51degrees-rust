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

//! The cloud IP-intelligence engine and its builder.

use std::collections::HashMap;
use std::sync::Arc;

use once_cell::sync::OnceCell;

use fiftyone_cloud_request_engine::{CloudRequestEngine, LicensedProducts};
use fiftyone_ip_intelligence_shared::{
    declared_property_value_type, default_aspect_property_metadata, default_property_metadata,
    IpIntelligenceDataBase, IP_DATA_KEY, IP_DATA_KEY_NAME, WEIGHTED_PROPERTY_VALUE_TYPE,
};
use fiftyone_pipeline_core::{
    Error, EvidenceKeyFilter, EvidenceKeyFilterWhitelist, FlowData, FlowElement, PropertyMetaData,
    PropertyValueType, Result,
};
use fiftyone_pipeline_engines::{
    default_missing_property_reason, AspectEngine, AspectPropertyMetaData, EngineDeployment,
    MissingPropertyResult,
};
use serde_json::Value;

use crate::parse::{self, ValueKind};

/// The engine's name in log records.
const ENGINE_NAME: &str = "IpIntelligenceCloudEngine";

/// The metadata derived from the request engine's accessible properties for the
/// IPI product, kept together so the three pieces are computed and cached as a
/// unit.
struct DerivedMetadata {
    /// The core property metadata.
    core: Vec<PropertyMetaData>,
    /// The aspect view of the same metadata.
    aspect: Vec<AspectPropertyMetaData>,
    /// The property name (lower case) to value kind map used during parsing.
    kinds: HashMap<String, ValueKind>,
}

/// The cloud IP-intelligence engine.
///
/// The engine consumes the JSON response stored by a
/// [`CloudRequestEngine`] earlier in the same pipeline, slices out the `ip`
/// member it owns and deserializes it into an [`IpIntelligenceDataBase`], which
/// it stores under [`IP_DATA_KEY`]. That is the same element-data type and key
/// the on-premise engine produces, so the two are interchangeable to a
/// consumer.
///
/// # Evidence
///
/// This engine reads no evidence of its own. It works entirely from the cloud
/// response, so its [`FlowElement::evidence_key_filter`] is an empty whitelist.
///
/// # Metadata
///
/// The property metadata is derived from the
/// [`CloudRequestEngine::public_properties`] of the supplied request engine when
/// the engine is built, taking the product whose key is `ip`. When the request
/// engine's discovery has not completed, it is derived the first time it is
/// needed after it has, and until then the engine reports the shared default
/// metadata so a consumer always sees the documented property set. A resource
/// key that grants no `ip` product, or lists it with no properties, leaves the
/// metadata empty and logs an error once. The metadata `Type` field of each
/// cloud property also drives how its weighted values are parsed (string,
/// integer or floating-point).
///
/// # Example
///
/// ```no_run
/// use std::sync::Arc;
/// use fiftyone_pipeline_core::{Evidence, Pipeline};
/// use fiftyone_cloud_request_engine::CloudRequestEngine;
/// use fiftyone_ip_intelligence_cloud::{IpIntelligenceCloudEngine, IP_DATA_KEY};
///
/// let request_engine = Arc::new(
///     CloudRequestEngine::builder()
///         .resource_key("my-resource-key")
///         .build()
///         .unwrap(),
/// );
/// let ipi_engine = IpIntelligenceCloudEngine::builder()
///     .cloud_request_engine(request_engine.clone())
///     .build()
///     .unwrap();
///
/// let pipeline = Pipeline::builder()
///     .add_element(request_engine)
///     .add_element(Arc::new(ipi_engine))
///     .build()
///     .unwrap();
///
/// let mut data = pipeline.create_flow_data_with(
///     Evidence::builder().add("query.client-ip-51d", "185.28.167.78").build(),
/// );
/// data.process().unwrap();
/// if let Some(ip) = data.get(IP_DATA_KEY) {
///     use fiftyone_ip_intelligence_cloud::IpIntelligenceData;
///     // CountryCode is a plain string property, read through its typed accessor.
///     if let Ok(country_code) = ip.country_code().value() {
///         println!("country code: {country_code}");
///     }
/// }
/// ```
pub struct IpIntelligenceCloudEngine {
    /// The request engine whose response and accessible-properties this engine
    /// reads. Held so the metadata can be derived once discovery completes.
    request_engine: Arc<CloudRequestEngine>,

    /// The empty evidence filter. This engine reads no evidence directly.
    evidence_filter: EvidenceKeyFilterWhitelist,

    /// Core metadata for the IPI product, derived once the request engine's
    /// discovery has completed.
    properties: OnceCell<Vec<PropertyMetaData>>,
    /// The aspect view of the same metadata.
    aspect_properties: OnceCell<Vec<AspectPropertyMetaData>>,
    /// Property name (lower case) to value kind, derived with the metadata and
    /// used to parse each weighted property as the right type.
    value_kinds: OnceCell<HashMap<String, ValueKind>>,

    /// The shared default core metadata, returned before discovery completes.
    default_properties: Vec<PropertyMetaData>,
    /// The shared default aspect metadata, returned before discovery completes.
    default_aspect_properties: Vec<AspectPropertyMetaData>,
}

impl IpIntelligenceCloudEngine {
    /// Start building a cloud IP-intelligence engine.
    pub fn builder() -> IpIntelligenceCloudEngineBuilder {
        IpIntelligenceCloudEngineBuilder::new()
    }

    /// The request engine this engine reads its input and metadata from.
    pub fn cloud_request_engine(&self) -> &Arc<CloudRequestEngine> {
        &self.request_engine
    }

    /// Derive the IPI metadata from the request engine's accessible properties,
    /// taking the product keyed by `ip`. Returns `None` when there is no such
    /// product or it lists no properties.
    fn derive_metadata(products: &LicensedProducts) -> Option<DerivedMetadata> {
        // The product is looked up by the element data key, which for IPI is
        // `ip`.
        let product = products.products.get(IP_DATA_KEY_NAME)?;
        if product.properties.is_empty() {
            return None;
        }

        let mut core = Vec::with_capacity(product.properties.len());
        let mut aspect = Vec::with_capacity(product.properties.len());
        let mut kinds = HashMap::with_capacity(product.properties.len());

        for property in &product.properties {
            // Publish each property with its declared value type. A weighted
            // property is surfaced through the dynamic bag as the flattened
            // key-value-list type; the plain scalars keep their type.
            let kind = ValueKind::from_cloud_type(&property.value_type);
            let value_type = match kind {
                ValueKind::Integer => PropertyValueType::Integer,
                ValueKind::Double => PropertyValueType::Double,
                ValueKind::Bool => PropertyValueType::Bool,
                ValueKind::WeightedString => WEIGHTED_PROPERTY_VALUE_TYPE,
                ValueKind::String => PropertyValueType::String,
            };
            let core_meta = PropertyMetaData::new(&property.name, IP_DATA_KEY_NAME, value_type);
            let mut aspect_meta = AspectPropertyMetaData::from_core(core_meta.clone());
            if let Some(tier) = &product.data_tier {
                aspect_meta = aspect_meta.with_data_tiers([tier.clone()]);
            }
            core.push(core_meta);
            aspect.push(aspect_meta);
            kinds.insert(property.name.to_ascii_lowercase(), kind);
        }
        Some(DerivedMetadata {
            core,
            aspect,
            kinds,
        })
    }

    /// Derive the metadata from the request engine's accessible properties if
    /// it is not there yet. Makes no cloud request.
    ///
    /// While the request engine's discovery has not completed the caches stay
    /// empty and the engine reports the shared defaults. A resource key that
    /// grants no `ip` product, or lists it with no properties, gets empty
    /// metadata, as the Device Detection cloud engine and .NET do, and the
    /// call that sets it logs an error, so it is logged once. With only a
    /// license key there are no accessible properties to read, so the engine
    /// keeps the defaults and logs nothing. The properties are set last
    /// because a request that finds them set reads the rest.
    fn ensure_metadata(&self) {
        if self.properties.get().is_some() {
            return;
        }
        let Ok(products) = self.request_engine.public_properties() else {
            return;
        };
        match Self::derive_metadata(products) {
            Some(derived) => {
                let _ = self.value_kinds.set(derived.kinds);
                let _ = self.aspect_properties.set(derived.aspect);
                let _ = self.properties.set(derived.core);
            }
            None if self.request_engine.resource_key().is_some() => {
                let _ = self.value_kinds.set(HashMap::new());
                let _ = self.aspect_properties.set(Vec::new());
                if self.properties.set(Vec::new()).is_ok() {
                    log::error!(
                        "Aspect properties could not be loaded for {ENGINE_NAME} - \
                         '{IP_DATA_KEY_NAME}'"
                    );
                }
            }
            None => {}
        }
    }

    /// The value kinds derived from the cloud metadata, or an empty map before
    /// discovery completes. An empty map makes the parser infer each kind from
    /// the JSON value.
    fn value_kinds(&self) -> HashMap<String, ValueKind> {
        self.ensure_metadata();
        self.value_kinds.get().cloned().unwrap_or_default()
    }

    /// Read the upstream cloud JSON, slice out the `ip` aspect and build the
    /// element data from it.
    ///
    /// Returns an [`Error::PipelineConfiguration`] when there is no cloud
    /// request data, which means no [`CloudRequestEngine`] ran before this
    /// engine. A request engine that ran and failed has already recorded its
    /// own error and left no response, which gives `None`.
    fn build_data(&self, data: &FlowData) -> Result<Option<IpIntelligenceDataBase>> {
        let cloud = data.get(CloudRequestEngine::DATA_KEY).ok_or_else(|| {
            Error::configuration(
                "No cloud request data is present. This is probably because there \
                 is no 'CloudRequestEngine' before the 'IpIntelligenceCloudEngine' \
                 in the pipeline. This engine cannot produce results until that is \
                 corrected.",
            )
        })?;

        match cloud.json_response() {
            Some(json) if !json.is_empty() => self.parse_json(json).map(Some),
            _ => Ok(None),
        }
    }

    /// Parse a full cloud response body into the element data, slicing out the
    /// `ip` member. Split out from [`IpIntelligenceCloudEngine::build_data`] so
    /// it can be unit tested without a live pipeline.
    fn parse_json(&self, json: &str) -> Result<IpIntelligenceDataBase> {
        let root: Value = serde_json::from_str(json).map_err(|e| {
            Error::configuration(format!("failed to parse the cloud JSON response: {e}"))
        })?;

        let mut element = IpIntelligenceDataBase::new(IP_DATA_KEY_NAME);

        // The aspect lives under the `ip` member. An absent or non-object member
        // yields empty data rather than an error, so a response that simply
        // carried no IPI data still produces a valid (empty) result.
        if let Some(Value::Object(aspect)) = root.get(IP_DATA_KEY_NAME) {
            let kinds = self.value_kinds();
            parse::populate_from_aspect(&mut element, aspect, &kinds);
        }

        Ok(element)
    }
}

impl FlowElement for IpIntelligenceCloudEngine {
    /// Stores the IP data the cloud returned. When the cloud request engine
    /// left no response, it logs that at info level, records a flow error that
    /// does not fail processing, and stores empty data marked as from a failed
    /// cloud request, so each property read gives
    /// [`MissingPropertyReason::CloudRequestFailed`](fiftyone_pipeline_core::MissingPropertyReason::CloudRequestFailed),
    /// as .NET does.
    fn process(&self, data: &mut FlowData) -> Result<()> {
        // Read the metadata if discovery has completed since construction,
        // whether or not this response carries an `ip` member, so a key
        // without the product is logged and later elements see its metadata.
        self.ensure_metadata();
        let element = match self.build_data(data)? {
            Some(element) => element,
            None => {
                CloudRequestEngine::record_not_processed(data, ENGINE_NAME, IP_DATA_KEY_NAME);
                let mut element = IpIntelligenceDataBase::new(IP_DATA_KEY_NAME);
                element.base_mut().mark_cloud_request_failed();
                element
            }
        };
        // The on-premise and cloud engines store identical types under the same
        // key, so a consumer can swap one for the other unchanged.
        data.get_or_add(IP_DATA_KEY, || element)?;
        Ok(())
    }

    fn data_key(&self) -> &str {
        IP_DATA_KEY_NAME
    }

    fn evidence_key_filter(&self) -> &dyn EvidenceKeyFilter {
        &self.evidence_filter
    }

    fn properties(&self) -> &[PropertyMetaData] {
        match self.properties.get() {
            Some(properties) => properties,
            None => &self.default_properties,
        }
    }
}

impl AspectEngine for IpIntelligenceCloudEngine {
    fn engine_type_name(&self) -> &str {
        ENGINE_NAME
    }

    fn data_source_tier(&self) -> &str {
        // Cloud engines have no on-premise data tier.
        ""
    }

    fn deployment(&self) -> EngineDeployment {
        EngineDeployment::Cloud
    }

    fn aspect_properties(&self) -> &[AspectPropertyMetaData] {
        match self.aspect_properties.get() {
            Some(properties) => properties,
            None => &self.default_aspect_properties,
        }
    }

    fn has_loaded_properties(&self) -> bool {
        self.properties.get().is_some()
    }

    fn missing_property_reason(&self, property_name: &str) -> MissingPropertyResult {
        // Read the metadata if discovery has completed, so the reason reflects
        // the live cloud metadata rather than the pre-discovery defaults.
        self.ensure_metadata();
        default_missing_property_reason(self, property_name)
    }

    /// Whether `IpIntelligenceData` has a typed accessor for the property,
    /// matched ignoring case. Before discovery completes, such a property
    /// gives a data file upgrade as the reason it is missing, as .NET does.
    fn has_typed_property(&self, property_name: &str) -> bool {
        declared_property_value_type(property_name).is_some()
    }
}

/// A fluent builder for [`IpIntelligenceCloudEngine`] instances.
///
/// The cloud request engine is required, as the cloud engine reads both its
/// JSON response and its accessible properties.
pub struct IpIntelligenceCloudEngineBuilder {
    request_engine: Option<Arc<CloudRequestEngine>>,
}

impl IpIntelligenceCloudEngineBuilder {
    /// Create a builder with no request engine set.
    pub fn new() -> Self {
        IpIntelligenceCloudEngineBuilder {
            request_engine: None,
        }
    }

    /// Set the [`CloudRequestEngine`] this engine reads from (required). The same
    /// request engine instance must be added to the pipeline before the IPI
    /// cloud engine.
    pub fn cloud_request_engine(mut self, engine: Arc<CloudRequestEngine>) -> Self {
        self.request_engine = Some(engine);
        self
    }

    /// Build the engine, returning an [`Error::PipelineConfiguration`] if no
    /// cloud request engine was supplied.
    ///
    /// The metadata is read from the request engine here, which makes no cloud
    /// request, and an error is logged when the request engine has a resource
    /// key that is not granted the IP Intelligence product. When the request
    /// engine's discovery has not completed, both wait until it has.
    pub fn build(self) -> Result<IpIntelligenceCloudEngine> {
        let request_engine = self.request_engine.ok_or_else(|| {
            Error::configuration(
                "an IpIntelligenceCloudEngine requires a CloudRequestEngine; \
                 set one with .cloud_request_engine(..)",
            )
        })?;

        let engine = IpIntelligenceCloudEngine {
            request_engine,
            evidence_filter: EvidenceKeyFilterWhitelist::new(Vec::<String>::new()),
            properties: OnceCell::new(),
            aspect_properties: OnceCell::new(),
            value_kinds: OnceCell::new(),
            default_properties: default_property_metadata(),
            default_aspect_properties: default_aspect_property_metadata(),
        };
        engine.ensure_metadata();
        Ok(engine)
    }
}

impl Default for IpIntelligenceCloudEngineBuilder {
    fn default() -> Self {
        IpIntelligenceCloudEngineBuilder::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fiftyone_cloud_request_engine::{
        CloudEngineState, CloudHttpClient, CloudHttpRequest, CloudHttpResponse,
    };
    use fiftyone_ip_intelligence_shared::IpIntelligenceData;
    use fiftyone_pipeline_core::MissingPropertyReason;

    /// A request engine wired to a stub HTTP client, so no network is touched.
    ///
    /// An empty state is supplied so the builder resolves without a discovery
    /// fetch (the stub would otherwise error). The empty accessible-properties set
    /// grants no `ip` product, which leaves the engine with no metadata and
    /// exercises the fall back to inferred value kinds.
    fn request_engine() -> Arc<CloudRequestEngine> {
        struct Stub;
        impl CloudHttpClient for Stub {
            fn send(
                &self,
                _request: &CloudHttpRequest,
            ) -> std::result::Result<CloudHttpResponse, String> {
                Err("no network in tests".to_owned())
            }
        }
        Arc::new(
            CloudRequestEngine::builder()
                .resource_key("test-key")
                .http_client(Arc::new(Stub))
                .set_state(CloudEngineState::default())
                .build()
                .unwrap(),
        )
    }

    fn engine() -> IpIntelligenceCloudEngine {
        IpIntelligenceCloudEngine::builder()
            .cloud_request_engine(request_engine())
            .build()
            .unwrap()
    }

    #[test]
    fn build_requires_request_engine() {
        match IpIntelligenceCloudEngine::builder().build() {
            Err(Error::PipelineConfiguration { .. }) => {}
            Err(other) => panic!("expected a configuration error, got {other:?}"),
            Ok(_) => panic!("expected a configuration error without a request engine"),
        }
    }

    #[test]
    fn a_key_without_the_ip_product_has_no_properties_and_says_why() {
        let engine = engine();
        assert!(engine.has_loaded_properties());
        assert!(engine.properties().is_empty());
        assert!(engine.aspect_properties().is_empty());
        assert_eq!(
            engine.missing_property_reason("RegisteredName").reason,
            MissingPropertyReason::ProductNotAccessibleWithResourceKey
        );
    }

    #[test]
    fn data_key_and_deployment_match_contract() {
        let engine = engine();
        assert_eq!(engine.data_key(), "ip");
        assert_eq!(engine.deployment(), EngineDeployment::Cloud);
        assert_eq!(engine.data_source_tier(), "");
        // No evidence is read.
        assert!(!engine.evidence_key_filter().include("header.user-agent"));
    }

    #[test]
    fn parses_a_representative_response() {
        // A representative IPI cloud response: a plain string property, a
        // genuinely weighted multi-candidate array, and a null property with a
        // top-level reason. Property kinds are inferred because discovery cannot
        // reach the stub cloud (a `{rawweighting,value}` array is weighted, a
        // bare scalar is plain), so the integer/double cases use bare scalars in
        // a separate test below.
        let json = r#"{
            "ip": {
                "Country": "United Kingdom",
                "CountryCodesGeographical": [
                    { "rawweighting": 20000, "value": "GB" },
                    { "rawweighting": 60000, "value": "FR" }
                ],
                "RegisteredOwner": null,
                "nullValueReasons": {
                    "RegisteredOwner": "The results are empty. We don't have this data."
                }
            }
        }"#;

        let data = engine().parse_json(json).unwrap();

        // A plain string property.
        assert_eq!(data.country().value().unwrap().as_str(), "United Kingdom");

        // A genuinely weighted property: the multi-candidate array, ordered high
        // weighting first by the setter.
        let codes = data
            .weighted_string("CountryCodesGeographical")
            .into_value()
            .unwrap();
        assert_eq!(codes.len(), 2);
        assert_eq!(codes[0].value, "FR");
        assert_eq!(codes[0].raw_weighting, 60000);
        assert_eq!(codes[1].value, "GB");

        // The null property carries its reason from the top-level object.
        let owner = data.registered_owner();
        assert!(!owner.has_value());
        assert_eq!(
            owner.no_value_message(),
            Some("The results are empty. We don't have this data.")
        );
    }

    #[test]
    fn nullreason_sibling_is_honoured() {
        let json = r#"{
            "ip": {
                "Town": null,
                "Townnullreason": "no location for this IP"
            }
        }"#;
        let data = engine().parse_json(json).unwrap();
        let town = data.town();
        assert!(!town.has_value());
        assert_eq!(town.no_value_message(), Some("no location for this IP"));
    }

    #[test]
    fn integer_and_double_properties_parse_by_inference() {
        // With no cloud metadata the parser infers integer vs double from a bare
        // JSON number, so a whole number becomes an integer property and a
        // fractional number a double property.
        let json = r#"{
            "ip": {
                "TimeZoneOffset": 60,
                "Latitude": 51.45
            }
        }"#;
        let data = engine().parse_json(json).unwrap();
        assert_eq!(*data.time_zone_offset().value().unwrap(), 60_i32);
        assert!((data.latitude().value().unwrap() - 51.45_f32).abs() < 1e-4);
    }

    #[test]
    fn missing_cloud_data_is_a_configuration_error() {
        use fiftyone_pipeline_core::{Evidence, Pipeline};
        // A pipeline with only the IPI engine (no request engine before it).
        let pipeline = Pipeline::builder()
            .add_element(Arc::new(engine()))
            .suppress_process_exceptions(false)
            .build()
            .unwrap();
        let mut data = pipeline.create_flow_data_with(Evidence::builder().build());
        let result = data.process();
        assert!(result.is_err(), "missing upstream cloud data must error");
    }

    #[test]
    fn empty_ip_member_yields_empty_data() {
        let json = r#"{ "ip": {} }"#;
        let data = engine().parse_json(json).unwrap();
        // No properties were populated, so a typed accessor is a default
        // no-value.
        assert!(!data.country_code().has_value());
    }

    /// Resolve a cloud resource key from the environment for the live test,
    /// honouring the aligned name first and then the CI-exported bespoke and basic
    /// names, mirroring `examples-shared::keys::resource_key_from_env`, so the live
    /// test runs from an explicit key or the keys CI exports.
    ///
    /// Gated on `reqwest-client` to match its only caller, the live test below, so
    /// the helper is not flagged as dead code when the feature is off (for example
    /// the no-default-features wasm32-wasip1 build).
    #[cfg(feature = "reqwest-client")]
    fn live_resource_key() -> Option<String> {
        [
            "51DEGREES_RESOURCE_KEY",
            "_51DEGREES_RESOURCE_KEY_BESPOKE",
            "_51DEGREES_RESOURCE_KEY_BASIC",
        ]
        .into_iter()
        .find_map(|name| match std::env::var(name) {
            Ok(value) if !value.trim().is_empty() => Some(value.trim().to_owned()),
            _ => None,
        })
    }

    // Builds a CloudRequestEngine relying on the built-in reqwest client, so it
    // only compiles with the reqwest-client feature. The workspace build unifies
    // that feature on, and a standalone run needs `--features reqwest-client`.
    #[cfg(feature = "reqwest-client")]
    #[test]
    #[ignore = "requires network and a resource key (51DEGREES_RESOURCE_KEY or the _51DEGREES_RESOURCE_KEY_BESPOKE/_BASIC tiered names)"]
    fn live_cloud_returns_country_code() {
        use fiftyone_pipeline_core::{Evidence, Pipeline};

        let Some(resource_key) = live_resource_key() else {
            eprintln!("no resource key in the environment; skipping live cloud test");
            return;
        };

        let request_engine = Arc::new(
            CloudRequestEngine::builder()
                .resource_key(resource_key)
                .build()
                .unwrap(),
        );
        let ipi_engine = IpIntelligenceCloudEngine::builder()
            .cloud_request_engine(request_engine.clone())
            .build()
            .unwrap();

        let pipeline = Pipeline::builder()
            .add_element(request_engine)
            .add_element(Arc::new(ipi_engine))
            .build()
            .unwrap();

        let mut data = pipeline.create_flow_data_with(
            Evidence::builder()
                .add("query.client-ip-51d", "185.28.167.78")
                .build(),
        );
        // Processing must succeed regardless of the key's product tier: this
        // catches transport, authentication and deserialization regressions.
        data.process().expect("live cloud processing succeeds");

        let ip = data.get(IP_DATA_KEY).expect("ip data present");
        // The country code is only returned when the resource key grants the IP
        // intelligence (location) product. A device-only key processes cleanly
        // but yields no IPI values, so the content assertion is skipped in that
        // case rather than failing, mirroring the on-premise tests' "skip when
        // the environment can't exercise this" rule for an absent data file.
        match ip.country_code().value() {
            Ok(code) => assert!(
                !code.is_empty(),
                "a country code returned for a public IP should be non-empty"
            ),
            Err(_) => eprintln!(
                "the resource key returned no IP intelligence data (it may not grant the \
                 location product); skipping the live country-code assertion"
            ),
        }

        // Every property the cloud returned must unpack through the typed
        // accessor as the type the generated metadata declares, so a consumer
        // calling the strongly-typed accessor never gets a wrong-type no-value
        // for a value that is present.
        use fiftyone_ip_intelligence_shared::GENERATED_PROPERTY_TYPES;
        use fiftyone_pipeline_core::{ElementData, PropertyValueType};
        let mut offenders: Vec<String> = Vec::new();
        for (name, declared) in GENERATED_PROPERTY_TYPES {
            if ip.get(name).is_err() {
                continue;
            }
            let unpacks = match declared {
                PropertyValueType::Bool => ip.boolean(name).value().is_ok(),
                PropertyValueType::Integer => ip.integer(name).value().is_ok(),
                PropertyValueType::Double => ip.float(name).value().is_ok(),
                PropertyValueType::KeyValueList => ip.weighted_string(name).value().is_ok(),
                _ => ip.string(name).value().is_ok(),
            };
            if !unpacks {
                offenders.push(format!("{name} [{declared:?}] raw={:?}", ip.get(name).ok()));
            }
        }
        assert!(
            offenders.is_empty(),
            "cloud values not unpackable as their declared type:\n{}",
            offenders.join("\n")
        );
    }

    #[test]
    fn a_failed_cloud_request_is_not_reported_as_a_configuration_error() {
        // The request engine records its own error when its call fails and
        // leaves no response. Nothing in the configuration is wrong, so this
        // engine only records that it did not process, as an error that does
        // not fail processing, as .NET's cloud aspect engines do.
        use fiftyone_pipeline_core::{Evidence, Pipeline};
        let request_engine = request_engine();
        let ip_engine = IpIntelligenceCloudEngine::builder()
            .cloud_request_engine(request_engine.clone())
            .build()
            .unwrap();
        let pipeline = Pipeline::builder()
            .add_element(request_engine)
            .add_element(Arc::new(ip_engine))
            .suppress_process_exceptions(true)
            .build()
            .unwrap();
        let mut data = pipeline.create_flow_data_with(
            Evidence::builder()
                .add("query.client-ip-51d", "185.28.167.78")
                .build(),
        );
        data.process().unwrap();

        let reported: Vec<String> = data
            .errors()
            .iter()
            .map(|error| format!("{}: {}", error.element_data_key, error.source))
            .collect();
        assert_eq!(
            reported.len(),
            2,
            "the request engine's failure and this engine's record, got {reported:?}"
        );
        assert_eq!(data.errors()[1].element_data_key, IP_DATA_KEY_NAME);
        assert!(!data.errors()[1].should_throw);
        assert!(matches!(data.errors()[1].source, Error::Pipeline { .. }));
        assert!(
            !data
                .errors()
                .iter()
                .any(|error| matches!(error.source, Error::PipelineConfiguration { .. })),
            "a failed cloud request was reported as a configuration error: {reported:?}"
        );
    }
}
