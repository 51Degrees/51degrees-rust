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

//! Element builders found by name, and the pipeline made from
//! [`PipelineOptions`] with them, as .NET's
//! `PipelineBuilder.BuildFromConfiguration` makes it.

use std::any::{type_name, Any, TypeId};
use std::sync::Arc;

use super::options::{ConfigValue, ElementOptions, PipelineOptions};
use super::text::{equal_ignoring_case, ConfigType, ParseFailure};
use crate::error::{Error, Result};
use crate::flow_element::FlowElement;
use crate::pipeline::{Pipeline, PipelineBuilder};

/// The names a configuration can find a setting by.
trait SettingNames {
    fn name(&self) -> &str;
    fn alternate_names(&self) -> &[String];
}

/// What the builders of one pipeline share while it is made from a
/// configuration.
///
/// It starts with the resources given to the [`PipelineBuilder`]. A builder
/// adds a resource for the builders after it, such as the cloud request
/// engine that the cloud engines need, and the pipeline keeps every
/// resource once it is built.
pub struct BuildContext {
    resources: Vec<Arc<dyn Any + Send + Sync>>,
}

impl BuildContext {
    /// The resource of type `T` added most recently, so a builder finds the
    /// one made just before it.
    pub fn resource<T: Any + Send + Sync>(&self) -> Option<Arc<T>> {
        self.resources
            .iter()
            .rev()
            .find_map(|resource| Arc::clone(resource).downcast::<T>().ok())
    }

    /// Every resource of type `T`, oldest first, so a builder can tell
    /// whether more than one was given or made.
    pub fn resources<T: Any + Send + Sync>(&self) -> Vec<Arc<T>> {
        self.resources
            .iter()
            .filter_map(|resource| Arc::clone(resource).downcast::<T>().ok())
            .collect()
    }

    /// Add a resource for the builders that follow and for the pipeline.
    pub fn add_resource(&mut self, resource: Arc<dyn Any + Send + Sync>) {
        self.resources.push(resource);
    }
}

/// A parameter of a build method, being its name and the type its value is
/// read as.
pub struct Parameter {
    name: String,
    type_name: &'static str,
    parse: ParseBoxed,
}

type ParseBoxed = fn(&ConfigValue) -> std::result::Result<Box<dyn Any + Send>, ParseFailure>;

impl Parameter {
    /// A parameter called `name`, whose value is read as `T`.
    pub fn new<T: ConfigType>(name: impl Into<String>) -> Self {
        Parameter {
            name: name.into(),
            type_name: T::TYPE_NAME,
            parse: |value: &ConfigValue| {
                T::parse_value(value).map(|parsed| Box::new(parsed) as Box<dyn Any + Send>)
            },
        }
    }

    /// A parameter called `name` that takes its value in code only, such as
    /// a stream. .NET lists a build method with such a parameter and refuses
    /// any text a configuration gives it, because the type has no
    /// `TryParse`. `type_name` is the .NET type's name, such as `Stream`.
    pub fn code_only(name: impl Into<String>, type_name: &'static str) -> Self {
        Parameter {
            name: name.into(),
            type_name,
            parse: |_| Err(ParseFailure::no_text_form()),
        }
    }

    fn signature(&self) -> String {
        format!("{} ({})", self.name, self.type_name)
    }
}

/// The values of a build method's parameters, read as their types.
pub struct BuildArguments {
    values: Vec<(String, Box<dyn Any + Send>)>,
}

impl BuildArguments {
    /// The value of the parameter called `name`, ignoring case.
    ///
    /// # Panics
    ///
    /// When the build method has no such parameter or it is not of type
    /// `T`, which is a mistake in the builder's definition.
    pub fn get<T: 'static>(&self, name: &str) -> &T {
        let value = self
            .values
            .iter()
            .find(|(parameter, _)| equal_ignoring_case(parameter, name))
            .unwrap_or_else(|| panic!("the build method has no parameter '{name}'"))
            .1
            .downcast_ref::<T>();
        value
            .unwrap_or_else(|| panic!("the parameter '{name}' is not of type {}", type_name::<T>()))
    }
}

/// A setting whose value has been read, waiting for the builder.
type Pending<B> = Box<dyn FnOnce(B) -> Result<B> + Send>;

type Prepare<B> =
    Box<dyn Fn(&ConfigValue) -> std::result::Result<Pending<B>, ParseFailure> + Send + Sync>;

struct Setting<B> {
    name: String,
    alternate_names: Vec<String>,
    type_name: &'static str,
    /// How many values the method takes. Only a method taking one is a
    /// setting a configuration can call.
    parameters: usize,
    prepare: Prepare<B>,
}

