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

//! The robots.txt engine and its builder.

use std::collections::HashSet;
use std::ffi::{c_int, CStr};
use std::sync::{Arc, OnceLock, Weak};

use fiftyone_device_detection_onpremise::DeviceDetectionOnPremiseEngine;
use fiftyone_native::dd::{Manager, Results};
use fiftyone_pipeline_core::{
    Error, Evidence, EvidenceKeyFilter, EvidenceKeyFilterWhitelist, FlowData, FlowElement,
    Pipeline, PropertyMetaData, PropertyValueType, Result,
};
use fiftyone_pipeline_engines::{AspectEngine, AspectPropertyMetaData};
use url::Url;

use crate::constants::{
    ALLOW_VALUES, CRAWLER_NAME, CRAWLER_PRODUCT_TOKENS, CRAWLER_URL, CRAWLER_USAGE,
    DISALLOW_VALUES, ELEMENT_DATA_KEY, PROPERTY_ANNOTATED_TEXT, PROPERTY_CATEGORY,
    PROPERTY_PLAIN_TEXT, TDL_EVIDENCE_KEY, USAGE_EVIDENCE_KEY_PREFIX,
};
use crate::data::{RobotsTxtData, ROBOTS_TXT_DATA_KEY};
use crate::model::{CrawlerModel, RobotsTxtModel, UsageModel};
use crate::tdl::TdlSourceResolver;

/// The separator the native reader joins a list's values with.
const VALUE_SEPARATOR: &CStr = c"|";

/// The User-Agent the default TDL source resolver sends to GitHub.
#[cfg(feature = "reqwest-client")]
const DEFAULT_USER_AGENT: &str = concat!(
    "fiftyone-device-detection-robots-txt/",
    env!("CARGO_PKG_VERSION")
);

/// Writes a robots.txt that refuses each crawler the request does not allow,
/// from the crawlers the Device Detection data file describes, as .NET's
/// `RobotsTxtEngine` does.
///
/// Add it after a [`DeviceDetectionOnPremiseEngine`], whose data file must
/// hold the crawler properties, as the Enterprise file does. When the
/// pipeline is built, the engine reads every crawler and every usage, being
/// what a crawler uses the pages it fetches for, such as search or training a
/// model. A refresh of the data file does not read them again.
///
/// For each usage the engine accepts the evidence
/// `query.robotstxt.<usage in lower case>`, which allows the usage with
/// `allow`, `true`, `yes`, `enabled` or `on`, and refuses it with
/// `disallow`, `false`, `no`, `disabled` or `off`, ignoring case. Refusing
/// wins. A crawler is allowed when it makes an allowed usage, and a crawler
/// the data records no usage for is allowed when the usage `N/A` is.
///
/// The evidence `query.robotstxt.tdl` gives Terms Document Locator URIs,
/// separated by `,` or `|`, which the text names in its closing
/// `User-Agent: *` group. An entry that is not an absolute URI is given to
/// the [`TdlSourceResolver`], so `MOW-SOCW` names the latest Standard Online
/// Content Wording, and is left out if the resolver does not know it.
///
/// The engine writes [`RobotsTxtData`] only when the request carries one of
/// these evidence keys.
pub struct RobotsTxtEngine {
    /// The properties asked for, or none for both.
    wanted: Vec<String>,
    resolver: Option<Arc<dyn TdlSourceResolver>>,
    state: OnceLock<State>,
    /// The filter until the pipeline is known, which accepts nothing.
    no_evidence: EvidenceKeyFilterWhitelist,
}

/// What the engine reads from the Device Detection engine once it is added
/// to a pipeline.
struct State {
    /// The Device Detection engine the model was read from.
    source: Weak<DeviceDetectionOnPremiseEngine>,
    tier: String,
    model: Arc<RobotsTxtModel>,
    /// Each usage's evidence key, in lower case, and the usage's name.
    usage_keys: Vec<(String, String)>,
    filter: EvidenceKeyFilterWhitelist,
    properties: Vec<PropertyMetaData>,
    aspect_properties: Vec<AspectPropertyMetaData>,
}

