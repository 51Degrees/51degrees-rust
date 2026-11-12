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

//! Resolving why a property is missing from an engine's results.
//!
//! When a property an engine advertises is requested but is not present in its
//! results, the caller wants to know why. The [`MissingPropertyService`] applies
//! the rules and messages of the
//! [missing-properties section](https://github.com/51Degrees/specifications/blob/main/pipeline-specification/features/properties.md#missing-properties)
//! of the properties specification.
//!
//! The outcome is a core [`MissingPropertyReason`] plus a developer-facing
//! description. An aspect engine surfaces this through
//! [`crate::AspectEngine::missing_property_reason`].
//! [`crate::AspectEngine::property_missing_error`] builds an
//! [`Error::PropertyMissing`](fiftyone_pipeline_core::Error::PropertyMissing)
//! from the reason alone, so that error's text is the reason's general
//! wording rather than the description.
//! [`missing_property_reason_for_engines`] resolves the reason across every
//! engine that writes to one aspect data.
//!
//! An aspect data gives the reason for a property it does not hold through a
//! [`MissingPropertySource`] that the engine attached when it wrote the data.

use fiftyone_pipeline_core::MissingPropertyReason;

use crate::aspect_engine::AspectEngine;
use crate::aspect_property_metadata::AspectPropertyMetaData;

/// The pricing page link in the data file upgrade message.
const PRICING_URL: &str = "https://51degrees.com/pricing?utm_source=code&utm_medium=comment&utm_campaign=51degrees-rust&utm_content=pipeline-engines-src-missing_property.rs&utm_term=data-upgrade-required";

/// The configurator link in the message for a product the resource key lacks.
const CONFIGURATOR_PRODUCT_URL: &str = "https://51degrees.com/configure?utm_source=code&utm_medium=comment&utm_campaign=51degrees-rust&utm_content=pipeline-engines-src-missing_property.rs&utm_term=product-not-in-resource";

/// The configurator link in the message for a property the resource key lacks.
const CONFIGURATOR_PROPERTY_URL: &str = "https://51degrees.com/configure?utm_source=code&utm_medium=comment&utm_campaign=51degrees-rust&utm_content=pipeline-engines-src-missing_property.rs&utm_term=property-not-in-resource";

/// The description for [`MissingPropertyReason::PropertyExcludedFromConfig`].
const MESSAGE_PROPERTY_EXCLUDED: &str =
    "This is because the property has been excluded when configuring the engine.";

/// The description for [`MissingPropertyReason::Unknown`].
const MESSAGE_UNKNOWN: &str = "The reason for this is unknown. Please check that \
     the aspect and property name are correct.";

/// The description for [`MissingPropertyReason::CloudRequestFailed`].
const MESSAGE_CLOUD_REQUEST_FAILED: &str = "This is because an upstream cloud request \
     failed, so no data was available to populate the property.";

/// The element named in the message for a failed cloud request when the data
/// names no engine.
const UNKNOWN_ELEMENT: &str = "Unknown";

/// The deployment kind of an engine, used to pick the correct missing-property
/// rules.
///
/// On-premise engines reason about data tiers and data-file upgrades, whilst
/// cloud engines reason about resource-key access. The two share a code path
/// but branch on this flag.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EngineDeployment {
    /// An on-premise engine backed by a local data file.
    OnPremise,
    /// A cloud engine backed by a 51Degrees cloud resource key.
    Cloud,
}

/// The result of resolving a missing property: the reason plus a description.
///
/// The `description` is a developer-facing sentence suitable for inclusion in
/// an error message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MissingPropertyResult {
    /// The reason the property is missing.
    pub reason: MissingPropertyReason,
    /// A developer-facing description of the reason.
    pub description: String,
}

