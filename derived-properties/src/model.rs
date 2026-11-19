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

//! The model, which is what a script becomes once it has been read.
//!
//! Every language builds the same model from the same script, and everything
//! after parsing works on the model rather than on the text. The types here
//! are the Rust form of it.

use std::fmt;

/// The four value types format 1 handles.
///
/// The same four are also the types a source property can be read as, which
/// the literal it is compared against decides.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ValueType {
    /// Text.
    String,
    /// True or false.
    Bool,
    /// A signed 32 bit whole number. The width is fixed by the format so that
    /// one script gives one answer in every language.
    Int,
    /// A number that may carry a fractional part.
    Double,
}

impl ValueType {
    /// The name the format writes the type as, which is also the name that
    /// appears in a fault message.
    pub fn as_str(self) -> &'static str {
        match self {
            ValueType::String => "string",
            ValueType::Bool => "bool",
            ValueType::Int => "int",
            ValueType::Double => "double",
        }
    }
}

impl fmt::Display for ValueType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// The smallest value the type `int` holds.
pub const INT_MIN: i64 = -2147483648;
/// The largest value the type `int` holds.
pub const INT_MAX: i64 = 2147483647;

/// A value written in a script, either to compare against or to return.
#[derive(Debug, Clone, PartialEq)]
pub enum Literal {
    /// True or false.
    Bool(bool),
    /// A whole number inside the range of `int`.
    Int(i32),
    /// A number with a fractional part, or one too large for `int`.
    Double(f64),
    /// Text.
    Text(String),
}

impl Literal {
    /// The type this literal infers for the property it is compared against.
    pub fn value_type(&self) -> ValueType {
        match self {
            Literal::Bool(_) => ValueType::Bool,
            Literal::Int(_) => ValueType::Int,
            Literal::Double(_) => ValueType::Double,
            Literal::Text(_) => ValueType::String,
        }
    }

    /// The text form, which is how a rule value is matched against the entries
    /// under `Output.Values` and how the value is printed.
    ///
    /// A whole number held as a `Double`, which is what a number too large for
    /// `int` becomes, prints without a decimal point, so 8.0 is `8`. That is
    /// the same text every other language implementation produces.
    pub fn to_text(&self) -> String {
        match self {
            Literal::Bool(value) => {
                if *value {
                    "true".to_owned()
                } else {
                    "false".to_owned()
                }
            }
            Literal::Int(value) => value.to_string(),
            Literal::Double(value) => format_number(*value),
            Literal::Text(value) => value.clone(),
        }
    }
}

impl fmt::Display for Literal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_text())
    }
}

/// Print a number the way every other language implementation prints it, which
/// is without a decimal point where the value is whole.
pub(crate) fn format_number(value: f64) -> String {
    if value.is_finite() && value.fract() == 0.0 && value.abs() < 1e15 {
        format!("{}", value as i64)
    } else {
        format!("{value}")
    }
}

/// One entry of the value list a property publishes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutputValue {
    /// The value itself, held as text because a value name may be written as a
    /// whole number and is compared as text.
    pub name: String,
    /// What the value asserts, which is the wording a customer reads.
    pub description: Option<String>,
}

/// The property definition, which is the `Output` block of a script.
///
/// The field names and meanings are those 51Degrees uses for the metadata of
/// every property. There is one definition of a derived property and it is
/// this block, so nothing here is a copy of anything held elsewhere.
#[derive(Debug, Clone, PartialEq)]
pub struct Output {
    /// The property name published under the `derived` element data key.
    pub name: String,
    /// What the property asserts.
    pub description: String,
    /// The type of the value the property returns.
    pub value_type: ValueType,
    /// The stored value type, carried through unchanged.
    pub stored_value_type: Option<String>,
    /// The text form of the default recorded for the property.
    ///
    /// Metadata and nothing more. Nothing reads it while a request is being
    /// processed, because every script ends in an `Else` and so always chooses
    /// a value from its own rules.
    pub default_value: Option<String>,
    /// Always false in format 1, because list outputs are deferred.
    pub is_list: bool,
    /// Whether the property always has a value.
    pub is_mandatory: Option<bool>,
    /// Whether the property is no longer maintained.
    pub is_obsolete: Option<bool>,
    /// The category the property is grouped under.
    pub category: Option<String>,
    /// Whether the property is one of the popular ones.
    pub is_popular: Option<bool>,
    /// Whether the value list is exported with the property.
    pub export_values: Option<bool>,
    /// A link to more about the property.
    pub url: Option<String>,
    /// Where the property sits in a display order.
    pub display_order: Option<i64>,
    /// The 51Degrees identifier of the property.
    pub property_id: Option<i64>,
    /// The vendors the property applies to, carried through unchanged.
    pub vendor_ids: Vec<String>,
    /// Every source property the checks and the rules name, in
    /// `elementKey.PropertyName` form. Computed where the script does not give
    /// it.
    pub dependencies: Vec<String>,
    /// The values the property can return, where the script lists them.
    pub values: Option<Vec<OutputValue>>,
}