impl RobotsTxtEngine {
    /// A builder for the engine.
    pub fn builder() -> RobotsTxtEngineBuilder {
        RobotsTxtEngineBuilder::new()
    }

    /// The crawlers and usages read from the data file, or [`None`] until the
    /// engine is added to a pipeline. A person choosing which usages to allow
    /// can be shown [`RobotsTxtModel::usages`].
    pub fn model(&self) -> Option<Arc<RobotsTxtModel>> {
        self.state.get().map(|state| Arc::clone(&state.model))
    }

    /// The evidence keys of the usages, in lower case, in the order of the
    /// usages' names, or none until the engine is added to a pipeline.
    pub fn usage_evidence_keys(&self) -> Vec<String> {
        self.state
            .get()
            .map(|state| {
                state
                    .usage_keys
                    .iter()
                    .map(|(key, _)| key.clone())
                    .collect()
            })
            .unwrap_or_default()
    }

    /// The usages the evidence allows, leaving out any it also refuses.
    fn allowed(state: &State, evidence: &Evidence) -> HashSet<String> {
        let mut allowed = HashSet::new();
        let mut refused = HashSet::new();
        for (key, name) in &state.usage_keys {
            let Some(value) = evidence.get(key) else {
                continue;
            };
            if ALLOW_VALUES
                .iter()
                .any(|word| word.eq_ignore_ascii_case(value))
            {
                allowed.insert(name.clone());
            }
            if DISALLOW_VALUES
                .iter()
                .any(|word| word.eq_ignore_ascii_case(value))
            {
                refused.insert(name.clone());
            }
        }
        allowed.retain(|name| !refused.contains(name));
        allowed
    }

    /// The TDL URIs the evidence gives, in order. Each entry is taken as an
    /// absolute URI, or else given to the resolver, and left out if neither
    /// gives a URI.
    fn tdls(&self, raw: Option<&str>) -> Vec<Url> {
        let Some(raw) = raw else {
            return Vec::new();
        };
        raw.split([',', '|'])
            .map(str::trim)
            .filter(|entry| !entry.is_empty())
            .filter_map(|entry| self.resolve_tdl(entry))
            .collect()
    }

    fn resolve_tdl(&self, entry: &str) -> Option<Url> {
        if let Ok(url) = Url::parse(entry) {
            return Some(url);
        }
        let resolver = self.resolver.as_ref()?;
        if resolver.is_known(entry) {
            resolver.resolve(entry)
        } else {
            None
        }
    }
}

impl State {
    fn new(source: &Arc<DeviceDetectionOnPremiseEngine>, wanted: &[String]) -> Result<Self> {
        let tier = AspectEngine::data_source_tier(source.as_ref()).to_owned();
        let model = read_model(&source.data_set())?;
        let usage_keys: Vec<(String, String)> = model
            .usages()
            .iter()
            .filter(|usage| !usage.name.is_empty())
            .map(|usage| {
                (
                    format!("{USAGE_EVIDENCE_KEY_PREFIX}{}", usage.name.to_lowercase()),
                    usage.name.clone(),
                )
            })
            .collect();
        let filter = EvidenceKeyFilterWhitelist::new(
            usage_keys
                .iter()
                .map(|(key, _)| key.as_str())
                .chain([TDL_EVIDENCE_KEY]),
        );
        let aspect_properties: Vec<AspectPropertyMetaData> =
            [PROPERTY_ANNOTATED_TEXT, PROPERTY_PLAIN_TEXT]
                .into_iter()
                .filter(|name| {
                    wanted.is_empty() || wanted.iter().any(|want| want.eq_ignore_ascii_case(name))
                })
                .map(|name| {
                    AspectPropertyMetaData::new(name, ELEMENT_DATA_KEY, PropertyValueType::String)
                        .with_data_tiers([tier.as_str()])
                        .map_core(|core| core.with_category(PROPERTY_CATEGORY))
                })
                .collect();
        let properties = aspect_properties
            .iter()
            .map(|property| property.core().clone())
            .collect();
        Ok(State {
            source: Arc::downgrade(source),
            tier,
            model: Arc::new(model),
            usage_keys,
            filter,
            properties,
            aspect_properties,
        })
    }

