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

//! The element itself, which runs the scripts over one flow data.
//!
//! The element holds no data file, sends no request over the network, keeps
//! nothing between one request and the next apart from whether each script
//! has logged a missing source element, and reads no evidence from the
//! request. Anything that needs one of those four is a different element and
//! not a script.

use std::any::Any;
use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use fiftyone_pipeline_core::{
    ElementData, Error, EvidenceKeyFilter, EvidenceKeyFilterWhitelist, FlowData, FlowElement,
    MapElementData, NoValueError, Pipeline, PropertyMetaData, PropertyValue, PropertyValueType,
    Result, TypedKey,
};

use crate::canonical::FORMAT;
use crate::evaluate::Outcome;
use crate::fault::{Fault, Faults};
use crate::files;
use crate::model::{Literal, Script, SourceProperty, ValueType};
use crate::scripts::{built_in_names, built_in_scripts_commit, BuiltInScript, BUILT_IN_SOURCE};
use crate::source::{Lookup, PropertySource, SourceValue, WeightedSourceValue};

/// The element data key every script writes into, one property per script,
/// named by the `Output.Name` of that script.
pub const DERIVED_DEFAULT_ELEMENT_DATA_KEY: &str = "derived";

/// The key a weighted value carries its value under, as the 51Degrees element
/// data publishes one. The names are repeated here rather than taken from a
/// product crate, because this element sits above the products and must not
/// depend on either of them.
const WEIGHTED_VALUE_KEY: &str = "value";
/// The key a weighted value carries its raw weighting under, from 0 to
/// 65,535.
const WEIGHTED_RAW_WEIGHTING_KEY: &str = "rawweighting";
/// The key a weighted value from other element data may carry a weight under
/// instead. Only the order of the weights matters here, so either is read as
/// the number it holds.
const WEIGHTED_WEIGHT_KEY: &str = "weight";

/// The element data the [`DerivedPropertyElement`] produces.
///
/// One property per script. A property whose script could not read one of the
/// source properties it names is present here with no value, and reading it
/// raises [`NoValueError`] carrying the message that names every property that
/// was missing.
#[derive(Debug, Clone, Default)]
pub struct DerivedData {
    values: MapElementData,
    /// Keyed by the property name folded to lower case.
    no_value: HashMap<String, String>,
    /// The property names as the scripts write them.
    names: Vec<String>,
}

impl DerivedData {
    /// Record the value a script chose.
    fn set_value(&mut self, name: &str, value: PropertyValue) {
        self.remember(name);
        self.no_value.remove(&name.to_lowercase());
        self.values.insert(name, value);
    }

    /// Record that a script produced no value, and why.
    fn set_no_value(&mut self, name: &str, message: String) {
        self.remember(name);
        self.no_value.insert(name.to_lowercase(), message);
    }

    fn remember(&mut self, name: &str) {
        if !self
            .names
            .iter()
            .any(|held| held.eq_ignore_ascii_case(name))
        {
            self.names.push(name.to_owned());
        }
    }
}

impl ElementData for DerivedData {
    fn get(&self, name: &str) -> std::result::Result<PropertyValue, NoValueError> {
        if let Some(value) = self.values.get_value(name) {
            return Ok(value.clone());
        }
        if let Some(message) = self.no_value.get(&name.to_lowercase()) {
            return Err(NoValueError::new(message.clone()));
        }
        Err(NoValueError::new(format!(
            "No value for property '{name}'."
        )))
    }