impl<B: Send + 'static> Setting<B> {
    fn new<T, F>(name: String, apply: F) -> Self
    where
        T: ConfigType,
        F: Fn(B, T) -> Result<B> + Send + Sync + 'static,
    {
        let apply = Arc::new(apply);
        Setting {
            name,
            alternate_names: Vec::new(),
            type_name: T::TYPE_NAME,
            parameters: 1,
            prepare: Box::new(
                move |value: &ConfigValue| -> std::result::Result<Pending<B>, ParseFailure> {
                    let parsed = T::parse_value(value)?;
                    let apply = Arc::clone(&apply);
                    Ok(Box::new(move |builder| apply(builder, parsed)))
                },
            ),
        }
    }
}

impl<B> SettingNames for Setting<B> {
    fn name(&self) -> &str {
        &self.name
    }

    fn alternate_names(&self) -> &[String] {
        &self.alternate_names
    }
}

type Create<B> = Box<dyn Fn(&BuildArguments, &mut BuildContext) -> Result<B> + Send + Sync>;

struct BuildMethod<B> {
    parameters: Vec<Parameter>,
    create: Create<B>,
}

impl<B> BuildMethod<B> {
    fn signature(&self) -> String {
        self.parameters
            .iter()
            .map(Parameter::signature)
            .collect::<Vec<_>>()
            .join(",")
    }
}

type Finish<B> = Box<dyn Fn(B, &mut BuildContext) -> Result<Arc<dyn FlowElement>> + Send + Sync>;

/// How a configuration makes an element with one builder, playing the part
/// .NET's reflection over a builder's methods plays.
///
/// `B` is the builder. Each setting is one of its methods taking one value,
/// named as the .NET builder's method is so that a configuration file
/// written for .NET works here. Each build method lists the parameters it
/// takes and makes the builder from them, and the definition's last step
/// makes the element from the builder.
///
/// Settings are read first, in the order the configuration gives them, and
/// any name that is not a setting is taken as a build method's parameter.
/// Settings that share a name are tried in the order they were added, as
/// .NET tries a method's overloads, until one reads the value. A list given
/// for a text setting ends the search, as .NET's call refuses it, and so
/// does reaching a method that takes no value or several
/// ([`method_with_parameters`](Self::method_with_parameters)).
/// The build method whose parameters are exactly those names is chosen,
/// it makes the builder, the settings are applied to it in order, and the
/// last step makes the element.
pub struct BuilderDefinition<B> {
    name: String,
    full_name: String,
    alternate_names: Vec<String>,
    settings: Vec<Setting<B>>,
    build_methods: Vec<BuildMethod<B>>,
    finish: Finish<B>,
}

