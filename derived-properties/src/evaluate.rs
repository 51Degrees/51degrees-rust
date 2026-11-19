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

//! Running one script for one request.
//!
//! There is exactly one way to get no value and exactly one way to get a
//! value. Where every source property the script names is available the checks
//! and the rules run and a value is chosen, and where any one of them is not
//! available the script produces no value and the message names every property
//! that was missing.

use crate::model::{Aggregate, CompareOp, Condition, Literal, Operand, Script};
use crate::source::{convert, display_source_value, Lookup, PropertySource, Reading, SourceValue};

/// The sentence every no value message ends with, naming the things that
/// usually cause a source property to be missing.
pub const USUAL_CAUSES: &str = "Usual causes are the element that supplies the property not \
                                being in the pipeline, or being added after this element rather \
                                than before it, the property being excluded in the engine \
                                configuration, the property not being included in the resource \
                                key, or JavaScript that populates the property not having run \
                                yet.";

/// What running a script for one request produced.
#[derive(Debug, Clone, PartialEq)]
pub enum Outcome {
    /// The value a rule chose.
    Value(Literal),
    /// No value, because a source property the script names could not be read.
    NoValue {
        /// Every property that could not be read, in the order the script
        /// first named them.
        missing: Vec<String>,
        /// The message the output property carries, which names each missing
        /// property and what its source element said about it.
        message: String,
    },
}

/// What running a script produced, with the working shown.
///
/// [`Script::evaluate`] answers the value alone, which is all a pipeline
/// needs. This answers the checks and the rule as well, which is what tells
/// somebody why a request came out Medium rather than High.
#[derive(Debug, Clone, PartialEq)]
pub struct Evaluation {
    /// The value, or the reason there is none.
    pub outcome: Outcome,
    /// What each named check came out as, in the order the script defines
    /// them. Empty where a source property could not be read, because nothing
    /// is evaluated then.
    pub checks: Vec<bool>,
    /// The rule that supplied the value, as an index into
    /// [`Script::rules`]. `None` where there is no value.
    pub matched_rule: Option<usize>,
}

impl Script {
    /// Run this script for one request.
    ///
    /// Every source property the script names is read first. Where any one of
    /// them could not be read nothing is evaluated, which is the only way a
    /// script produces no value.
    pub fn evaluate(&self, source: &dyn PropertySource) -> Outcome {
        self.evaluate_detailed(source).outcome
    }

    /// Run this script for one request and answer the working as well as the
    /// value.
    pub fn evaluate_detailed(&self, source: &dyn PropertySource) -> Evaluation {
        let mut readings: Vec<Option<Reading>> = Vec::with_capacity(self.properties.len());
        let mut missing: Vec<String> = Vec::new();
        let mut reasons: Vec<String> = Vec::new();
        for property in &self.properties {
            match read_one(
                source,
                &property.element_key,
                &property.property_name,
                property.value_type,
            ) {
                Ok(reading) => readings.push(Some(reading)),
                Err(reason) => {
                    readings.push(None);
                    missing.push(property.name.clone());
                    reasons.push(reason);
                }
            }
        }
        if !missing.is_empty() {
            let message = missing_message(&self.output.name, &missing, &reasons);
            return Evaluation {
                outcome: Outcome::NoValue { missing, message },
                checks: Vec::new(),
                matched_rule: None,
            };
        }

        // Every check is evaluated once, before any rule is read, and a check
        // reference then reads the result already worked out.
        let readings: Vec<Reading> = readings.into_iter().flatten().collect();
        let mut states: Vec<bool> = Vec::with_capacity(self.checks.len());
        for check in &self.checks {
            let state = evaluate_condition(&check.condition, &readings, &states);
            states.push(state);
        }

        for (index, rule) in self.rules.iter().enumerate() {
            let matched = match &rule.when {
                // An Else has no condition and always matches.
                None => true,
                Some(condition) => evaluate_condition(condition, &readings, &states),
            };
            if matched {
                return Evaluation {
                    outcome: Outcome::Value(rule.value.clone()),
                    checks: states,
                    matched_rule: Some(index),
                };
            }
        }

        // Every script ends in an Else, which validation enforces, so the loop
        // above always returns. Reaching here would mean a model built by hand
        // rather than by the validator, and saying so beats a panic.
        Evaluation {
            outcome: Outcome::NoValue {
                missing: Vec::new(),
                message: format!(
                    "Derived property '{}' has no value because its rules do not end in an \
                     Else, which format 1 does not allow.",
                    self.output.name
                ),
            },
            checks: states,
            matched_rule: None,
        }
    }
}