    fn keys(&self) -> Vec<String> {
        self.names.clone()
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

/// Computes a property from properties other elements have already produced.
///
/// Build one with [`DerivedPropertyElement::builder`]. Each script produces one
/// property, and every script writes into the element data key `derived`.
#[derive(Debug)]
pub struct DerivedPropertyElement {
    scripts: Vec<Script>,
    /// One for each script, set once the script has logged a source element
    /// whose data a request did not carry.
    reported: Box<[AtomicBool]>,
    properties: Vec<PropertyMetaData>,
    filter: EvidenceKeyFilterWhitelist,
}

impl DerivedPropertyElement {
    /// The typed key under which this element stores its [`DerivedData`].
    pub const KEY: TypedKey<DerivedData> = TypedKey::new(DERIVED_DEFAULT_ELEMENT_DATA_KEY);

    /// Start building an element.
    pub fn builder() -> DerivedPropertyElementBuilder {
        DerivedPropertyElementBuilder::new()
    }

    /// The scripts this element runs, in the order they run.
    pub fn scripts(&self) -> &[Script] {
        &self.scripts
    }

    /// Every reason this element cannot work among `elements`, one line
    /// each in .NET's words. The properties another derived property
    /// element also writes come first, then the source properties no
    /// element supplies.
    ///
    /// Where an element sits is not judged, because the members of a
    /// parallel group are listed after the elements around the group. A
    /// source property written after this element runs has no value on
    /// each request, and the element logs that once.
    fn pipeline_faults(&self, elements: &[Arc<dyn FlowElement>]) -> Vec<String> {
        let mut faults = Vec::new();
        for script in &self.scripts {
            let name = &script.output().name;
            for element in elements {
                // An element yet to load its properties lists none, as .NET
                // reads one.
                if self.is_self(element)
                    || !element
                        .data_key()
                        .eq_ignore_ascii_case(DERIVED_DEFAULT_ELEMENT_DATA_KEY)
                    || !element.properties_loaded()
                    || !lists(element.as_ref(), name)
                {
                    continue;
                }
                faults.push(format!(
                    "The script '{}' writes the derived property '{name}', and another \
                     element in the pipeline already writes a property of that name. One \
                     pipeline writes each derived property once.",
                    script.name()
                ));
            }
        }
        for (index, script) in self.scripts.iter().enumerate() {
            for property in script.source_properties() {
                if self.supplied(index, elements, property) {
                    continue;
                }
                let name = &property.name;
                faults.push(format!(
                    "The script '{}' needs the property '{name}', and no element in the \
                     pipeline supplies it. Either add the element that supplies '{name}', or \
                     change the script so that it does not name '{name}'.",
                    script.name()
                ));
            }
        }
        faults
    }

    /// Whether `element` is this element, or one wrapping it that passes
    /// calls on and so gives this element's own property list. No other
    /// element can give that list, because it is never empty.
    fn is_self(&self, element: &Arc<dyn FlowElement>) -> bool {
        std::ptr::addr_eq(Arc::as_ptr(element), self as *const Self)
            || std::ptr::eq(element.properties().as_ptr(), self.properties.as_ptr())
    }

    /// Whether a source property of the script at `index` is supplied.
    ///
    /// This element supplies only what its scripts before that one write,
    /// because a script reads the outcomes of the scripts already run, so
    /// a property a later script writes never has a value when it is read.
    /// Another element supplies a property when it has the source
    /// property's element data key and lists the property, or has that key
    /// and has yet to load its properties. A cloud engine has its list when
    /// the pipeline is built unless the request for the list failed for a
    /// reason that may pass, such as the cloud being out of reach, and it
    /// asks again when first used, so such a fault does not refuse the
    /// pipeline.
    fn supplied(
        &self,
        index: usize,
        elements: &[Arc<dyn FlowElement>],
        property: &SourceProperty,
    ) -> bool {
        if property
            .element_key
            .eq_ignore_ascii_case(DERIVED_DEFAULT_ELEMENT_DATA_KEY)
            && self.scripts[..index].iter().any(|earlier| {
                earlier
                    .output()
                    .name
                    .eq_ignore_ascii_case(&property.property_name)
            })
        {
            return true;
        }
        elements.iter().any(|element| {
            !self.is_self(element)
                && element
                    .data_key()
                    .eq_ignore_ascii_case(&property.element_key)
                && (lists(element.as_ref(), &property.property_name)
                    || !element.properties_loaded())
        })
    }
}

/// Whether an element's metadata lists a property, ignoring case.
fn lists(element: &dyn FlowElement, name: &str) -> bool {
    element
        .properties()
        .iter()
        .any(|property| property.name.eq_ignore_ascii_case(name))
}

impl FlowElement for DerivedPropertyElement {
    fn process(&self, data: &mut FlowData) -> Result<()> {
        // Every script is run before anything is written, because a script may
        // name a property an earlier script in this same element produced and
        // the element data cannot be borrowed for reading and writing at once.
        let results = {
            let source = FlowDataSource::new(data);
            for (script, reported) in self.scripts.iter().zip(self.reported.iter()) {
                let outcome = script.evaluate(&source);
                if let Some((property, element)) = source.take_absent() {
                    report_absent_element(script, reported, &property, &element);
                }
                source.remember(script.output().name.clone(), outcome);
            }
            source.into_results()
        };

        let value_types: Vec<ValueType> = self
            .scripts
            .iter()
            .map(|script| script.output().value_type)
            .collect();
        let derived = data.get_or_add(Self::KEY, DerivedData::default)?;
        for ((name, outcome), value_type) in results.into_iter().zip(value_types) {
            match outcome {
                Outcome::Value(literal) => {
                    derived.set_value(&name, property_value(&literal, value_type));
                }
                Outcome::NoValue { message, .. } => derived.set_no_value(&name, message),
            }
        }
        Ok(())
    }

    fn data_key(&self) -> &str {
        DERIVED_DEFAULT_ELEMENT_DATA_KEY
    }

    fn evidence_key_filter(&self) -> &dyn EvidenceKeyFilter {
        &self.filter
    }

    fn properties(&self) -> &[PropertyMetaData] {
        &self.properties
    }

    /// Refuse a pipeline in which no element supplies a source property a
    /// script reads, or another element with the data key `derived` writes
    /// a property this element writes, as the specification's pipeline
    /// check asks. The error names every fault, with .NET's message.
    fn add_pipeline(&self, pipeline: &Arc<Pipeline>) -> Result<()> {
        let faults = self.pipeline_faults(pipeline.flow_elements());
        if faults.is_empty() {
            return Ok(());
        }
        let mut message =
            "The derived property element cannot be used in this pipeline.".to_owned();
        for fault in faults {
            message.push('\n');
            message.push_str(&fault);
        }
        Err(Error::configuration(message))
    }
}

/// Builds a [`DerivedPropertyElement`] from the scripts it is to run.
///
/// Faults are collected as each script is added, so one `build` reports
/// everything wrong with every script rather than stopping at the first.
#[derive(Default)]
pub struct DerivedPropertyElementBuilder {
    scripts: Vec<Script>,
    faults: Vec<Fault>,
}

impl DerivedPropertyElementBuilder {
    /// A builder with no scripts.
    pub fn new() -> Self {
        DerivedPropertyElementBuilder::default()
    }

    /// Add a script shipped inside this crate.
    pub fn add_built_in(mut self, script: BuiltInScript) -> Self {
        match script.compile() {
            Ok(script) => self.scripts.push(script),
            Err(faults) => self.faults.extend(faults.into_inner()),
        }
        self
    }

    /// Add the text of a script held by the caller, as YAML or as JSON.
    ///
    /// `name` is what a configuration would select the script by, which the
    /// script's own `Name` must equal, and it is also what a fault names.
    pub fn add_script(mut self, name: &str, text: &str) -> Self {
        match Script::compile_named(text, Some(name), "code") {
            Ok(script) => self.scripts.push(script),
            Err(faults) => self.faults.extend(faults.into_inner()),
        }
        self
    }

    /// Add a script from a file, whose name without its extension the script's
    /// own `Name` must equal.
    ///
    /// The scripts 51Degrees ships are compiled in rather than read from a
    /// file, so this is for a script of the caller's own. It is also the one
    /// part of this crate that needs a file system.
    ///
    /// A `*` or `?` in the file name adds every file in the folder whose name
    /// matches, in order of their paths, as .NET's `Directory.GetFiles`
    /// matches them. So `derived/*.yaml` adds every YAML script in
    /// `derived`, and a folder part that is empty means the current folder.
    /// Letter case is ignored on Windows and Apple systems only. A folder
    /// that does not exist is a fault, and a pattern that matches nothing
    /// adds nothing.
    ///
    /// A script read from a file gives its full path as its source.
    pub fn add_script_file(mut self, path: impl AsRef<Path>) -> Self {
        let path = path.as_ref();
        if !files::has_wildcard(&path.to_string_lossy()) {
            return self.add_one_file(path);
        }
        let (folder, pattern) = files::split(path);
        let fault = |message: String| Fault {
            script: None,
            source: path.display().to_string(),
            path: String::new(),
            line: None,
            message,
        };
        if !folder.is_dir() {
            self.faults.push(fault(format!(
                "the folder '{}' does not exist",
                folder.display()
            )));
            return self;
        }
        let entries = match std::fs::read_dir(&folder) {
            Ok(entries) => entries,
            Err(error) => {
                self.faults.push(fault(format!(
                    "the folder '{}' cannot be read: {error}",
                    folder.display()
                )));
                return self;
            }
        };
        let pattern = files::translate(&pattern);
        let mut found: Vec<PathBuf> = entries
            .flatten()
            .map(|entry| entry.path())
            .filter(|file| !file.is_dir())
            .filter(|file| {
                file.file_name().is_some_and(|name| {
                    files::matches(&pattern, &name.to_string_lossy(), files::IGNORE_CASE)
                })
            })
            .collect();
        // Ordered so a wildcard adds the same scripts in the same order on
        // every machine, comparing UTF-16 code units as .NET's ordinal order
        // does.
        found.sort_by(|a, b| {
            a.to_string_lossy()
                .encode_utf16()
                .cmp(b.to_string_lossy().encode_utf16())
        });
        for file in found {
            self = self.add_one_file(&file);
        }
        self
    }

    /// Add the one script file at `path`, with .NET's faults for a file that
    /// is not there or cannot be read.
    fn add_one_file(mut self, path: &Path) -> Self {
        let fault = |message: String| Fault {
            script: None,
            source: path.display().to_string(),
            path: String::new(),
            line: None,
            message,
        };
        // A folder is not a file, as .NET's `FileInfo.Exists` has it.
        if !path.exists() || path.is_dir() {
            self.faults
                .push(fault("the file does not exist".to_owned()));
            return self;
        }
        let text = match std::fs::read_to_string(path) {
            Ok(text) => text,
            Err(error) => {
                self.faults
                    .push(fault(format!("the file cannot be read: {error}")));
                return self;
            }
        };
        let name = path
            .file_stem()
            .map(|stem| stem.to_string_lossy().into_owned());
        let source = std::path::absolute(path)
            .unwrap_or_else(|_| path.to_path_buf())
            .display()
            .to_string();
        match Script::compile_named(&text, name.as_deref(), &source) {
            Ok(script) => self.scripts.push(script),
            Err(faults) => self.faults.extend(faults.into_inner()),
        }
        self
    }

    /// Add a script that has already been read.
    pub fn add_compiled(mut self, script: Script) -> Self {
        self.scripts.push(script);
        self
    }

    /// Add the shipped script called `name`, matched ignoring case, as a
    /// configuration names one. An empty name, or one no shipped script
    /// has, is a fault.
    pub(crate) fn add_built_in_named(mut self, name: &str) -> Self {
        if name.trim().is_empty() {
            self.faults.push(Fault {
                script: None,
                source: BUILT_IN_SOURCE.to_owned(),
                path: String::new(),
                line: None,
                message: "a built in script name is empty".to_owned(),
            });
            return self;
        }
        let Some(script) = BuiltInScript::named(name) else {
            self.faults.push(Fault {
                script: Some(name.to_owned()),
                source: BUILT_IN_SOURCE.to_owned(),
                path: String::new(),
                line: None,
                message: format!(
                    "there is no script called '{name}' in this package. The scripts in this \
                     package are {}. A script from your own environment is added with \
                     ScriptFiles rather than Scripts.",
                    built_in_names()
                ),
            });
            return self;
        };
        let source = format!("{BUILT_IN_SOURCE} {name}");
        match Script::compile_named(script.text(), Some(script.name()), &source) {
            Ok(script) => self.scripts.push(script),
            Err(faults) => self.faults.extend(faults.into_inner()),
        }
        self
    }

    /// True when nothing has been added, neither a script nor a fault.
    pub(crate) fn is_empty(&self) -> bool {
        self.scripts.is_empty() && self.faults.is_empty()
    }

    /// Build the element, or report everything wrong with the scripts.
    ///
    /// An element with no scripts is a fault rather than an element that does
    /// nothing, because it is a configuration that cannot be what anybody
    /// meant. Two scripts producing the same property name are a fault for the
    /// same reason, since the second would overwrite the first under the one
    /// `derived` key.
    pub fn build(mut self) -> std::result::Result<DerivedPropertyElement, Faults> {
        if self.scripts.is_empty() && self.faults.is_empty() {
            self.faults.push(Fault {
                script: None,
                source: "code".to_owned(),
                path: String::new(),
                line: None,
                message: "no scripts were added, so the element would produce nothing".to_owned(),
            });
        }
        let mut seen: Vec<String> = Vec::new();
        for script in &self.scripts {
            let name = script.output().name.to_lowercase();
            if seen.contains(&name) {
                self.faults.push(Fault {
                    script: Some(script.name().to_owned()),
                    source: script.source().to_owned(),
                    path: "Output.Name".to_owned(),
                    line: None,
                    message: format!(
                        "two scripts both produce the property '{}', and only one of the two \
                         could be published under the '{DERIVED_DEFAULT_ELEMENT_DATA_KEY}' key",
                        script.output().name
                    ),
                });
            }
            seen.push(name);
        }
        if !self.faults.is_empty() {
            return Err(Faults::new(self.faults));
        }
        log_scripts(&self.scripts);
        let reported = self
            .scripts
            .iter()
            .map(|_| AtomicBool::new(false))
            .collect();
        let properties = self
            .scripts
            .iter()
            .map(|script| {
                let output = script.output();
                PropertyMetaData::new(
                    output.name.clone(),
                    DERIVED_DEFAULT_ELEMENT_DATA_KEY,
                    property_value_type(output.value_type),
                )
                .with_category(output.category.clone().unwrap_or_default())
            })
            .collect();
        Ok(DerivedPropertyElement {
            scripts: self.scripts,
            reported,
            properties,
            // The element reads no evidence, so it advertises none. What it
            // reads is the element data earlier elements produced.
            filter: EvidenceKeyFilterWhitelist::new(Vec::<&str>::new()),
        })
    }
}

/// Log each script at build. One information line per script gives its name,
/// version, format, source and output, a deprecated script adds a warning
/// carrying its note, and at debug level the compiled script follows as
/// canonical JSON, so anyone holding the log can see what was evaluated without
/// the file. Where any script is shipped in this crate, one line first names
/// the derived-properties commit the shipped scripts came from.
fn log_scripts(scripts: &[Script]) {
    if scripts
        .iter()
        .any(|script| script.source().starts_with(BUILT_IN_SOURCE))
    {
        log::info!(
            "Built in derived property scripts came from derived-properties commit {}.",
            built_in_scripts_commit()
        );
    }
    for script in scripts {
        let output = script.output();
        log::info!(
            "Derived property script '{}' version {} (format {}) from {} produces '{}' as {}.",
            script.name(),
            script.version(),
            FORMAT,
            script.source(),
            output.name,
            output.value_type
        );
        if script.is_deprecated() {
            log::warn!(
                "Derived property script '{}' is deprecated. {}",
                script.name(),
                script.deprecation_note().unwrap_or_default()
            );
        }
        if log::log_enabled!(log::Level::Debug) {
            log::debug!(
                "Derived property script '{}' compiled to {}",
                script.name(),
                script.to_canonical_json()
            );
        }
    }
}

/// Reads source properties out of a flow data, and out of the scripts this
/// element has already run.
struct FlowDataSource<'a> {
    data: &'a FlowData,
    /// What the scripts run so far produced, so that a script can name a
    /// property an earlier script in the same element produces.
    results: RefCell<Vec<(String, Outcome)>>,
    /// The property names each element data holds, read once and only where a
    /// property could not be read, which is what tells a property that is not
    /// there at all from one whose source supplied a no value message.
    keys: RefCell<HashMap<String, Vec<String>>>,
    /// The first source property, and its element key, whose element data
    /// the request did not carry though an element in the pipeline writes
    /// it, since the last [`Self::take_absent`].
    absent: RefCell<Option<(String, String)>>,
}

impl<'a> FlowDataSource<'a> {
    fn new(data: &'a FlowData) -> Self {
        FlowDataSource {
            data,
            results: RefCell::new(Vec::new()),
            keys: RefCell::new(HashMap::new()),
            absent: RefCell::new(None),
        }
    }