/// A property another element produced, which a condition names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceProperty {
    /// The full name as the script writes it, such as `device.IsCrawler`.
    pub name: String,
    /// The short name the element that supplies the property publishes its
    /// results under, such as `device`.
    pub element_key: String,
    /// The name of the property on that element, such as `IsCrawler`.
    pub property_name: String,
    /// The type the property is read as, which the literals it is compared
    /// against decide. Every use of a property across one script infers the
    /// same type, so this is a fact about the property rather than about each
    /// place the property is used.
    pub value_type: ValueType,
}

/// The two counts a condition can compare a group of checks by.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Aggregate {
    /// How many checks in the group came out true.
    Passed,
    /// How many checks in the group came out false.
    Failed,
}

impl Aggregate {
    /// The name the format writes the count as.
    pub fn as_str(self) -> &'static str {
        match self {
            Aggregate::Passed => "Passed",
            Aggregate::Failed => "Failed",
        }
    }
}

/// The operators a comparison can use.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompareOp {
    /// Equal. Text compares ordinally and with regard to letter case.
    Eq,
    /// Not equal, on the same terms as [`CompareOp::Eq`].
    Ne,
    /// Greater than.
    Gt,
    /// Greater than or equal to.
    Ge,
    /// Less than.
    Lt,
    /// Less than or equal to.
    Le,
    /// The value is one of the members of a non empty list.
    In,
    /// The value is none of the members of a non empty list.
    NotIn,
    /// Text starts with the value, ordinally and with regard to letter case.
    StartsWith,
    /// Text ends with the value, on the same terms.
    EndsWith,
    /// Text holds the value, on the same terms.
    Contains,
}

impl CompareOp {
    /// The name the format writes the operator as.
    pub fn as_str(self) -> &'static str {
        match self {
            CompareOp::Eq => "Eq",
            CompareOp::Ne => "Ne",
            CompareOp::Gt => "Gt",
            CompareOp::Ge => "Ge",
            CompareOp::Lt => "Lt",
            CompareOp::Le => "Le",
            CompareOp::In => "In",
            CompareOp::NotIn => "NotIn",
            CompareOp::StartsWith => "StartsWith",
            CompareOp::EndsWith => "EndsWith",
            CompareOp::Contains => "Contains",
        }
    }

    /// The types the operator is allowed on.
    ///
    /// `Gt`, `Ge`, `Lt` and `Le` are deliberately kept off text, because the
    /// order two strings sort in differs between languages and a script must
    /// give the same answer everywhere.
    pub fn allowed_on(self) -> &'static [ValueType] {
        use ValueType::{Bool, Double, Int, String};
        match self {
            CompareOp::Eq | CompareOp::Ne | CompareOp::In | CompareOp::NotIn => {
                &[Bool, Int, Double, String]
            }
            CompareOp::Gt | CompareOp::Ge | CompareOp::Lt | CompareOp::Le => &[Int, Double],
            CompareOp::StartsWith | CompareOp::EndsWith | CompareOp::Contains => &[String],
        }
    }

    /// Every operator, in the order a fault message lists them.
    pub const ALL: &'static [CompareOp] = &[
        CompareOp::Eq,
        CompareOp::Ne,
        CompareOp::Gt,
        CompareOp::Ge,
        CompareOp::Lt,
        CompareOp::Le,
        CompareOp::In,
        CompareOp::NotIn,
        CompareOp::StartsWith,
        CompareOp::EndsWith,
        CompareOp::Contains,
    ];
}