/// Read one source property for one request, converting it to the type the
/// script inferred for it. The error is the reason the property is absent,
/// worded as the message that carries it needs.
fn read_one(
    source: &dyn PropertySource,
    element_key: &str,
    property_name: &str,
    value_type: crate::model::ValueType,
) -> Result<Reading, String> {
    // The first two shapes name the element and the property, because the
    // message is being carried up from the source element. The last two do
    // not repeat the name, because the failure happened while converting a
    // value that was found.
    let from_source = |detail: &str| {
        format!("element '{element_key}' has no value for '{property_name}': {detail}")
    };
    let value = match source.lookup(element_key, property_name) {
        Lookup::Absent => return Err(from_source("property not present on this request")),
        Lookup::NoValue(message) => return Err(from_source(&message)),
        Lookup::List => return Err("held a list where a single value is needed".to_owned()),
        Lookup::Weighted(candidates) => match highest_weighted(candidates) {
            Some(value) => value,
            None => return Err("held a list where a single value is needed".to_owned()),
        },
        Lookup::Value(value) => value,
    };
    match convert(&value, value_type) {
        Some(reading) => Ok(reading),
        None => Err(format!(
            "held '{}' which cannot be read as {value_type}",
            display_source_value(&value)
        )),
    }
}

/// The value with the highest weight, where the first of two equal weights
/// wins. An empty list is a list where a single value is needed.
fn highest_weighted(candidates: Vec<crate::source::WeightedSourceValue>) -> Option<SourceValue> {
    let mut best: Option<crate::source::WeightedSourceValue> = None;
    for candidate in candidates {
        let better = match &best {
            None => true,
            Some(current) => candidate.weight > current.weight,
        };
        if better {
            best = Some(candidate);
        }
    }
    best.map(|candidate| candidate.value)
}

fn missing_message(output_name: &str, missing: &[String], reasons: &[String]) -> String {
    let noun = if missing.len() == 1 {
        "1 source property was not available".to_owned()
    } else {
        format!("{} source properties were not available", missing.len())
    };
    let details: Vec<String> = missing
        .iter()
        .zip(reasons)
        .map(|(name, reason)| format!("'{name}' ({reason})."))
        .collect();
    format!(
        "Derived property '{output_name}' has no value because {noun}. {} {USUAL_CAUSES}",
        details.join(" ")
    )
}

/// Every condition is true or false, because the rules only run once every
/// source property the script names has been read.
fn evaluate_condition(condition: &Condition, readings: &[Reading], states: &[bool]) -> bool {
    match condition {
        Condition::Compare {
            property,
            op,
            operand,
            ..
        } => match readings.get(*property) {
            Some(reading) => compare(reading, *op, operand),
            None => false,
        },
        // A check reference reads the result already worked out. A reference
        // can only name a check defined before the rules run, so a state that
        // is not there would be a model built by hand.
        Condition::Check(index) => states.get(*index).copied().unwrap_or(false),
        Condition::Aggregate {
            aggregate,
            group,
            op,
            operand,
        } => {
            let count = count_of(*aggregate, group.as_deref(), states);
            compare_numbers(count as f64, *op, *operand as f64)
        }
        Condition::All(items) => items
            .iter()
            .all(|item| evaluate_condition(item, readings, states)),
        Condition::Any(items) => items
            .iter()
            .any(|item| evaluate_condition(item, readings, states)),
        Condition::Not(item) => !evaluate_condition(item, readings, states),
    }
}