    /// Note a source whose element data the request did not carry, where an
    /// element in the pipeline writes that data. A script reading another
    /// derived property is left out, because this element writes `derived`
    /// only after every script has run.
    fn note_absent(&self, element_key: &str, property_name: &str) {
        let mut absent = self.absent.borrow_mut();
        if absent.is_some() || element_key.eq_ignore_ascii_case(DERIVED_DEFAULT_ELEMENT_DATA_KEY) {
            return;
        }
        let written = self
            .data
            .pipeline()
            .flow_elements()
            .iter()
            .any(|element| element.data_key().eq_ignore_ascii_case(element_key));
        if written {
            *absent = Some((
                format!("{element_key}.{property_name}"),
                element_key.to_owned(),
            ));
        }
    }

    /// Take what [`Self::note_absent`] noted for the script just run.
    fn take_absent(&self) -> Option<(String, String)> {
        self.absent.borrow_mut().take()
    }

    fn remember(&self, name: String, outcome: Outcome) {
        self.results.borrow_mut().push((name, outcome));
    }

    fn into_results(self) -> Vec<(String, Outcome)> {
        self.results.into_inner()
    }

    /// Whether an element data holds a property at all, which decides between
    /// the two wordings a missing property carries.
    fn element_holds(&self, element_key: &str, property_name: &str) -> bool {
        let mut cache = self.keys.borrow_mut();
        let names = cache.entry(element_key.to_lowercase()).or_insert_with(|| {
            self.data
                .get_str(element_key)
                .map(|element| element.keys())
                .unwrap_or_default()
        });
        names
            .iter()
            .any(|name| name.eq_ignore_ascii_case(property_name))
    }
}

impl PropertySource for FlowDataSource<'_> {
    fn lookup(&self, element_key: &str, property_name: &str) -> Lookup {
        if element_key.eq_ignore_ascii_case(DERIVED_DEFAULT_ELEMENT_DATA_KEY) {
            let results = self.results.borrow();
            if let Some((_, outcome)) = results
                .iter()
                .find(|(name, _)| name.eq_ignore_ascii_case(property_name))
            {
                return match outcome {
                    Outcome::Value(literal) => Lookup::Value(source_value(literal)),
                    Outcome::NoValue { message, .. } => Lookup::NoValue(message.clone()),
                };
            }
        }
        let element = match self.data.get_str(element_key) {
            None => {
                self.note_absent(element_key, property_name);
                return Lookup::Absent;
            }
            Some(element) => element,
        };
        match element.get(property_name) {
            Ok(value) => lookup_of(value),
            Err(no_value) => {
                if self.element_holds(element_key, property_name) {
                    Lookup::NoValue(no_value.message)
                } else {
                    Lookup::Absent
                }
            }
        }
    }
}