    fn is_required(&self, name: &str) -> bool {
        self.properties
            .iter()
            .any(|property| property.name.eq_ignore_ascii_case(name))
    }
}

/// The data file indexes of the crawler properties, or [`None`] for one the
/// data set was opened without.
struct CrawlerIndexes {
    name: Option<c_int>,
    product_tokens: Option<c_int>,
    url: Option<c_int>,
    usages: Option<c_int>,
}

/// Read every crawler and usage from the data set. A data file without the
/// `CrawlerUsage` property gives an empty model. A crawler whose values
/// cannot be read is left out, with a warning in the log.
fn read_model(manager: &Arc<Manager>) -> Result<RobotsTxtModel> {
    let properties = manager.data_file_properties()?;
    let find = |name: &str| {
        properties
            .iter()
            .find(|property| property.name.eq_ignore_ascii_case(name))
    };
    let Some(usage) = find(CRAWLER_USAGE) else {
        return Ok(RobotsTxtModel::default());
    };
    let usages: Vec<UsageModel> = manager
        .data_file_property_values(usage.index)?
        .into_iter()
        .map(|value| UsageModel::new(value.name, value.description))
        .collect();
    let index = |name: &str| find(name).and_then(|property| property.required_property_index);
    let indexes = CrawlerIndexes {
        name: index(CRAWLER_NAME),
        product_tokens: index(CRAWLER_PRODUCT_TOKENS),
        url: index(CRAWLER_URL),
        usages: usage.required_property_index,
    };
    let mut results = manager.create_results()?;
    let mut crawlers = Vec::new();
    for profile in manager
        .data_file_profiles()?
        .into_iter()
        .filter(|profile| profile.component_id == usage.component_id)
    {
        match read_crawler(&mut results, profile.profile_id, &indexes) {
            Ok(crawler) => crawlers.push(crawler),
            Err(reason) => log::warn!(
                "Could not include crawler for profile '{}' in robots.txt model: {reason}",
                profile.profile_id
            ),
        }
    }
    Ok(RobotsTxtModel::new(crawlers, usages))
}

/// The crawler the profile describes. Its name, product tokens and usages
/// must have values. Its address is kept only when it is an absolute URI.
fn read_crawler(
    results: &mut Results,
    profile_id: u32,
    indexes: &CrawlerIndexes,
) -> std::result::Result<CrawlerModel, String> {
    let found = results
        .process_device_id(&profile_id.to_string())
        .map_err(|error| error.to_string())?;
    if found == 0 {
        return Err("the profile was not found".to_owned());
    }
    let name = required(results, indexes.name, CRAWLER_NAME)?;
    let product_tokens = list(&required(
        results,
        indexes.product_tokens,
        CRAWLER_PRODUCT_TOKENS,
    )?);
    let usages = list(&required(results, indexes.usages, CRAWLER_USAGE)?);
    let reference_url = read(results, indexes.url, CRAWLER_URL)?
        .filter(|url| !url.trim().is_empty())
        .and_then(|url| Url::parse(&url).ok());
    Ok(CrawlerModel::new(
        name,
        usages,
        product_tokens,
        reference_url,
    ))
}

/// The value of a property the data set was opened with, or [`None`] when
/// the profile has none.
fn read(
    results: &Results,
    index: Option<c_int>,
    name: &str,
) -> std::result::Result<Option<String>, String> {
    let Some(index) = index else {
        return Err(format!("the data set was opened without {name}"));
    };
    results
        .value_as_string_by_index(index, VALUE_SEPARATOR)
        .map_err(|error| error.to_string())
}

fn required(
    results: &Results,
    index: Option<c_int>,
    name: &str,
) -> std::result::Result<String, String> {
    read(results, index, name)?.ok_or_else(|| format!("{name} has no value"))
}