/// What a comparison compares against, being one literal, or the list that
/// `In` and `NotIn` take.
#[derive(Debug, Clone, PartialEq)]
pub enum Operand {
    /// One value.
    One(Literal),
    /// A non empty list of values, for `In` and `NotIn`.
    Many(Vec<Literal>),
}

/// A true or false test.
#[derive(Debug, Clone, PartialEq)]
pub enum Condition {
    /// A source property compared against a literal.
    Compare {
        /// Which of the script's source properties is read, as an index into
        /// [`Script::source_properties`].
        property: usize,
        /// The operator.
        op: CompareOp,
        /// What the value is compared against.
        operand: Operand,
        /// The type both sides are read as.
        value_type: ValueType,
    },
    /// The result a named check already produced, as an index into
    /// [`Script::checks`].
    Check(usize),
    /// A count of checks compared with a whole number.
    Aggregate {
        /// Which count is taken.
        aggregate: Aggregate,
        /// The checks counted, as indexes into [`Script::checks`], or `None`
        /// for every check the script defines.
        group: Option<Vec<usize>>,
        /// The operator, which is one of `Eq`, `Ne`, `Gt`, `Ge`, `Lt` or `Le`.
        op: CompareOp,
        /// The whole number the count is compared with.
        operand: i64,
    },
    /// True where every member is true.
    All(Vec<Condition>),
    /// True where at least one member is true.
    Any(Vec<Condition>),
    /// True becomes false and false becomes true.
    Not(Box<Condition>),
}

/// A named test, which a rule can reuse and an aggregate can count.
#[derive(Debug, Clone, PartialEq)]
pub struct Check {
    /// The name, matched exactly as written wherever it is referenced.
    pub name: String,
    /// The test.
    pub condition: Condition,
}

/// One rule, read in order until one matches.
#[derive(Debug, Clone, PartialEq)]
pub struct Rule {
    /// The condition, or `None` for the `Else` that ends every script and
    /// always matches.
    pub when: Option<Condition>,
    /// The value the rule supplies.
    pub value: Literal,
}

/// A script that has been read and found sound, ready to run.
///
/// Build one with [`Script::compile`](crate::Script::compile) or take a
/// shipped one from [`BuiltInScript`](crate::BuiltInScript).
#[derive(Debug, Clone, PartialEq)]
pub struct Script {
    pub(crate) name: String,
    pub(crate) version: String,
    pub(crate) deprecated: bool,
    pub(crate) deprecation_note: Option<String>,
    pub(crate) source: String,
    pub(crate) output: Output,
    /// Whether the script wrote `VendorIds`, even as an empty list, which the
    /// canonical form needs because [`Output::vendor_ids`] cannot say.
    pub(crate) vendor_ids_written: bool,
    pub(crate) properties: Vec<SourceProperty>,
    pub(crate) checks: Vec<Check>,
    pub(crate) rules: Vec<Rule>,
}

impl Script {
    /// What configuration selects the script by, which equals the file name
    /// without its extension.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The author's semantic version. It is printed in the build log and plays
    /// no part in selecting a script.
    pub fn version(&self) -> &str {
        &self.version
    }

    /// Whether the script still works but should be replaced.
    pub fn is_deprecated(&self) -> bool {
        self.deprecated
    }

    /// What to use instead, where the script is deprecated.
    pub fn deprecation_note(&self) -> Option<&str> {
        self.deprecation_note.as_deref()
    }

    /// Where the script came from: a path, `built in` and the name of a script
    /// shipped in this crate, or `code`.
    pub fn source(&self) -> &str {
        &self.source
    }

    /// The property definition.
    pub fn output(&self) -> &Output {
        &self.output
    }

    /// Every source property the script names, in the order the script first
    /// named them, with the checks read before the rules.
    pub fn source_properties(&self) -> &[SourceProperty] {
        &self.properties
    }

    /// The named checks, in the order the script defines them.
    pub fn checks(&self) -> &[Check] {
        &self.checks
    }

    /// The rules, in the order they are read.
    pub fn rules(&self) -> &[Rule] {
        &self.rules
    }
}