/// Log, once for each script, that a request carried no data from an element
/// the pipeline holds, because that element runs after this one or did not
/// run on that request, so the script has no value.
fn report_absent_element(script: &Script, reported: &AtomicBool, property: &str, element: &str) {
    if reported.swap(true, Ordering::Relaxed) {
        return;
    }
    log::error!(
        "The script '{}' reads '{property}', and the request carried no element data \
         '{element}' at all. An element in the pipeline writes '{element}', so that element \
         is running after this one rather than before it, or it failed or was skipped on \
         this request. Add the derived property element after it. This is reported once.",
        script.name()
    );
}

/// What a value in a flow data is to a script.
fn lookup_of(value: PropertyValue) -> Lookup {
    match value {
        PropertyValue::String(text) | PropertyValue::JavaScript(text) => {
            Lookup::Value(SourceValue::Text(text))
        }
        PropertyValue::WktString(text) => Lookup::Value(SourceValue::Text(text.into_string())),
        PropertyValue::Bool(flag) => Lookup::Value(SourceValue::Bool(flag)),
        PropertyValue::Integer(number) => Lookup::Value(SourceValue::Int(number)),
        PropertyValue::Double(number) => Lookup::Value(SourceValue::Double(number)),
        PropertyValue::Float(number) => Lookup::Value(SourceValue::Double(float_as_double(number))),
        PropertyValue::StringList(_) => Lookup::List,
        // A list of weighted values takes the value with the highest weight. A
        // list of anything else, where a single value is needed, cannot be
        // read.
        PropertyValue::KeyValueList(records) => {
            let mut candidates = Vec::with_capacity(records.len());
            for record in &records {
                let value = match record.get(WEIGHTED_VALUE_KEY) {
                    None => return Lookup::List,
                    Some(value) => value,
                };
                let value = match single_value(value) {
                    None => return Lookup::List,
                    Some(value) => value,
                };
                let weight = record
                    .get(WEIGHTED_RAW_WEIGHTING_KEY)
                    .or_else(|| record.get(WEIGHTED_WEIGHT_KEY))
                    .and_then(PropertyValue::as_double)
                    .unwrap_or(0.0);
                candidates.push(WeightedSourceValue { weight, value });
            }
            if candidates.is_empty() {
                Lookup::List
            } else {
                Lookup::Weighted(candidates)
            }
        }
        _ => Lookup::List,
    }
}