/// The values of a list property, which the native reader joins with the
/// separator, as the Device Detection engine splits them.
fn list(joined: &str) -> Vec<String> {
    joined
        .split('|')
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .collect()
}

/// The [`DeviceDetectionOnPremiseEngine`] among the pipeline's elements.
fn device_detection_engine(pipeline: &Pipeline) -> Result<Arc<DeviceDetectionOnPremiseEngine>> {
    pipeline
        .flow_elements()
        .iter()
        .find_map(|element| {
            Arc::clone(element)
                .into_any()?
                .downcast::<DeviceDetectionOnPremiseEngine>()
                .ok()
        })
        .ok_or_else(|| {
            Error::configuration(
                "Ensure DeviceDetectionOnPremiseEngine is added to the pipeline before the \
                 robots.txt engine.",
            )
        })
}

impl FlowElement for RobotsTxtEngine {
    /// Write the text for the request, if it carries any of the engine's
    /// evidence keys.
    fn process(&self, data: &mut FlowData) -> Result<()> {
        let Some(state) = self.state.get() else {
            return Err(Error::configuration(
                "The robots.txt engine must be added to a pipeline before processing data.",
            ));
        };
        let evidence = data.evidence();
        let tdl = evidence.get(TDL_EVIDENCE_KEY);
        if tdl.is_none()
            && !state
                .usage_keys
                .iter()
                .any(|(key, _)| evidence.get(key).is_some())
        {
            return Ok(());
        }
        let allowed = Self::allowed(state, evidence);
        let tdls = self.tdls(tdl);
        let texts: Vec<(&str, String)> = [
            (PROPERTY_PLAIN_TEXT, false),
            (PROPERTY_ANNOTATED_TEXT, true),
        ]
        .into_iter()
        .filter(|(name, _)| state.is_required(name))
        .map(|(name, annotations)| (name, state.model.write(&allowed, &tdls, annotations)))
        .collect();
        let output = data.get_or_add(ROBOTS_TXT_DATA_KEY, RobotsTxtData::new)?;
        for (name, text) in texts {
            output.insert(name, text);
        }
        Ok(())
    }

    fn data_key(&self) -> &str {
        ELEMENT_DATA_KEY
    }

    fn evidence_key_filter(&self) -> &dyn EvidenceKeyFilter {
        match self.state.get() {
            Some(state) => &state.filter,
            None => &self.no_evidence,
        }
    }

    fn properties(&self) -> &[PropertyMetaData] {
        match self.state.get() {
            Some(state) => &state.properties,
            None => &[],
        }
    }

    fn properties_loaded(&self) -> bool {
        self.state.get().is_some()
    }

    /// Read the crawlers and usages from the pipeline's
    /// [`DeviceDetectionOnPremiseEngine`]. A pipeline without one is refused.
    ///
    /// The engine keeps what it read from the first pipeline, so it can be
    /// added to another pipeline only when that pipeline has the same Device
    /// Detection engine.
    fn add_pipeline(&self, pipeline: &Arc<Pipeline>) -> Result<()> {
        let source = device_detection_engine(pipeline)?;
        if self.state.get().is_none() {
            let state = State::new(&source, &self.wanted)?;
            // Another pipeline may have set it first, which the check below
            // compares with.
            let _ = self.state.set(state);
        }
        let same = self
            .state
            .get()
            .is_some_and(|state| Weak::ptr_eq(&state.source, &Arc::downgrade(&source)));
        if same {
            Ok(())
        } else {
            Err(Error::configuration(
                "The robots.txt engine is already in a pipeline with another Device \
                 Detection engine, whose crawlers it read. Make another robots.txt engine \
                 for this pipeline.",
            ))
        }
    }
}

impl AspectEngine for RobotsTxtEngine {
    fn data_source_tier(&self) -> &str {
        self.state.get().map_or("", |state| state.tier.as_str())
    }