impl<B: Send + 'static> BuilderDefinition<B> {
    /// A builder found by `name`, such as `SequenceElementBuilder`, whose
    /// element `finish` makes. Its full name in errors is the Rust name of
    /// `B`.
    pub fn new<F>(name: impl Into<String>, finish: F) -> Self
    where
        F: Fn(B, &mut BuildContext) -> Result<Arc<dyn FlowElement>> + Send + Sync + 'static,
    {
        BuilderDefinition {
            name: name.into(),
            full_name: type_name::<B>().to_string(),
            alternate_names: Vec::new(),
            settings: Vec::new(),
            build_methods: Vec::new(),
            finish: Box::new(finish),
        }
    }

    /// Another name the builder is found by, as .NET's `AlternateName`
    /// attribute gives one.
    pub fn alternate_name(mut self, name: impl Into<String>) -> Self {
        self.alternate_names.push(name.into());
        self
    }

    /// A setting called `name`, which reads its value as `T` and gives it
    /// to `apply`.
    pub fn setting<T, F>(self, name: impl Into<String>, apply: F) -> Self
    where
        T: ConfigType,
        F: Fn(B, T) -> B + Send + Sync + 'static,
    {
        self.try_setting(name, move |builder, value| Ok(apply(builder, value)))
    }

    /// A setting called `name` whose `apply` can refuse the value.
    pub fn try_setting<T, F>(mut self, name: impl Into<String>, apply: F) -> Self
    where
        T: ConfigType,
        F: Fn(B, T) -> Result<B> + Send + Sync + 'static,
    {
        self.settings.push(Setting::new(name.into(), apply));
        self
    }

    /// A setting called `name` that takes its value in code only, such as a
    /// list of converters. .NET finds such a method and then refuses any
    /// value a configuration gives, because the type has no `TryParse`, so
    /// naming it here fails with .NET's message. `type_name` is the .NET
    /// type's name, such as ``IEnumerable`1``.
    pub fn code_only_setting(mut self, name: impl Into<String>, type_name: &'static str) -> Self {
        self.settings.push(Setting {
            name: name.into(),
            alternate_names: Vec::new(),
            type_name,
            parameters: 1,
            prepare: Box::new(|value: &ConfigValue| match value {
                ConfigValue::Text(_) => Err(ParseFailure::no_text_form()),
                ConfigValue::List(_) => Err(ParseFailure::list()),
            }),
        });
        self
    }

    /// A method called `name` taking `parameters` values rather than one,
    /// such as .NET's `AddScript(string name, string content)`. .NET finds
    /// a builder's methods by name whatever they take, and refuses one that
    /// does not take one value as soon as its search reaches it, so naming
    /// it fails with .NET's message. A setting of the same name added
    /// before it is still tried first, and one added after it never is.
    ///
    /// # Panics
    ///
    /// When `parameters` is 1, because a method taking one value is a
    /// setting.
    pub fn method_with_parameters(mut self, name: impl Into<String>, parameters: usize) -> Self {
        assert_ne!(parameters, 1, "a method taking one value is a setting");
        self.settings.push(Setting {
            name: name.into(),
            alternate_names: Vec::new(),
            type_name: "",
            parameters,
            prepare: Box::new(|_: &ConfigValue| Err(ParseFailure::text())),
        });
        self
    }

    /// Another name for the settings called `setting`, as .NET's
    /// `AlternateName` attribute gives one.
    ///
    /// # Panics
    ///
    /// When no setting is called `setting`.
    pub fn setting_alternate_name(mut self, setting: &str, alternate: impl Into<String>) -> Self {
        let alternate = alternate.into();
        let mut found = false;
        for named in self.settings.iter_mut().filter(|s| s.name == setting) {
            named.alternate_names.push(alternate.clone());
            found = true;
        }
        assert!(found, "the builder has no setting '{setting}'");
        self
    }

    /// A build method taking `parameters`, which `create` makes the builder
    /// from.
    pub fn build_method<F>(mut self, parameters: Vec<Parameter>, create: F) -> Self
    where
        F: Fn(&BuildArguments, &mut BuildContext) -> Result<B> + Send + Sync + 'static,
    {
        self.build_methods.push(BuildMethod {
            parameters,
            create: Box::new(create),
        });
        self
    }
}

/// The elements of a pipeline made from a configuration, in the order they
/// run, as a [`PipelineDefinition`] sees them just before the pipeline is
/// built. Each is an element on its own or a parallel group.
pub struct PipelineElements {
    steps: Vec<Vec<Arc<dyn FlowElement>>>,
}

impl PipelineElements {
    /// True if an element on its own, outside a parallel group, is an `E`,
    /// as [`FlowElement::type_name`] names it. An element in a group is not
    /// found, as .NET's `FiftyOnePipelineBuilder` does not find one. A group
    /// of one element is that element on its own.
    pub fn contains<E: FlowElement>(&self) -> bool {
        let wanted = type_name::<E>();
        self.steps
            .iter()
            .any(|step| matches!(step.as_slice(), [element] if element.type_name() == wanted))
    }

    /// Add `element` to run before the others.
    pub fn insert_first(&mut self, element: Arc<dyn FlowElement>) {
        self.steps.insert(0, vec![element]);
    }

    /// Add `element` to run after the others.
    pub fn push(&mut self, element: Arc<dyn FlowElement>) {
        self.steps.push(vec![element]);
    }
}

type BeforeBuild<S> =
    Box<dyn Fn(S, &mut PipelineElements, &mut BuildContext) -> Result<()> + Send + Sync>;

/// The pipeline builder a configuration's own settings are given to, as
/// .NET gives them to the class `BuildFromConfiguration` is called on.
///
/// .NET's `FiftyOnePipelineBuilder`, for example, adds the setting
/// `SetShareUsage` to every pipeline's settings, and adds the elements it
/// needs that the configuration left out before it builds. `S` holds the
/// values of the settings this builder adds. A setting is found as an
/// element builder's is (see [`BuilderDefinition`]), among these and every
/// pipeline's own. Once the elements are made and the settings read,
/// `before_build` runs, as .NET's `OnPreBuild` does.
pub struct PipelineDefinition<S> {
    full_name: String,
    new: Box<dyn Fn() -> S + Send + Sync>,
    settings: Vec<Setting<S>>,
    before_build: BeforeBuild<S>,
}