/// Inputs describing the engine that should have populated the property.
///
/// This is the minimal view of an engine the resolution rules need, decoupled
/// from the [`crate::AspectEngine`] trait so the service can be unit-tested in
/// isolation and so a cloud engine that has not yet loaded its metadata can
/// pass `properties_loaded = false`.
///
/// A context carries neither the engine's type name, its typed accessors, nor
/// whether its property list request failed. So a description built from one
/// names the element data key where the engine's type name would go, the
/// typed accessor rule never applies, and a cloud engine without metadata
/// gives [`MissingPropertyReason::Unknown`].
/// [`crate::AspectEngine::missing_property_reason`] applies every rule.
pub struct EngineMissingPropertyContext<'a> {
    /// The engine's string data key, used in the message.
    pub element_data_key: &'a str,
    /// The deployment kind of the engine.
    pub deployment: EngineDeployment,
    /// The data tier of the engine's current data source, for example `Lite`.
    /// Only consulted for on-premise engines.
    pub data_source_tier: &'a str,
    /// True if the engine has loaded its property metadata. A cloud engine may
    /// answer `false` before its first request, in which case the
    /// metadata-driven rules are skipped.
    pub properties_loaded: bool,
    /// The engine's aspect property metadata.
    pub properties: &'a [AspectPropertyMetaData],
}

/// Everything the rules read from one engine.
pub(crate) struct EngineView<'a> {
    /// The engine's string data key.
    pub(crate) element_data_key: &'a str,
    /// The engine's type name, written in the data file upgrade message.
    pub(crate) type_name: &'a str,
    /// The deployment kind of the engine.
    pub(crate) deployment: EngineDeployment,
    /// The data tier of the engine's current data source.
    pub(crate) data_source_tier: &'a str,
    /// True if the engine has loaded its property metadata.
    pub(crate) properties_loaded: bool,
    /// True if a cloud engine has no metadata because its request for the
    /// property list failed.
    pub(crate) property_list_request_failed: bool,
    /// The engine's aspect property metadata.
    pub(crate) properties: &'a [AspectPropertyMetaData],
    /// Whether the engine's data type has a typed accessor for a lowercased
    /// name, or [`None`] when the typed accessors are not known.
    pub(crate) has_typed_property: Option<&'a dyn Fn(&str) -> bool>,
}

impl<'a> EngineView<'a> {
    /// The view of a context, which has no type name or typed accessors.
    fn from_context(engine: &'a EngineMissingPropertyContext<'a>) -> Self {
        EngineView {
            element_data_key: engine.element_data_key,
            type_name: engine.element_data_key,
            deployment: engine.deployment,
            data_source_tier: engine.data_source_tier,
            properties_loaded: engine.properties_loaded,
            property_list_request_failed: false,
            properties: engine.properties,
            has_typed_property: None,
        }
    }
}

/// Determines why a property is not present in an engine's results.
///
/// This is a stateless resolver. Construct one with
/// [`MissingPropertyService::new`] or use the free functions
/// [`missing_property_reason`] and [`missing_property_reason_for_engines`].
#[derive(Debug, Default, Clone, Copy)]
pub struct MissingPropertyService;

impl MissingPropertyService {
    /// Create a new service.
    pub fn new() -> Self {
        MissingPropertyService
    }

    /// Resolve the reason `property_name` is missing for the supplied engine
    /// context, returning the reason and a developer-facing description.
    pub fn reason(
        &self,
        property_name: &str,
        engine: &EngineMissingPropertyContext<'_>,
    ) -> MissingPropertyResult {
        resolve(property_name, &EngineView::from_context(engine))
    }

    /// Resolve the reason `property_name` is missing across every engine that
    /// writes to one aspect data, in order.
    ///
    /// The first engine that gives a reason other than
    /// [`MissingPropertyReason::Unknown`] decides. When none does, the last
    /// engine's result is returned, and [`None`] when there are no engines.
    pub fn reason_for_engines(
        &self,
        property_name: &str,
        engines: &[&dyn AspectEngine],
    ) -> Option<MissingPropertyResult> {
        first_known_reason(
            engines
                .iter()
                .map(|engine| engine.missing_property_reason(property_name)),
        )
    }
}