fn single_value(value: &PropertyValue) -> Option<SourceValue> {
    match value {
        PropertyValue::String(text) | PropertyValue::JavaScript(text) => {
            Some(SourceValue::Text(text.clone()))
        }
        PropertyValue::WktString(text) => Some(SourceValue::Text(text.as_str().to_owned())),
        PropertyValue::Bool(flag) => Some(SourceValue::Bool(*flag)),
        PropertyValue::Integer(number) => Some(SourceValue::Int(*number)),
        PropertyValue::Double(number) => Some(SourceValue::Double(*number)),
        PropertyValue::Float(number) => Some(SourceValue::Double(float_as_double(*number))),
        _ => None,
    }
}

/// A 32-bit float as the `f64` nearest its decimal, as
/// [`PropertyValue::as_double`] gives it, so a latitude of 18.543 compares
/// equal to the 18.543 a script holds.
fn float_as_double(number: f32) -> f64 {
    PropertyValue::Float(number)
        .as_double()
        .unwrap_or(f64::from(number))
}

/// A value one script produced, as the next script reads it.
fn source_value(literal: &Literal) -> SourceValue {
    match literal {
        Literal::Bool(flag) => SourceValue::Bool(*flag),
        Literal::Int(number) => SourceValue::Int(i64::from(*number)),
        Literal::Double(number) => SourceValue::Double(*number),
        Literal::Text(text) => SourceValue::Text(text.clone()),
    }
}