impl<S: Send + 'static> PipelineDefinition<S> {
    /// A pipeline builder whose settings start as `new` gives them, and
    /// whose `before_build` can add elements and resources. Its full name in
    /// errors is the Rust name of `S`.
    pub fn new<N, F>(new: N, before_build: F) -> Self
    where
        N: Fn() -> S + Send + Sync + 'static,
        F: Fn(S, &mut PipelineElements, &mut BuildContext) -> Result<()> + Send + Sync + 'static,
    {
        PipelineDefinition {
            full_name: type_name::<S>().to_string(),
            new: Box::new(new),
            settings: Vec::new(),
            before_build: Box::new(before_build),
        }
    }

    /// A setting called `name`, which reads its value as `T` and gives it
    /// to `apply`.
    pub fn setting<T, F>(mut self, name: impl Into<String>, apply: F) -> Self
    where
        T: ConfigType,
        F: Fn(S, T) -> S + Send + Sync + 'static,
    {
        self.settings
            .push(Setting::new(name.into(), move |state, value| {
                Ok(apply(state, value))
            }));
        self
    }
}

impl PipelineDefinition<()> {
    /// .NET's `PipelineBuilder`, which adds no settings and nothing before
    /// it builds.
    fn core() -> Self {
        PipelineDefinition {
            full_name: type_name::<PipelineBuilder>().to_string(),
            new: Box::new(|| ()),
            settings: Vec::new(),
            before_build: Box::new(|_, _, _| Ok(())),
        }
    }
}

/// A setting of the pipeline, being one every pipeline has or one the
/// registry's [`PipelineDefinition`] adds.
enum PipelineSetting<'a, S> {
    Core(&'a Setting<PipelineBuilder>),
    Added(&'a Setting<S>),
}

/// A pipeline setting whose value has been read, waiting for the builder.
enum PipelineReady<S> {
    Core(Pending<PipelineBuilder>),
    Added(Pending<S>),
}

impl<S> PipelineSetting<'_, S> {
    fn type_name(&self) -> &'static str {
        match self {
            PipelineSetting::Core(setting) => setting.type_name,
            PipelineSetting::Added(setting) => setting.type_name,
        }
    }

    fn prepare(&self, value: &ConfigValue) -> std::result::Result<PipelineReady<S>, ParseFailure> {
        match self {
            PipelineSetting::Core(setting) => (setting.prepare)(value).map(PipelineReady::Core),
            PipelineSetting::Added(setting) => (setting.prepare)(value).map(PipelineReady::Added),
        }
    }
}

impl<S> SettingNames for PipelineSetting<'_, S> {
    fn name(&self) -> &str {
        match self {
            PipelineSetting::Core(setting) => &setting.name,
            PipelineSetting::Added(setting) => &setting.name,
        }
    }

    fn alternate_names(&self) -> &[String] {
        match self {
            PipelineSetting::Core(setting) => &setting.alternate_names,
            PipelineSetting::Added(setting) => &setting.alternate_names,
        }
    }
}

/// An error making one element, with the cause .NET would give as the
/// inner exception.
struct ElementError {
    message: String,
    inner: Option<String>,
}

impl ElementError {
    fn new(message: impl Into<String>) -> Self {
        ElementError {
            message: message.into(),
            inner: None,
        }
    }
}

impl From<Error> for ElementError {
    fn from(error: Error) -> Self {
        ElementError::new(error.message().into_owned())
    }
}

/// A test of one way a name can match, tried in turn until one matches.
type Matches<'a, T> = &'a dyn Fn(&T) -> bool;

/// The settings read from a configuration, waiting for the builder, and
/// the upper case names of the build method's parameters.
type SettingsRead<B> = (Vec<Pending<B>>, Vec<String>);

/// The settings whose names match `key`, as .NET's pipeline finds a
/// builder's methods. The first of these to match anything wins, being the
/// name itself, the name with `Set` before it, and an alternate name equal
/// to either, all ignoring case.
fn matching_settings<'a, T: SettingNames>(settings: &'a [T], key: &str) -> Vec<&'a T> {
    let with_set = format!("Set{key}");
    let stages: [Matches<T>; 3] = [
        &|s| equal_ignoring_case(s.name(), key),
        &|s| equal_ignoring_case(s.name(), &with_set),
        &|s| {
            s.alternate_names()
                .iter()
                .any(|a| equal_ignoring_case(a, key) || equal_ignoring_case(a, &with_set))
        },
    ];
    stages
        .iter()
        .map(|stage| settings.iter().filter(|s| stage(s)).collect::<Vec<_>>())
        .find(|found| !found.is_empty())
        .unwrap_or_default()
}