/// The first result whose reason is not [`MissingPropertyReason::Unknown`],
/// otherwise the last result, and [`None`] when there are no results.
pub(crate) fn first_known_reason(
    results: impl IntoIterator<Item = MissingPropertyResult>,
) -> Option<MissingPropertyResult> {
    let mut last = None;
    for result in results {
        if result.reason != MissingPropertyReason::Unknown {
            return Some(result);
        }
        last = Some(result);
    }
    last
}

/// Resolve the missing-property reason for a single engine context.
///
/// A free-function shortcut over [`MissingPropertyService::reason`] for callers
/// that do not want to hold a service value.
pub fn missing_property_reason(
    property_name: &str,
    engine: &EngineMissingPropertyContext<'_>,
) -> MissingPropertyResult {
    MissingPropertyService::new().reason(property_name, engine)
}

/// Resolve the missing-property reason across several engines. A free-function
/// shortcut over [`MissingPropertyService::reason_for_engines`].
pub fn missing_property_reason_for_engines(
    property_name: &str,
    engines: &[&dyn AspectEngine],
) -> Option<MissingPropertyResult> {
    MissingPropertyService::new().reason_for_engines(property_name, engines)
}

/// What an aspect data needs from the engine that wrote it to say why a
/// property it does not hold is missing.
///
/// An engine builds one each time it loads its data and adds it to every
/// aspect data it writes with
/// [`AspectDataBase::add_missing_property_source`](crate::AspectDataBase::add_missing_property_source).
/// Its reasons are the ones [`AspectEngine::missing_property_reason`] gives
/// when the engine reports the same type name, tier, metadata and typed
/// accessors.
///
/// # Example
///
/// ```
/// use fiftyone_pipeline_core::PropertyValueType;
/// use fiftyone_pipeline_engines::{
///     AspectPropertyMetaData, EngineDeployment, MissingPropertyReason,
///     MissingPropertySource,
/// };
///
/// let source = MissingPropertySource::new(
///     "device",
///     "DeviceEngine",
///     EngineDeployment::OnPremise,
///     "Lite",
///     vec![AspectPropertyMetaData::new("IsMobile", "device", PropertyValueType::Bool)
///         .with_data_tiers(["Lite"])],
/// );
/// assert_eq!(
///     source.reason("NotAProperty").reason,
///     MissingPropertyReason::Unknown
/// );
/// ```
#[derive(Clone)]
pub struct MissingPropertySource {
    element_data_key: String,
    type_name: String,
    deployment: EngineDeployment,
    data_source_tier: String,
    properties: Vec<AspectPropertyMetaData>,
    has_typed_property: Option<fn(&str) -> bool>,
}

/// Written by hand so that printing data that holds a source prints the
/// number of properties rather than the engine's whole property metadata.
impl std::fmt::Debug for MissingPropertySource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MissingPropertySource")
            .field("element_data_key", &self.element_data_key)
            .field("type_name", &self.type_name)
            .field("deployment", &self.deployment)
            .field("data_source_tier", &self.data_source_tier)
            .field("properties", &self.properties.len())
            .field("has_typed_property", &self.has_typed_property.is_some())
            .finish()
    }
}

impl MissingPropertySource {
    /// The details of an engine whose property metadata is loaded, with no
    /// typed accessors.
    pub fn new(
        element_data_key: impl Into<String>,
        type_name: impl Into<String>,
        deployment: EngineDeployment,
        data_source_tier: impl Into<String>,
        properties: Vec<AspectPropertyMetaData>,
    ) -> Self {
        MissingPropertySource {
            element_data_key: element_data_key.into(),
            type_name: type_name.into(),
            deployment,
            data_source_tier: data_source_tier.into(),
            properties,
            has_typed_property: None,
        }
    }

    /// Use `has_typed_property` to say whether the engine's data type has a
    /// typed accessor for a name, as
    /// [`AspectEngine::has_typed_property`] does.
    pub fn with_typed_properties(mut self, has_typed_property: fn(&str) -> bool) -> Self {
        self.has_typed_property = Some(has_typed_property);
        self
    }

