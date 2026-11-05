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

//! The cloud 51Degrees-identifier engine and its builder.

use std::sync::Arc;

use once_cell::sync::OnceCell;

use fiftyone_cloud_request_engine::{CloudRequestEngine, LicensedProducts};
use fiftyone_pipeline_core::{
    Error, EvidenceKeyFilter, EvidenceKeyFilterWhitelist, FlowData, FlowElement, PropertyMetaData,
    PropertyValueType, Result,
};
use fiftyone_pipeline_engines::{
    missing_property_reason, AspectEngine, AspectPropertyMetaData, EngineDeployment,
    EngineMissingPropertyContext, MissingPropertyResult,
};
use serde_json::Value;

use crate::data::{
    default_aspect_property_metadata, default_property_metadata, FodIdDataBase, FODID_DATA_KEY,
    FODID_ELEMENT_DATA_KEY,
};
use crate::dto::map_fodid_object;

/// The engine's name in log records and messages.
const ENGINE_NAME: &str = "FodIdCloudEngine";

/// The cloud 51Degrees-identifier engine.
///
/// The engine consumes the JSON response stored by a [`CloudRequestEngine`]
/// earlier in the same pipeline, slices out the `fodid` member it owns and maps
/// it into a [`FodIdDataBase`], stored under [`FODID_DATA_KEY`]. That data
/// carries the two probabilistic identifiers (`IdProbGlobal` and `IdProbLic`) as
/// raw base64 OWID strings and, on demand, as parsed
/// [`FodId`](fodid::FodId) values.
///
/// # Where it sits
///
/// A cloud identifier pipeline has a [`CloudRequestEngine`] followed by a
/// [`FodIdCloudEngine`]. The request engine makes the single HTTP call and
/// stores the raw JSON under its `cloud` data key; this engine reads that JSON.
/// It must therefore be added to the pipeline *after* the request engine.
///
/// # Cloud only
///
/// Unlike device detection and IP intelligence there is no on-premise twin: a
/// 51Did is issued by the cloud, which alone holds the signing key. So this is
/// the only engine that produces [`FODID_DATA_KEY`], and there is no shared
/// crate to keep interface-compatible with.
///
/// # Evidence
///
/// This engine reads no evidence of its own; the request engine before it
/// gathers the device-detection evidence (User-Agent / UA-CH), the client IP and
/// the `id.usage` usage-policy value that drive the lookup. Its
/// [`FlowElement::evidence_key_filter`] is therefore an empty whitelist.
///
/// # Response cache
///
/// The cloud creates the 51Did values afresh for each request that asks for
/// them, so the request engine never stores a response carrying them in its
/// response cache and never serves one from it. Each visitor receives the 51Did
/// created for their own request, and a request that carries no `id.usage`
/// evidence, which asks for no 51Did, is still served from the cache.
///
/// # Metadata
///
/// The property metadata is derived lazily from the request engine's
/// accessible properties on first use, taking the product whose key is `fodid`
/// compared ignoring case, as the cloud service reports it as `FODid`.
/// Until that fetch succeeds the engine reports the shared default metadata (the
/// two identifier properties), so a consumer always sees the documented set.
///
/// # Example
///
/// ```no_run
/// use std::sync::Arc;
/// use fiftyone_pipeline_core::{Evidence, Pipeline};
/// use fiftyone_cloud_request_engine::CloudRequestEngine;
/// use fiftyone_fodid_cloud::{FodIdCloudEngine, FodIdData, FODID_DATA_KEY};
///
/// let request_engine = Arc::new(
///     CloudRequestEngine::builder()
///         .resource_key("my-resource-key")
///         .build()
///         .unwrap(),
/// );
/// let fodid_engine = FodIdCloudEngine::builder()
///     .cloud_request_engine(request_engine.clone())
///     .build()
///     .unwrap();
///
/// let pipeline = Pipeline::builder()
///     .add_element(request_engine)
///     .add_element(Arc::new(fodid_engine))
///     .build()
///     .unwrap();
///
/// let mut data = pipeline.create_flow_data_with(
///     Evidence::builder()
///         .add("header.user-agent", "Mozilla/5.0")
///         .add("query.client-ip", "185.28.167.78")
///         .add("query.id.usage", "non-marketing")
///         .build(),
/// );
/// data.process().unwrap();
/// if let Some(fodid) = data.get(FODID_DATA_KEY) {
///     if let Ok(id) = fodid.id_prob_global().value() {
///         println!("global 51Did: {id}");
///     }
/// }
/// ```
pub struct FodIdCloudEngine {
    /// The request engine whose response and accessible-properties this engine
    /// reads. Held so the metadata can be discovered lazily on first use.
    request_engine: Arc<CloudRequestEngine>,