/// Read `parameters` against `settings`, giving the settings read, in
/// order, and the upper case names of the parameters that are not
/// settings, which are a build method's.
fn read_settings<B>(
    settings: &[Setting<B>],
    parameters: &[(String, ConfigValue)],
    full_name: &str,
    location: &str,
) -> std::result::Result<SettingsRead<B>, ElementError> {
    let mut pending = Vec::new();
    let mut build_names = Vec::new();
    for (key, value) in parameters {
        let candidates = matching_settings(settings, key);
        if candidates.is_empty() {
            build_names.push(key.to_uppercase());
            continue;
        }
        let mut failure = None;
        for setting in &candidates {
            if setting.parameters != 1 {
                let takes = if setting.parameters == 0 {
                    "no parameters "
                } else {
                    "more than one parameter. "
                };
                return Err(ElementError::new(format!(
                    "Method '{}' on builder '{full_name}' for {location} takes {takes}This is \
                     not supported.",
                    setting.name
                )));
            }
            match (setting.prepare)(value) {
                Ok(ready) => {
                    pending.push(ready);
                    failure = None;
                    break;
                }
                Err(refused) => {
                    let ends_search = refused.ends_search();
                    failure = Some((setting, refused));
                    if ends_search {
                        break;
                    }
                }
            }
        }
        if let Some((setting, refused)) = failure {
            let prefix = format!(
                "Method '{}' on builder '{full_name}' for {location} expects a parameter of \
                 type '{}'",
                setting.name, setting.type_name
            );
            let (message, inner) = refused.describe(&prefix, value);
            return Err(ElementError { message, inner });
        }
    }
    Ok((pending, build_names))
}

trait ElementFactory: Send + Sync {
    /// The type of the builder's state, which with the name tells one
    /// builder from another.
    fn state_type(&self) -> TypeId;
    fn name(&self) -> &str;
    fn full_name(&self) -> &str;
    fn alternate_names(&self) -> &[String];
    fn create(
        &self,
        options: &ElementOptions,
        location: &str,
        context: &mut BuildContext,
    ) -> std::result::Result<Arc<dyn FlowElement>, ElementError>;
}

impl<B: Send + 'static> ElementFactory for BuilderDefinition<B> {
    fn state_type(&self) -> TypeId {
        TypeId::of::<B>()
    }

    fn name(&self) -> &str {
        &self.name
    }

    fn full_name(&self) -> &str {
        &self.full_name
    }

    fn alternate_names(&self) -> &[String] {
        &self.alternate_names
    }

    fn create(
        &self,
        options: &ElementOptions,
        location: &str,
        context: &mut BuildContext,
    ) -> std::result::Result<Arc<dyn FlowElement>, ElementError> {
        let full_name = &self.full_name;
        if self.build_methods.is_empty() {
            return Err(ElementError::new(format!(
                "Builder '{full_name}' for {location} has no 'Build' methods."
            )));
        }
        let parameters = &options.build_parameters;
        let (pending, build_names) =
            read_settings(&self.settings, parameters, full_name, location)?;

        let build_signatures: String = self
            .build_methods
            .iter()
            .map(|method| format!("{}\n", method.signature()))
            .collect();
        let possible: Vec<&BuildMethod<B>> = self
            .build_methods
            .iter()
            .filter(|method| {
                method.parameters.len() == build_names.len()
                    && method
                        .parameters
                        .iter()
                        .all(|p| build_names.contains(&p.name.to_uppercase()))
            })
            .collect();
        let method = match possible.as_slice() {
            [method] => *method,
            [] => {
                let setting_signatures: String = self
                    .settings
                    .iter()
                    .filter(|s| s.parameters == 1)
                    .map(|s| format!("{} ({})\n", s.name, s.type_name))
                    .collect();
                return Err(ElementError::new(format!(
                    "Builder '{full_name}' for {location} has no 'Set' methods or 'Build' \
                     methods that match (case-insensitively) the following parameters: {}. \
                     The available configuration methods on this builder are: \n\
                     {setting_signatures}The available 'Build' methods have the following \
                     signatures: \n{build_signatures}",
                    build_names.join(",")
                )));
            }
            _ => {
                return Err(ElementError::new(format!(
                    "Builder '{full_name}' for {location} has multiple 'Build' methods that \
                     match the following parameters: {}. Matching method signatures are: \n\
                     {build_signatures}",
                    build_names.join(",")
                )));
            }
        };

        let mut values = Vec::with_capacity(method.parameters.len());
        for parameter in &method.parameters {
            let (_, value) = parameters
                .iter()
                .find(|(key, _)| equal_ignoring_case(key, &parameter.name))
                .expect("every parameter of the chosen build method was given");
            // .NET casts a build parameter's value to text before reading
            // it as any other type, and a list given in code fails the cast.
            if matches!(value, ConfigValue::List(_)) && parameter.type_name != String::TYPE_NAME {
                return Err(ElementError::new(
                    "Unable to cast object of type 'System.String[]' to type 'System.String'.",
                ));
            }
            let parsed = (parameter.parse)(value).map_err(|refused| {
                let prefix = format!(
                    "Method 'Build' on builder '{full_name}' for {location} expects a \
                     parameter of type '{}'",
                    parameter.type_name
                );
                let (message, inner) = refused.describe(&prefix, value);
                ElementError { message, inner }
            })?;
            values.push((parameter.name.clone(), parsed));
        }

        let mut builder = (method.create)(&BuildArguments { values }, context)?;
        for apply in pending {
            builder = apply(builder)?;
        }
        Ok((self.finish)(builder, context)?)
    }
}