    /// Resolve why `property_name` is missing.
    pub fn reason(&self, property_name: &str) -> MissingPropertyResult {
        let view = EngineView {
            element_data_key: &self.element_data_key,
            type_name: &self.type_name,
            deployment: self.deployment,
            data_source_tier: &self.data_source_tier,
            properties_loaded: true,
            property_list_request_failed: false,
            properties: &self.properties,
            has_typed_property: self
                .has_typed_property
                .as_ref()
                .map(|check| check as &dyn Fn(&str) -> bool),
        };
        resolve(property_name, &view)
    }
}

/// The result for a property of data whose cloud request failed, naming the
/// element that should have populated it, or `Unknown` when there is none.
pub(crate) fn cloud_request_failed(
    property_name: &str,
    element_data_key: Option<&str>,
) -> MissingPropertyResult {
    let element = element_data_key.unwrap_or(UNKNOWN_ELEMENT);
    MissingPropertyResult {
        reason: MissingPropertyReason::CloudRequestFailed,
        description: format!(
            "{}{MESSAGE_CLOUD_REQUEST_FAILED}",
            description_prefix(property_name, element)
        ),
    }
}

/// The start of every description, naming the property and the element.
fn description_prefix(property_name: &str, element_data_key: &str) -> String {
    format!("Property '{property_name}' not found in data for element '{element_data_key}'. ")
}

/// Apply the rules to one engine and describe the outcome.
pub(crate) fn resolve(property_name: &str, engine: &EngineView<'_>) -> MissingPropertyResult {
    let (reason, property) = determine_reason(property_name, engine);
    let description = build_description(&reason, property_name, engine, property);
    MissingPropertyResult {
        reason,
        description,
    }
}

/// Apply the resolution rules, first match wins, returning the reason and the
/// engine's metadata for the property when it has some.
///
/// Names are compared once lowercased, as element data compares them, so a
/// property has one reason however it is spelt.
///
/// 1. When the engine has loaded its metadata and lists the property, an
///    on-premise engine whose data tier is not one of the property's tiers
///    gives [`MissingPropertyReason::DataFileUpgradeRequired`], and a property
///    marked unavailable gives
///    [`MissingPropertyReason::PropertyExcludedFromConfig`]. Cloud engines do
///    not publish tiers, so the tier check is skipped for them.
/// 2. A cloud engine that has loaded its metadata gives
///    [`MissingPropertyReason::ProductNotAccessibleWithResourceKey`] when it
///    lists no properties, and otherwise
///    [`MissingPropertyReason::PropertyNotAccessibleWithResourceKey`]. One
///    without metadata gives [`MissingPropertyReason::CloudRequestFailed`]
///    when its property list request failed, and otherwise
///    [`MissingPropertyReason::Unknown`].
/// 3. When an on-premise engine's metadata does not list the property and the
///    engine's data type has a typed accessor for the name, the property is
///    one another data file would supply, so the reason is
///    [`MissingPropertyReason::DataFileUpgradeRequired`] with no tiers. A
///    listed property has passed rule 1's tier check, so an upgrade would
///    name the tier the engine already has.
/// 4. Otherwise [`MissingPropertyReason::Unknown`].
fn determine_reason<'a>(
    property_name: &str,
    engine: &EngineView<'a>,
) -> (MissingPropertyReason, Option<&'a AspectPropertyMetaData>) {
    let is_cloud = engine.deployment == EngineDeployment::Cloud;
    let lowered = property_name.to_lowercase();

    let property = if engine.properties_loaded {
        engine
            .properties
            .iter()
            .find(|p| p.name().to_lowercase() == lowered)
    } else {
        None
    };

    if let Some(property) = property {
        if !is_cloud
            && !property
                .data_tiers_where_present()
                .iter()
                .any(|t| t == engine.data_source_tier)
        {
            return (
                MissingPropertyReason::DataFileUpgradeRequired,
                Some(property),
            );
        }
        if !property.available() {
            return (
                MissingPropertyReason::PropertyExcludedFromConfig,
                Some(property),
            );
        }
    }

    if is_cloud {
        let reason = if !engine.properties_loaded {
            if engine.property_list_request_failed {
                MissingPropertyReason::CloudRequestFailed
            } else {
                MissingPropertyReason::Unknown
            }
        } else if engine.properties.is_empty() {
            MissingPropertyReason::ProductNotAccessibleWithResourceKey
        } else {
            MissingPropertyReason::PropertyNotAccessibleWithResourceKey
        };
        return (reason, property);
    }

    if property.is_none()
        && engine
            .has_typed_property
            .is_some_and(|has_typed_property| has_typed_property(&lowered))
    {
        return (MissingPropertyReason::DataFileUpgradeRequired, None);
    }

    (MissingPropertyReason::Unknown, property)
}