/// The value a script chose, as the element data publishes it. The type is the
/// one the script's `Output` declares, which validation has already matched the
/// literal against.
fn property_value(literal: &Literal, value_type: ValueType) -> PropertyValue {
    match (literal, value_type) {
        (Literal::Bool(flag), _) => PropertyValue::Bool(*flag),
        (Literal::Int(number), ValueType::Double) => PropertyValue::Double(f64::from(*number)),
        (Literal::Int(number), _) => PropertyValue::Integer(i64::from(*number)),
        (Literal::Double(number), _) => PropertyValue::Double(*number),
        (Literal::Text(text), _) => PropertyValue::String(text.clone()),
    }
}

fn property_value_type(value_type: ValueType) -> PropertyValueType {
    match value_type {
        ValueType::String => PropertyValueType::String,
        ValueType::Bool => PropertyValueType::Bool,
        ValueType::Int => PropertyValueType::Integer,
        ValueType::Double => PropertyValueType::Double,
    }
}

/// Faults are a configuration fault in pipeline terms, which is what a builder
/// that could not read its scripts has.
impl From<Faults> for Error {
    fn from(faults: Faults) -> Self {
        Error::configuration(faults.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fiftyone_pipeline_core::WktString;

    /// A WKT value, such as an IP Intelligence area, is read as its text.
    #[test]
    fn a_wkt_value_is_read_as_its_text() {
        let text = || SourceValue::Text("POINT(2 4)".to_owned());
        let value = PropertyValue::WktString(WktString::new("POINT(2 4)"));
        assert_eq!(single_value(&value), Some(text()));
        assert_eq!(lookup_of(value), Lookup::Value(text()));
    }
}