trait PipelineFactory: Send + Sync {
    /// Apply `parameters` to the pipeline's settings, `core` being those
    /// every pipeline has, then run the step before building, and build.
    fn build(
        &self,
        core: &[Setting<PipelineBuilder>],
        parameters: &[(String, ConfigValue)],
        builder: PipelineBuilder,
        elements: PipelineElements,
        context: BuildContext,
    ) -> Result<Arc<Pipeline>>;
}

impl<S: Send + 'static> PipelineFactory for PipelineDefinition<S> {
    fn build(
        &self,
        core: &[Setting<PipelineBuilder>],
        parameters: &[(String, ConfigValue)],
        mut builder: PipelineBuilder,
        mut elements: PipelineElements,
        mut context: BuildContext,
    ) -> Result<Arc<Pipeline>> {
        let settings: Vec<PipelineSetting<S>> = self
            .settings
            .iter()
            .map(PipelineSetting::Added)
            .chain(core.iter().map(PipelineSetting::Core))
            .collect();
        let mut state = (self.new)();
        for (key, value) in parameters {
            let candidates = matching_settings(&settings, key);
            if candidates.is_empty() {
                log::warn!(
                    "The pipeline has no setting matching the build parameter '{key}', so it is \
                     ignored."
                );
                continue;
            }
            let mut ready = None;
            let mut failure = None;
            for setting in candidates {
                match setting.prepare(value) {
                    Ok(prepared) => {
                        ready = Some(prepared);
                        break;
                    }
                    Err(refused) => failure = Some((setting, refused)),
                }
            }
            match (ready, failure) {
                (Some(PipelineReady::Core(apply)), _) => builder = apply(builder)?,
                (Some(PipelineReady::Added(apply)), _) => state = apply(state)?,
                (None, Some((setting, refused))) => {
                    let prefix = format!(
                        "Method '{}' on builder '{}' for pipeline expects a parameter of type \
                         '{}'",
                        setting.name(),
                        self.full_name,
                        setting.type_name()
                    );
                    let (message, _) = refused.describe(&prefix, value);
                    return Err(Error::configuration(message));
                }
                (None, None) => unreachable!("a candidate setting either applies or refuses"),
            }
        }

        (self.before_build)(state, &mut elements, &mut context)?;
        for step in elements.steps {
            builder = builder.add_parallel_elements(step);
        }
        for resource in context.resources {
            builder = builder.add_resource(resource);
        }
        builder.build()
    }
}

/// The element builders a configuration can name, and the settings of the
/// pipeline itself.
///
/// .NET finds every builder in the loaded assemblies by reflection. Rust
/// has no reflection, so each crate gives a [`BuilderDefinition`] for each
/// of its builders, and an application adds those of the crates it uses.
/// [`PipelineBuilder::build_from_configuration`] then makes a pipeline from
/// [`PipelineOptions`] with them.
///
/// The pipeline's own settings are `SetSuppressProcessException` and
/// `SetSuppressProcessExceptions`, which both set
/// [`PipelineBuilder::suppress_process_exceptions`], and
/// `SetAutoDisposeElements`, which is read and has no effect because a
/// pipeline always releases its elements when it is dropped. These are
/// .NET's `PipelineBuilder` settings. A [`PipelineDefinition`] given to
/// [`BuilderRegistry::set_pipeline`] adds its own, as a .NET class derived
/// from `PipelineBuilder`, such as `FiftyOnePipelineBuilder`, does.
pub struct BuilderRegistry {
    builders: Vec<Box<dyn ElementFactory>>,
    pipeline_settings: Vec<Setting<PipelineBuilder>>,
    pipeline: Box<dyn PipelineFactory>,
}