    /// The empty evidence filter. This engine reads no evidence directly.
    evidence_filter: EvidenceKeyFilterWhitelist,

    /// Lazily derived core metadata for the identifier product, cached after the
    /// first successful discovery from the request engine.
    properties: OnceCell<Vec<PropertyMetaData>>,
    /// The aspect view of the same metadata.
    aspect_properties: OnceCell<Vec<AspectPropertyMetaData>>,

    /// The shared default core metadata, returned before discovery completes.
    default_properties: Vec<PropertyMetaData>,
    /// The shared default aspect metadata, returned before discovery completes.
    default_aspect_properties: Vec<AspectPropertyMetaData>,
}

impl FodIdCloudEngine {
    /// Start building a cloud 51Degrees-identifier engine.
    pub fn builder() -> FodIdCloudEngineBuilder {
        FodIdCloudEngineBuilder::new()
    }

    /// The request engine this engine reads its input and metadata from.
    pub fn cloud_request_engine(&self) -> &Arc<CloudRequestEngine> {
        &self.request_engine
    }

    /// Derive the identifier metadata from the request engine's accessible
    /// properties, taking the product keyed by `fodid` compared ignoring case,
    /// because the cloud service reports the product as `FODid`. Returns `None`
    /// if the discovery fetch fails or the resource key grants no identifier
    /// product, in which case the engine keeps reporting the shared defaults.
    fn derive_metadata(
        products: &LicensedProducts,
    ) -> Option<(Vec<PropertyMetaData>, Vec<AspectPropertyMetaData>)> {
        let product = products
            .products
            .iter()
            .find(|(name, _)| name.eq_ignore_ascii_case(FODID_ELEMENT_DATA_KEY))
            .map(|(_, product)| product)?;
        if product.properties.is_empty() {
            return None;
        }

        let mut core = Vec::with_capacity(product.properties.len());
        let mut aspect = Vec::with_capacity(product.properties.len());

        for property in &product.properties {
            // The identifier values are base64 strings, so the published value
            // type is always a string regardless of how the cloud labels it.
            let core_meta = PropertyMetaData::new(
                &property.name,
                FODID_ELEMENT_DATA_KEY,
                PropertyValueType::String,
            );
            let mut aspect_meta = AspectPropertyMetaData::from_core(core_meta.clone());
            if let Some(tier) = &product.data_tier {
                aspect_meta = aspect_meta.with_data_tiers([tier.clone()]);
            }
            core.push(core_meta);
            aspect.push(aspect_meta);
        }
        Some((core, aspect))
    }

    /// Ensure the lazily derived metadata is populated, fetching the request
    /// engine's accessible properties on first use. A discovery failure leaves
    /// the caches empty, so the engine continues with the shared defaults,
    /// degrading gracefully when the metadata cannot be loaded.
    fn ensure_metadata(&self) {
        if self.properties.get().is_some() {
            return;
        }
        if let Ok(products) = self.request_engine.public_properties() {
            if let Some((core, aspect)) = Self::derive_metadata(products) {
                let _ = self.properties.set(core);
                let _ = self.aspect_properties.set(aspect);
            }
        }
    }

    /// Read the upstream cloud JSON, slice out the `fodid` aspect and build the
    /// element data from it, or `None` when the request engine left no
    /// response.
    ///
    /// Returns an [`Error::PipelineConfiguration`] when there is no cloud
    /// request data, which almost always means no [`CloudRequestEngine`] ran
    /// before this engine.
    fn build_data(&self, data: &FlowData) -> Result<Option<FodIdDataBase>> {
        let cloud = data.get(CloudRequestEngine::DATA_KEY).ok_or_else(|| {
            Error::configuration(
                "No cloud request data is present. This is probably because there \
                 is no 'CloudRequestEngine' before the 'FodIdCloudEngine' in the \
                 pipeline. This engine cannot produce results until that is \
                 corrected.",
            )
        })?;

        // An empty or absent response means the request engine failed and has
        // already recorded its own error.
        match cloud.json_response() {
            Some(json) if !json.is_empty() => self.parse_json(json).map(Some),
            _ => Ok(None),
        }
    }