/// Build the developer-facing description for a resolved reason, being the
/// prefix naming the property and element, then the reason's own sentence.
fn build_description(
    reason: &MissingPropertyReason,
    property_name: &str,
    engine: &EngineView<'_>,
    property: Option<&AspectPropertyMetaData>,
) -> String {
    let element = engine.element_data_key;
    let prefix = description_prefix(property_name, element);

    let detail = match reason {
        MissingPropertyReason::DataFileUpgradeRequired => {
            let tiers = property
                .map(AspectPropertyMetaData::data_tiers_where_present)
                .filter(|tiers| !tiers.is_empty());
            match tiers {
                Some(tiers) => format!(
                    "This is because your license or data file does not include this \
                     property. It is available with the {} license/data for the {}. \
                     To upgrade, see {PRICING_URL}.",
                    tiers.join(","),
                    engine.type_name
                ),
                None => format!(
                    "This is because your license or data file does not include this \
                     property. To see which licenses include it, see {PRICING_URL}."
                ),
            }
        }
        MissingPropertyReason::PropertyExcludedFromConfig => MESSAGE_PROPERTY_EXCLUDED.to_owned(),
        MissingPropertyReason::ProductNotAccessibleWithResourceKey => format!(
            "This is because your resource key does not include access to any \
             properties under '{element}'. Check or create a resource key with the \
             properties you need in the configurator at {CONFIGURATOR_PRODUCT_URL}."
        ),
        MissingPropertyReason::PropertyNotAccessibleWithResourceKey => {
            let available = engine
                .properties
                .iter()
                .map(|p| p.name())
                .collect::<Vec<_>>()
                .join(",");
            format!(
                "This is because your resource key does not include access to this \
                 property. The properties included for your key under '{element}' are \
                 {available}. Check or change your key in the configurator at \
                 {CONFIGURATOR_PROPERTY_URL}."
            )
        }
        MissingPropertyReason::Unknown => MESSAGE_UNKNOWN.to_owned(),
        MissingPropertyReason::CloudRequestFailed => MESSAGE_CLOUD_REQUEST_FAILED.to_owned(),
        // A reason added later has no sentence of its own yet, so the
        // description is the prefix alone.
        _ => String::new(),
    };

    prefix + &detail
}

#[cfg(test)]
mod tests {
    use super::*;
    use fiftyone_pipeline_core::PropertyValueType;

    fn prop(name: &str) -> AspectPropertyMetaData {
        AspectPropertyMetaData::new(name, "device", PropertyValueType::String)
    }