impl Default for BuilderRegistry {
    fn default() -> Self {
        BuilderRegistry::new()
    }
}

impl BuilderRegistry {
    /// A registry with no element builders, and the pipeline's own
    /// settings.
    pub fn new() -> Self {
        BuilderRegistry {
            builders: Vec::new(),
            pipeline_settings: vec![
                Setting::new(
                    "SetSuppressProcessException".to_string(),
                    |builder: PipelineBuilder, suppress: bool| {
                        Ok(builder.suppress_process_exceptions(suppress))
                    },
                ),
                Setting::new(
                    "SetSuppressProcessExceptions".to_string(),
                    |builder: PipelineBuilder, suppress: bool| {
                        Ok(builder.suppress_process_exceptions(suppress))
                    },
                ),
                Setting::new(
                    "SetAutoDisposeElements".to_string(),
                    |builder: PipelineBuilder, _: bool| Ok(builder),
                ),
            ],
            pipeline: Box::new(PipelineDefinition::core()),
        }
    }

    /// Make each pipeline as `definition` describes, adding its settings to
    /// the pipeline's own and running its step before the pipeline is
    /// built. It replaces any definition set before. Returns `self` for
    /// chaining.
    pub fn set_pipeline<S: Send + 'static>(
        &mut self,
        definition: PipelineDefinition<S>,
    ) -> &mut Self {
        self.pipeline = Box::new(definition);
        self
    }

    /// Add an element builder. Returns `self` for chaining.
    pub fn with<B: Send + 'static>(mut self, definition: BuilderDefinition<B>) -> Self {
        self.add(definition);
        self
    }

    /// Add an element builder.
    ///
    /// A builder already in the registry, being one with the same name and
    /// the same type of state, is not added again, as .NET finds each
    /// builder once however many of the loaded packages depend on it. So
    /// the builders of two crates that both add a third crate's builders
    /// can go in one registry.
    pub fn add<B: Send + 'static>(&mut self, definition: BuilderDefinition<B>) -> &mut Self {
        let added = self
            .builders
            .iter()
            .any(|b| b.state_type() == TypeId::of::<B>() && b.name() == definition.name);
        if !added {
            self.builders.push(Box::new(definition));
        }
        self
    }

    /// The names of the element builders, in the order they were added.
    pub fn builder_names(&self) -> Vec<&str> {
        self.builders.iter().map(|b| b.name()).collect()
    }

    /// The builder `name` gives, as .NET's pipeline finds it. The first of
    /// these to match anything wins, being the builder's name, the builder's
    /// name with `Builder` removed from the end, and an alternate name, all
    /// ignoring case.
    fn find(&self, name: &str) -> std::result::Result<Option<&dyn ElementFactory>, ElementError> {
        let with_builder = format!("{name}Builder");
        let stages: [Matches<dyn ElementFactory>; 3] = [
            &|b| equal_ignoring_case(b.name(), name),
            &|b| equal_ignoring_case(b.name(), &with_builder),
            &|b| {
                b.alternate_names()
                    .iter()
                    .any(|a| equal_ignoring_case(a, name))
            },
        ];
        for stage in stages {
            let found: Vec<&dyn ElementFactory> = self
                .builders
                .iter()
                .map(|b| b.as_ref())
                .filter(|b| stage(*b))
                .collect();
            match found.as_slice() {
                [] => continue,
                [one] => return Ok(Some(*one)),
                many => {
                    let names: Vec<&str> = many.iter().map(|b| b.full_name()).collect();
                    return Err(ElementError::new(format!(
                        "The flow element builder name '{name}' matches multiple builders: \
                         [{}].",
                        names.join(",")
                    )));
                }
            }
        }
        Ok(None)
    }

    fn create_element(
        &self,
        options: &ElementOptions,
        location: &str,
        context: &mut BuildContext,
    ) -> std::result::Result<Arc<dyn FlowElement>, ElementError> {
        let name = match options.builder_name.as_deref() {
            Some(name) if !name.is_empty() => name,
            _ => {
                return Err(ElementError::new(format!(
                    "A BuilderName must be specified for {location}."
                )))
            }
        };
        if let Some((first, _)) = options
            .build_parameters
            .iter()
            .enumerate()
            .flat_map(|(i, (a, _))| {
                options.build_parameters[..i]
                    .iter()
                    .filter(move |(b, _)| equal_ignoring_case(a, b))
            })
            .next()
        {
            return Err(ElementError::new(format!(
                "The build parameter '{first}' is given more than once for {location}."
            )));
        }
        let factory = self.find(name)?.ok_or_else(|| {
            ElementError::new(format!(
                "Could not find builder matching '{name}' for {location}. Available builders \
                 are: {}",
                self.builder_names().join(",")
            ))
        })?;
        factory.create(options, location, context)
    }

    fn create_group(
        &self,
        options: &ElementOptions,
        index: usize,
        context: &mut BuildContext,
    ) -> std::result::Result<Vec<Arc<dyn FlowElement>>, ElementError> {
        if options
            .builder_name
            .as_deref()
            .is_some_and(|n| !n.is_empty())
            || !options.build_parameters.is_empty()
        {
            return Err(ElementError::new(format!(
                "ElementOptions {index} contains both SubElements and other settings values. \
                 This is invalid"
            )));
        }
        let mut elements = Vec::with_capacity(options.sub_elements.len());
        for (sub, element) in options.sub_elements.iter().enumerate() {
            if !element.sub_elements.is_empty() {
                return Err(ElementError::new(format!(
                    "ElementOptions {index} contains nested sub elements. This is not \
                     supported."
                )));
            }
            let location = format!("element {sub} in element {index}");
            elements.push(self.create_element(element, &location, context)?);
        }
        Ok(elements)
    }
}