    /// Parse a full cloud response body into the element data, slicing out the
    /// `fodid` member. Split out from [`FodIdCloudEngine::build_data`] so it can
    /// be unit tested without a live pipeline.
    fn parse_json(&self, json: &str) -> Result<FodIdDataBase> {
        let root: Value = serde_json::from_str(json).map_err(|e| {
            Error::configuration(format!("failed to parse the cloud JSON response: {e}"))
        })?;

        // The aspect lives under the `fodid` member. An absent or non-object
        // member yields empty data rather than an error, so a response that
        // simply carried no identifier still produces a valid (empty) result.
        match root.get(FODID_ELEMENT_DATA_KEY) {
            Some(fodid) => Ok(map_fodid_object(fodid)),
            None => Ok(FodIdDataBase::new()),
        }
    }
}

impl FlowElement for FodIdCloudEngine {
    fn process(&self, data: &mut FlowData) -> Result<()> {
        let element = match self.build_data(data)? {
            Some(element) => element,
            None => {
                CloudRequestEngine::record_not_processed(data, ENGINE_NAME, FODID_ELEMENT_DATA_KEY);
                FodIdDataBase::new()
            }
        };
        data.get_or_add(FODID_DATA_KEY, || element)?;
        Ok(())
    }

    fn data_key(&self) -> &str {
        FODID_ELEMENT_DATA_KEY
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

impl AspectEngine for FodIdCloudEngine {
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
        // Trigger discovery so the reason reflects the live cloud metadata
        // rather than the pre-discovery defaults.
        self.ensure_metadata();
        let ctx = EngineMissingPropertyContext {
            element_data_key: self.data_key(),
            deployment: self.deployment(),
            data_source_tier: self.data_source_tier(),
            properties_loaded: self.has_loaded_properties(),
            properties: self.aspect_properties(),
        };
        missing_property_reason(property_name, &ctx)
    }
}

/// A fluent builder for [`FodIdCloudEngine`] instances.
///
/// The cloud request engine is required, as the engine reads both its JSON
/// response and its accessible properties.
pub struct FodIdCloudEngineBuilder {
    request_engine: Option<Arc<CloudRequestEngine>>,
}

impl FodIdCloudEngineBuilder {
    /// Create a builder with no request engine set.
    pub fn new() -> Self {
        FodIdCloudEngineBuilder {
            request_engine: None,
        }
    }

    /// Set the [`CloudRequestEngine`] this engine reads from (required). The same
    /// request engine instance must be added to the pipeline before the
    /// identifier cloud engine.
    pub fn cloud_request_engine(mut self, engine: Arc<CloudRequestEngine>) -> Self {
        self.request_engine = Some(engine);
        self
    }