    fn on_premise(properties: &[AspectPropertyMetaData]) -> EngineView<'_> {
        EngineView {
            element_data_key: "device",
            type_name: "TestEngine",
            deployment: EngineDeployment::OnPremise,
            data_source_tier: "Lite",
            properties_loaded: true,
            property_list_request_failed: false,
            properties,
            has_typed_property: None,
        }
    }

    fn cloud(properties: &[AspectPropertyMetaData]) -> EngineView<'_> {
        EngineView {
            deployment: EngineDeployment::Cloud,
            data_source_tier: "",
            ..on_premise(properties)
        }
    }

    const PREFIX: &str = "Property 'IsMobile' not found in data for element 'device'. ";

    #[test]
    fn on_premise_tier_mismatch_is_upgrade_required() {
        let props = vec![prop("IsMobile").with_data_tiers(["Premium", "Enterprise"])];
        let result = resolve("IsMobile", &on_premise(&props));
        assert_eq!(
            result.reason,
            MissingPropertyReason::DataFileUpgradeRequired
        );
        assert_eq!(
            result.description,
            format!(
                "{PREFIX}This is because your license or data file does not include \
                 this property. It is available with the Premium,Enterprise \
                 license/data for the TestEngine. To upgrade, see {PRICING_URL}."
            )
        );
    }

    #[test]
    fn a_failed_cloud_request_reads_as_dotnet_says() {
        let result = cloud_request_failed("IsMobile", Some("device"));
        assert_eq!(result.reason, MissingPropertyReason::CloudRequestFailed);
        assert_eq!(
            result.description,
            format!(
                "{PREFIX}This is because an upstream cloud request failed, so no data was \
                 available to populate the property."
            )
        );
        assert_eq!(
            cloud_request_failed("IsMobile", None).description,
            "Property 'IsMobile' not found in data for element 'Unknown'. This is because \
             an upstream cloud request failed, so no data was available to populate the \
             property."
        );
    }

    /// The upgrade message for a property whose tiers are not known.
    fn upgrade_without_tiers(property_name: &str) -> String {
        format!(
            "Property '{property_name}' not found in data for element 'device'. This is \
             because your license or data file does not include this property. To see \
             which licenses include it, see {PRICING_URL}."
        )
    }

    #[test]
    fn a_listed_property_with_no_tiers_leaves_out_the_tier_sentence() {
        let props = vec![prop("IsMobile")];
        let result = resolve("IsMobile", &on_premise(&props));
        assert_eq!(
            result.reason,
            MissingPropertyReason::DataFileUpgradeRequired
        );
        assert_eq!(result.description, upgrade_without_tiers("IsMobile"));
    }

    #[test]
    fn on_premise_tier_present_but_unavailable_is_excluded() {
        let props = vec![prop("IsMobile")
            .with_data_tiers(["Lite"])
            .map_core(|c| c.with_available(false))];
        let result = resolve("IsMobile", &on_premise(&props));
        assert_eq!(
            result.reason,
            MissingPropertyReason::PropertyExcludedFromConfig
        );
        assert_eq!(
            result.description,
            format!(
                "{PREFIX}This is because the property has been excluded when \
                 configuring the engine."
            )
        );
    }

    #[test]
    fn names_are_matched_ignoring_case() {
        let props = vec![
            prop("IsMobile")
                .with_data_tiers(["Lite"])
                .map_core(|c| c.with_available(false)),
            prop("ÄrgerLevel")
                .with_data_tiers(["Lite"])
                .map_core(|c| c.with_available(false)),
        ];
        for name in [
            "IsMobile",
            "ismobile",
            "ISMOBILE",
            "ärgerlevel",
            "ÄRGERLEVEL",
        ] {
            assert_eq!(
                resolve(name, &on_premise(&props)).reason,
                MissingPropertyReason::PropertyExcludedFromConfig,
                "{name}"
            );
            assert_eq!(
                resolve(name, &cloud(&props)).reason,
                MissingPropertyReason::PropertyExcludedFromConfig,
                "{name}"
            );
        }
        let result = resolve("ismobile", &on_premise(&props));
        assert!(
            result
                .description
                .starts_with("Property 'ismobile' not found"),
            "the message keeps the caller's spelling: {}",
            result.description
        );
    }

    #[test]
    fn a_typed_accessor_means_upgrade_required() {
        let props = vec![prop("ScreenWidth").with_data_tiers(["Lite"])];
        // The rules pass the lowercased name, so this accessor check need not
        // ignore case itself.
        let typed = |name: &str| name == "ismobile";
        let view = EngineView {
            has_typed_property: Some(&typed),
            ..on_premise(&props)
        };
        let result = resolve("isMobile", &view);
        assert_eq!(
            result.reason,
            MissingPropertyReason::DataFileUpgradeRequired
        );
        assert_eq!(result.description, upgrade_without_tiers("isMobile"));
        assert_eq!(
            resolve("NotAProperty", &view).reason,
            MissingPropertyReason::Unknown
        );
    }

    #[test]
    fn a_listed_available_property_with_its_tier_is_unknown() {
        let props = vec![prop("IsMobile").with_data_tiers(["Lite"])];
        let typed = |_: &str| true;
        let view = EngineView {
            has_typed_property: Some(&typed),
            ..on_premise(&props)
        };
        // The typed accessor rule is only for a property the metadata does
        // not list, so an upgrade never names the engine's own tier.
        let result = resolve("IsMobile", &view);
        assert_eq!(result.reason, MissingPropertyReason::Unknown);
        assert!(!result.description.contains("license/data"));
    }

    #[test]
    fn cloud_with_no_properties_is_product_not_accessible() {
        let result = resolve("IsMobile", &cloud(&[]));
        assert_eq!(
            result.reason,
            MissingPropertyReason::ProductNotAccessibleWithResourceKey
        );
        assert_eq!(
            result.description,
            format!(
                "{PREFIX}This is because your resource key does not include access to \
                 any properties under 'device'. Check or create a resource key with \
                 the properties you need in the configurator at \
                 {CONFIGURATOR_PRODUCT_URL}."
            )
        );
    }

    #[test]
    fn cloud_with_other_properties_is_property_not_accessible() {
        let props = vec![prop("ScreenWidth"), prop("ScreenHeight")];
        let result = resolve("IsMobile", &cloud(&props));
        assert_eq!(
            result.reason,
            MissingPropertyReason::PropertyNotAccessibleWithResourceKey
        );
        assert_eq!(
            result.description,
            format!(
                "{PREFIX}This is because your resource key does not include access to \
                 this property. The properties included for your key under 'device' \
                 are ScreenWidth,ScreenHeight. Check or change your key in the \
                 configurator at {CONFIGURATOR_PROPERTY_URL}."
            )
        );
    }

    #[test]
    fn cloud_skips_the_tier_check() {
        let props = vec![prop("IsMobile").map_core(|c| c.with_available(false))];
        let result = resolve("IsMobile", &cloud(&props));
        assert_eq!(
            result.reason,
            MissingPropertyReason::PropertyExcludedFromConfig
        );
    }

    #[test]
    fn unloaded_properties_are_unknown() {
        let props = vec![prop("IsMobile")];
        let view = EngineView {
            properties_loaded: false,
            ..cloud(&props)
        };
        let result = resolve("IsMobile", &view);
        assert_eq!(result.reason, MissingPropertyReason::Unknown);
        assert_eq!(
            result.description,
            format!(
                "{PREFIX}The reason for this is unknown. Please check that the aspect \
                 and property name are correct."
            )
        );
    }

    #[test]
    fn a_cloud_engine_without_metadata_ignores_its_typed_accessors() {
        let typed = |_: &str| true;
        let view = EngineView {
            properties_loaded: false,
            has_typed_property: Some(&typed),
            ..cloud(&[])
        };
        assert_eq!(
            resolve("IsMobile", &view).reason,
            MissingPropertyReason::Unknown
        );
    }

    #[test]
    fn a_cloud_engine_whose_property_list_request_failed_says_so() {
        let props = vec![prop("IsMobile")];
        let view = EngineView {
            properties_loaded: false,
            property_list_request_failed: true,
            ..cloud(&props)
        };
        assert_eq!(
            resolve("IsMobile", &view),
            cloud_request_failed("IsMobile", Some("device"))
        );
        assert_eq!(
            resolve("NotAProperty", &view).reason,
            MissingPropertyReason::CloudRequestFailed
        );
        // Metadata that has loaded decides, whatever the request did earlier.
        let loaded = EngineView {
            properties_loaded: true,
            ..view
        };
        assert_eq!(
            resolve("ScreenWidth", &loaded).reason,
            MissingPropertyReason::PropertyNotAccessibleWithResourceKey
        );
    }

    #[test]
    fn an_on_premise_engine_never_blames_the_property_list_request() {
        let view = EngineView {
            properties_loaded: false,
            property_list_request_failed: true,
            ..on_premise(&[])
        };
        assert_eq!(
            resolve("IsMobile", &view).reason,
            MissingPropertyReason::Unknown
        );
    }

    #[test]
    fn a_context_names_the_element_where_the_type_would_go() {
        let props = vec![prop("IsMobile").with_data_tiers(["Premium"])];
        let ctx = EngineMissingPropertyContext {
            element_data_key: "device",
            deployment: EngineDeployment::OnPremise,
            data_source_tier: "Lite",
            properties_loaded: true,
            properties: &props,
        };
        let result = missing_property_reason("IsMobile", &ctx);
        assert_eq!(
            result.reason,
            MissingPropertyReason::DataFileUpgradeRequired
        );
        assert!(result
            .description
            .contains("It is available with the Premium license/data for the device."));
        // A context has no typed accessors, so a name the metadata does not
        // list is unknown.
        assert_eq!(
            missing_property_reason("ScreenWidth", &ctx).reason,
            MissingPropertyReason::Unknown
        );
    }

    fn typed_is_mobile(name: &str) -> bool {
        name.eq_ignore_ascii_case("IsMobile")
    }

    fn source(properties: Vec<AspectPropertyMetaData>) -> MissingPropertySource {
        MissingPropertySource::new(
            "device",
            "TestEngine",
            EngineDeployment::OnPremise,
            "Lite",
            properties,
        )
    }

    #[test]
    fn a_source_gives_the_reasons_of_the_engine_it_describes() {
        let props = vec![
            prop("IsMobile").with_data_tiers(["Premium"]),
            prop("BrowserName")
                .with_data_tiers(["Lite"])
                .map_core(|c| c.with_available(false)),
        ];
        let typed = |name: &str| typed_is_mobile(name);
        let view = EngineView {
            has_typed_property: Some(&typed),
            ..on_premise(&props)
        };
        let source = source(props.clone()).with_typed_properties(typed_is_mobile);
        for name in ["IsMobile", "BrowserName", "ismobile", "NotAProperty"] {
            assert_eq!(source.reason(name), resolve(name, &view), "{name}");
        }
        assert_eq!(
            source.reason("BrowserName").reason,
            MissingPropertyReason::PropertyExcludedFromConfig
        );
    }

    #[test]
    fn a_source_uses_its_typed_accessors_only_when_given_them() {
        assert_eq!(
            source(Vec::new()).reason("IsMobile").reason,
            MissingPropertyReason::Unknown
        );
        let result = source(Vec::new())
            .with_typed_properties(typed_is_mobile)
            .reason("IsMobile");
        assert_eq!(
            result.reason,
            MissingPropertyReason::DataFileUpgradeRequired
        );
        assert_eq!(result.description, upgrade_without_tiers("IsMobile"));
    }

    fn result(reason: MissingPropertyReason, description: &str) -> MissingPropertyResult {
        MissingPropertyResult {
            reason,
            description: description.to_owned(),
        }
    }

    #[test]
    fn the_first_known_reason_decides() {
        let unknown = result(MissingPropertyReason::Unknown, "first");
        let excluded = result(MissingPropertyReason::PropertyExcludedFromConfig, "second");
        let upgrade = result(MissingPropertyReason::DataFileUpgradeRequired, "third");
        assert_eq!(
            first_known_reason([unknown.clone(), excluded.clone(), upgrade]),
            Some(excluded)
        );
        let last = result(MissingPropertyReason::Unknown, "last");
        assert_eq!(first_known_reason([unknown, last.clone()]), Some(last));
        assert_eq!(first_known_reason(Vec::new()), None);
    }
}