/// The message .NET's pipeline gives when an element of a configuration
/// cannot be made, naming the element and the cause.
fn informative_message(options: &ElementOptions, error: ElementError) -> String {
    let builder_name = options
        .builder_name
        .as_deref()
        .unwrap_or("BuilderName not set");
    let sub_names = if options.sub_elements.is_empty() {
        "No SubElements".to_string()
    } else {
        options
            .sub_elements
            .iter()
            .map(|e| e.builder_name.as_deref().unwrap_or(""))
            .collect::<Vec<_>>()
            .join(",")
    };
    format!(
        "Configuration failed to build.\nBuilderName: {builder_name}\nSubElement Builder \
         Names: {sub_names}\n{}\n{}\n",
        error.message,
        error.inner.as_deref().unwrap_or("No Inner Exception.")
    )
}

impl PipelineBuilder {
    /// Make the pipeline that `options` describes, finding each element's
    /// builder by name in `registry`, as .NET's
    /// `PipelineBuilder.BuildFromConfiguration` does. See the
    /// [pipeline configuration specification](https://github.com/51Degrees/specifications/blob/main/pipeline-specification/features/pipeline-configuration.md).
    ///
    /// Elements already added to this builder are discarded, as .NET
    /// discards them. Its resources are given to the element builders
    /// through the [`BuildContext`]. Each element with `sub_elements` is a
    /// parallel group of them, added with
    /// [`PipelineBuilder::add_parallel_elements`], and a group cannot hold
    /// another. The pipeline's own settings are applied after the elements
    /// are made, and one the registry does not have is ignored, as .NET
    /// ignores it, with a warning logged. Then the step of the
    /// [`PipelineDefinition`] given to [`BuilderRegistry::set_pipeline`]
    /// runs, if there is one, and the pipeline is built.
    ///
    /// An element that cannot be made fails the build with a
    /// [`Error::PipelineConfiguration`] whose message is the one .NET gives,
    /// and the elements already made are dropped.
    pub fn build_from_configuration(
        mut self,
        registry: &BuilderRegistry,
        options: &PipelineOptions,
    ) -> Result<Arc<Pipeline>> {
        let mut context = BuildContext {
            resources: self.reset_for_configuration(),
        };
        let mut elements = PipelineElements {
            steps: Vec::with_capacity(options.elements.len()),
        };
        for (index, element) in options.elements.iter().enumerate() {
            let made = if element.sub_elements.is_empty() {
                registry
                    .create_element(element, &format!("element {index}"), &mut context)
                    .map(|made| vec![made])
            } else {
                registry.create_group(element, index, &mut context)
            };
            match made {
                Ok(made) => elements.steps.push(made),
                Err(error) => {
                    return Err(Error::configuration(informative_message(element, error)));
                }
            }
        }
        registry.pipeline.build(
            &registry.pipeline_settings,
            &options.build_parameters,
            self,
            elements,
            context,
        )
    }
}
