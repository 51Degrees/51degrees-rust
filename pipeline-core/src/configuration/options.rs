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

//! The description of a pipeline that a configuration gives, in the shape
//! of .NET's `PipelineOptions` and `ElementOptions`.

use crate::error::Result;

/// The value given for one build parameter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigValue {
    /// Text, which the setting reads as its own type. A configuration file
    /// gives every value this way, a list as comma-separated text.
    Text(String),
    /// A list of texts, which only a setting taking a list accepts. Only
    /// options made in code give one.
    List(Vec<String>),
}

impl ConfigValue {
    /// The value as configuration errors show it.
    pub(crate) fn display(&self) -> String {
        match self {
            ConfigValue::Text(text) => text.clone(),
            ConfigValue::List(list) => list.join(","),
        }
    }
}

impl From<&str> for ConfigValue {
    fn from(text: &str) -> Self {
        ConfigValue::Text(text.to_string())
    }
}

impl From<String> for ConfigValue {
    fn from(text: String) -> Self {
        ConfigValue::Text(text)
    }
}

impl From<Vec<String>> for ConfigValue {
    fn from(list: Vec<String>) -> Self {
        ConfigValue::List(list)
    }
}

impl From<Vec<&str>> for ConfigValue {
    fn from(list: Vec<&str>) -> Self {
        ConfigValue::List(list.into_iter().map(str::to_string).collect())
    }
}

/// How to make one element of a pipeline, or a parallel group of them.
///
/// Either `builder_name` names the builder and `build_parameters` gives its
/// settings, or `sub_elements` lists the elements of a parallel group and
/// the other two are left empty.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ElementOptions {
    /// The builder's name, its name without `Builder` at the end, or one of
    /// its alternate names, in any case.
    pub builder_name: Option<String>,
    /// Each setting's name and value, in the order they are applied. A name
    /// is a setting's name, its name without `Set` at the start, one of its
    /// alternate names, or the name of a parameter of a build method, in any
    /// case.
    pub build_parameters: Vec<(String, ConfigValue)>,
    /// The elements of a parallel group, which run at the same time.
    pub sub_elements: Vec<ElementOptions>,
}

impl ElementOptions {
    /// An element made by the builder `builder_name`.
    pub fn new(builder_name: impl Into<String>) -> Self {
        ElementOptions {
            builder_name: Some(builder_name.into()),
            ..ElementOptions::default()
        }
    }

    /// A parallel group of `elements`.
    pub fn group(elements: impl IntoIterator<Item = ElementOptions>) -> Self {
        ElementOptions {
            sub_elements: elements.into_iter().collect(),
            ..ElementOptions::default()
        }
    }

    /// Add a build parameter. Returns `self` for chaining.
    pub fn parameter(mut self, name: impl Into<String>, value: impl Into<ConfigValue>) -> Self {
        self.build_parameters.push((name.into(), value.into()));
        self
    }
}

/// How to make a pipeline, being its elements in order and the settings of
/// the pipeline itself.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PipelineOptions {
    /// The elements, in the order they run.
    pub elements: Vec<ElementOptions>,
    /// The pipeline's own settings, such as `SetSuppressProcessException`.
    pub build_parameters: Vec<(String, ConfigValue)>,
}

impl PipelineOptions {
    /// Options with no elements and no settings.
    pub fn new() -> Self {
        PipelineOptions::default()
    }

    /// Add an element. Returns `self` for chaining.
    pub fn element(mut self, element: ElementOptions) -> Self {
        self.elements.push(element);
        self
    }

    /// Add a setting of the pipeline itself. Returns `self` for chaining.
    pub fn parameter(mut self, name: impl Into<String>, value: impl Into<ConfigValue>) -> Self {
        self.build_parameters.push((name.into(), value.into()));
        self
    }

    /// Read the `PipelineOptions` section of a JSON configuration file
    /// written for the .NET pipeline, such as an `appsettings.json`.
    ///
    /// The file is read as .NET's JSON configuration reader reads it.
    /// Comments and trailing commas are allowed, names are matched ignoring
    /// case, a number or `true` is read as its text, a `null` builder name
    /// or build parameter is read as empty text, and two names at one place
    /// that differ only in case are refused. Each element's build parameters
    /// are applied in the order .NET's configuration gives them, which is by
    /// name ignoring case, with whole numbers before other names and in
    /// numeric order. Other sections, and settings the .NET web integration
    /// reads from the same section, are ignored.
    ///
    /// A build parameter that is an array or an object is refused, because
    /// .NET cannot read one either. A list is written as comma-separated
    /// text.
    pub fn from_json(text: &str) -> Result<Self> {
        super::json::read_pipeline_options(text)
    }
}