/// Count checks in a group. `None` means every named check. Every check is
/// true or false, so `Passed` and `Failed` always add up to the size of the
/// group.
fn count_of(aggregate: Aggregate, group: Option<&[usize]>, states: &[bool]) -> usize {
    let wanted = aggregate == Aggregate::Passed;
    match group {
        None => states.iter().filter(|state| **state == wanted).count(),
        Some(indexes) => indexes
            .iter()
            .filter(|index| states.get(**index).copied() == Some(wanted))
            .count(),
    }
}

/// Compare a value against what a script wrote. Validation has already refused
/// every operator that does not suit the type, so a pairing that cannot happen
/// is false rather than an error.
fn compare(reading: &Reading, op: CompareOp, operand: &Operand) -> bool {
    match (reading, operand) {
        (Reading::Bool(value), Operand::One(Literal::Bool(other))) => match op {
            CompareOp::Eq => value == other,
            CompareOp::Ne => value != other,
            _ => false,
        },
        (Reading::Int(value), Operand::One(Literal::Int(other))) => {
            compare_numbers(f64::from(*value), op, f64::from(*other))
        }
        (Reading::Double(value), Operand::One(Literal::Double(other))) => {
            compare_numbers(*value, op, *other)
        }
        (Reading::Text(value), Operand::One(Literal::Text(other))) => match op {
            CompareOp::Eq => value == other,
            CompareOp::Ne => value != other,
            CompareOp::StartsWith => value.starts_with(other.as_str()),
            CompareOp::EndsWith => value.ends_with(other.as_str()),
            CompareOp::Contains => value.contains(other.as_str()),
            _ => false,
        },
        (reading, Operand::Many(members)) => {
            let found = members.iter().any(|member| same(reading, member));
            match op {
                CompareOp::In => found,
                CompareOp::NotIn => !found,
                _ => false,
            }
        }
        _ => false,
    }
}

/// Whether a value read from a request is the same as a literal in the script.
/// Both are of the type the script inferred, which validation settled.
fn same(reading: &Reading, literal: &Literal) -> bool {
    match (reading, literal) {
        (Reading::Bool(value), Literal::Bool(other)) => value == other,
        (Reading::Int(value), Literal::Int(other)) => value == other,
        (Reading::Double(value), Literal::Double(other)) => value == other,
        (Reading::Text(value), Literal::Text(other)) => value == other,
        _ => false,
    }
}

fn compare_numbers(left: f64, op: CompareOp, right: f64) -> bool {
    match op {
        CompareOp::Eq => left == right,
        CompareOp::Ne => left != right,
        CompareOp::Gt => left > right,
        CompareOp::Ge => left >= right,
        CompareOp::Lt => left < right,
        CompareOp::Le => left <= right,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::Outcome;
    use crate::{MapSource, Script};

    /// Only a model changed after validation can lack the Else, and running it
    /// says so rather than panicking.
    #[test]
    fn rules_without_an_else_give_no_value() {
        let mut script = Script::compile(
            "format: 1\n\
             name: Demo\n\
             version: \"1.0.0\"\n\
             output:\n\
             \x20 name: Demo\n\
             \x20 description: A demonstration.\n\
             \x20 valuetype: string\n\
             \x20 islist: false\n\
             rules:\n\
             \x20 - else: only\n",
        )
        .expect("the script");
        script.rules.clear();
        assert_eq!(
            script.evaluate(&MapSource::new()),
            Outcome::NoValue {
                missing: Vec::new(),
                message: "Derived property 'Demo' has no value because its rules do not \
                          end in an Else, which format 1 does not allow."
                    .to_owned(),
            }
        );
    }
}