    fn aspect_properties(&self) -> &[AspectPropertyMetaData] {
        match self.state.get() {
            Some(state) => &state.aspect_properties,
            None => &[],
        }
    }

    fn has_loaded_properties(&self) -> bool {
        self.state.get().is_some()
    }

    fn has_typed_property(&self, property_name: &str) -> bool {
        [PROPERTY_ANNOTATED_TEXT, PROPERTY_PLAIN_TEXT]
            .iter()
            .any(|name| name.eq_ignore_ascii_case(property_name))
    }
}

/// How the builder chooses the TDL source resolver.
enum ResolverChoice {
    /// The GitHub resolver over the compiled in sources when the
    /// `reqwest-client` feature is on, and none otherwise.
    Default,
    /// The resolver given, or none.
    Given(Option<Arc<dyn TdlSourceResolver>>),
    /// The GitHub resolver, sending this User-Agent.
    #[cfg(feature = "reqwest-client")]
    UserAgent(String),
}

/// Builds a [`RobotsTxtEngine`].
pub struct RobotsTxtEngineBuilder {
    properties: Vec<String>,
    resolver: ResolverChoice,
}

impl Default for RobotsTxtEngineBuilder {
    fn default() -> Self {
        RobotsTxtEngineBuilder::new()
    }
}

impl RobotsTxtEngineBuilder {
    /// A builder giving both properties, with the default TDL source
    /// resolver.
    pub fn new() -> Self {
        RobotsTxtEngineBuilder {
            properties: Vec::new(),
            resolver: ResolverChoice::Default,
        }
    }

    /// Give the property named, `PlainText` or `AnnotatedText`, matched
    /// ignoring case. With none named, the engine gives both.
    pub fn property(mut self, name: impl Into<String>) -> Self {
        self.properties.push(name.into());
        self
    }

    /// Give each of the properties named. See [`Self::property`].
    pub fn properties<I, S>(mut self, names: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.properties.extend(names.into_iter().map(Into::into));
        self
    }

    /// Resolve TDL source ids with `resolver`, or with [`None`] accept only
    /// absolute URIs.
    ///
    /// By default, with the `reqwest-client` feature, the engine resolves
    /// the sources compiled into this crate by listing their GitHub folders.
    /// Without the feature it accepts only absolute URIs until it is given a
    /// resolver.
    pub fn set_tdl_source_resolver(mut self, resolver: Option<Arc<dyn TdlSourceResolver>>) -> Self {
        self.resolver = ResolverChoice::Given(resolver);
        self
    }

    /// Resolve TDL source ids through GitHub as the default does, sending
    /// `user_agent`, which should name the application, as the `User-Agent`
    /// header.
    ///
    /// Compiled only with the `reqwest-client` feature.
    #[cfg(feature = "reqwest-client")]
    pub fn set_user_agent(mut self, user_agent: impl Into<String>) -> Self {
        self.resolver = ResolverChoice::UserAgent(user_agent.into());
        self
    }

    /// Build the engine. It reads the crawlers when it is added to a
    /// pipeline.
    pub fn build(self) -> Result<Arc<RobotsTxtEngine>> {
        let resolver: Option<Arc<dyn TdlSourceResolver>> = match self.resolver {
            ResolverChoice::Given(resolver) => resolver,
            #[cfg(feature = "reqwest-client")]
            ResolverChoice::Default => Some(Arc::new(
                crate::github::GitHubTdlSourceResolver::create_default(DEFAULT_USER_AGENT)?,
            )),
            #[cfg(not(feature = "reqwest-client"))]
            ResolverChoice::Default => None,
            #[cfg(feature = "reqwest-client")]
            ResolverChoice::UserAgent(user_agent) => Some(Arc::new(
                crate::github::GitHubTdlSourceResolver::create_default(user_agent)?,
            )),
        };
        Ok(Arc::new(RobotsTxtEngine {
            wanted: self.properties,
            resolver,
            state: OnceLock::new(),
            no_evidence: EvidenceKeyFilterWhitelist::default(),
        }))
    }
}