    /// Build the engine, returning an [`Error::PipelineConfiguration`] if no
    /// cloud request engine was supplied.
    pub fn build(self) -> Result<FodIdCloudEngine> {
        let request_engine = self.request_engine.ok_or_else(|| {
            Error::configuration(
                "a FodIdCloudEngine requires a CloudRequestEngine; \
                 set one with .cloud_request_engine(..)",
            )
        })?;

        Ok(FodIdCloudEngine {
            request_engine,
            evidence_filter: EvidenceKeyFilterWhitelist::new(Vec::<String>::new()),
            properties: OnceCell::new(),
            aspect_properties: OnceCell::new(),
            default_properties: default_property_metadata(),
            default_aspect_properties: default_aspect_property_metadata(),
        })
    }
}

impl Default for FodIdCloudEngineBuilder {
    fn default() -> Self {
        FodIdCloudEngineBuilder::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::FodIdData;
    use fiftyone_cloud_request_engine::{
        CloudEngineState, CloudHttpClient, CloudHttpRequest, CloudHttpResponse,
    };

    /// A request engine wired to a stub HTTP client, so no network is touched.
    ///
    /// An empty state is supplied so the builder resolves without a discovery
    /// fetch (the stub would otherwise error). The FODid engine reports its six
    /// fixed identifier properties regardless of the cloud metadata, so an empty
    /// accessible-properties set is fine here.
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

    fn engine() -> FodIdCloudEngine {
        FodIdCloudEngine::builder()
            .cloud_request_engine(request_engine())
            .build()
            .unwrap()
    }

    #[test]
    fn build_requires_request_engine() {
        match FodIdCloudEngine::builder().build() {
            Err(Error::PipelineConfiguration { .. }) => {}
            Err(other) => panic!("expected a configuration error, got {other:?}"),
            Ok(_) => panic!("expected a configuration error without a request engine"),
        }
    }

    #[test]
    fn data_key_and_deployment_match_contract() {
        let engine = engine();
        assert_eq!(engine.data_key(), "fodid");
        assert_eq!(engine.deployment(), EngineDeployment::Cloud);
        assert_eq!(engine.data_source_tier(), "");
        // No evidence is read directly.
        assert!(!engine.evidence_key_filter().include("header.user-agent"));
    }

    #[test]
    fn default_metadata_is_reported_before_discovery() {
        // Discovery cannot reach the stub cloud, so the engine reports the six
        // default identifier properties.
        let engine = engine();
        assert_eq!(engine.properties().len(), 6);
        assert_eq!(engine.aspect_properties().len(), 6);
        assert!(!engine.has_loaded_properties());
    }

    #[test]
    fn metadata_is_derived_from_the_product_as_the_cloud_names_it() {
        // The cloud service reports the product as "FODid", which differs in
        // case from the "fodid" element data key.
        let products = LicensedProducts::parse(
            r#"{"Products":{"FODid":{"DataTier":"CloudV5Bespoke","Properties":[
                {"Name":"IdProbGlobal","Type":"String"},
                {"Name":"IdRandGlobal","Type":"String"}
            ]}}}"#,
        )
        .unwrap();
        let (core, aspect) = FodIdCloudEngine::derive_metadata(&products)
            .expect("the FODid product is found whatever its case");
        let names: Vec<&str> = core.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, ["IdProbGlobal", "IdRandGlobal"]);
        assert_eq!(aspect.len(), 2);

        // A product with no properties, or no identifier product at all,
        // still leaves the engine on its defaults.
        for json in [
            r#"{"Products":{"FODid":{"Properties":[]}}}"#,
            r#"{"Products":{"device":{"Properties":[{"Name":"IsMobile","Type":"Bool"}]}}}"#,
        ] {
            let products = LicensedProducts::parse(json).unwrap();
            assert!(FodIdCloudEngine::derive_metadata(&products).is_none());
        }
    }

    #[test]
    fn parses_a_representative_response() {
        let json = r#"{
            "fodid": {
                "idprobglobal": "AzUxZC5lcwBzGTMA",
                "idproblic": null,
                "idproblicnullreason": "The usage policy does not permit a licence identifier."
            }
        }"#;

        let data = engine().parse_json(json).unwrap();

        assert_eq!(data.id_prob_global().value().unwrap(), "AzUxZC5lcwBzGTMA");
        let lic = data.id_prob_lic();
        assert!(!lic.has_value());

        use fiftyone_pipeline_core::ElementData;
        assert_eq!(
            data.get("idproblicnullreason").unwrap().as_str(),
            Some("The usage policy does not permit a licence identifier.")
        );
    }

    #[test]
    fn absent_fodid_member_yields_empty_data() {
        let json = r#"{ "device": { "ismobile": true } }"#;
        let data = engine().parse_json(json).unwrap();
        assert!(!data.id_prob_global().has_value());
        assert!(!data.id_prob_lic().has_value());
    }

    #[test]
    fn empty_fodid_member_yields_empty_data() {
        let data = engine().parse_json(r#"{ "fodid": {} }"#).unwrap();
        assert!(!data.id_prob_global().has_value());
    }

    #[test]
    fn missing_cloud_data_is_a_configuration_error() {
        use fiftyone_pipeline_core::{Evidence, Pipeline};
        let pipeline = Pipeline::builder()
            .add_element(Arc::new(engine()))
            .suppress_process_exceptions(false)
            .build()
            .unwrap();
        let mut data = pipeline.create_flow_data_with(Evidence::builder().build());
        assert!(
            data.process().is_err(),
            "missing upstream cloud data must error"
        );
    }
}
