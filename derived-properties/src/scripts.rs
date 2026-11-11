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

//! The scripts 51Degrees ships, compiled into the crate.
//!
//! The text of each script is embedded at build time rather than read from a
//! file, so an element carrying one runs where there is no file system and
//! where fetching a file over the network would defeat the purpose, such as in
//! a WebAssembly host or at an edge runtime. A caller that wants a script of
//! its own passes the text in instead, which
//! [`Script::compile`](crate::Script::compile) reads.

use crate::fault::Faults;
use crate::model::Script;

/// What the source of a shipped script starts with, followed by its name.
pub(crate) const BUILT_IN_SOURCE: &str = "built in";

/// The commit of 51Degrees/derived-properties the scripts under `vendor` come
/// from, which the element logs at build so that anyone holding the log can
/// find the exact scripts that ran. `build.rs` says where it is found.
pub(crate) fn built_in_scripts_commit() -> &'static str {
    env!("FIFTYONE_DERIVED_PROPERTIES_COMMIT")
}

/// The names of the shipped scripts, joined by ", ", as messages list them.
pub(crate) fn built_in_names() -> String {
    BuiltInScript::ALL
        .iter()
        .map(|script| script.name())
        .collect::<Vec<_>>()
        .join(", ")
}

/// A script shipped inside this crate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuiltInScript {
    /// The confidence that the request came from a device being used by a
    /// human who is viewing the page, being one of High, Medium, Low or
    /// Unknown. Unknown is the answer until the 51Degrees JavaScript has run.
    ///
    /// The script is a draft below version 1.0.0, so its thresholds may be
    /// rewritten. Its own comment header says what each one rests on.
    HumanConfidence,
}

impl BuiltInScript {
    /// Every shipped script.
    pub const ALL: &'static [BuiltInScript] = &[BuiltInScript::HumanConfidence];

    /// What configuration selects the script by, which is also the name of the
    /// property it produces.
    pub fn name(self) -> &'static str {
        match self {
            BuiltInScript::HumanConfidence => "HumanConfidence",
        }
    }

    /// The text of the script, as it is written in the shared repository.
    pub fn text(self) -> &'static str {
        match self {
            BuiltInScript::HumanConfidence => {
                include_str!("../vendor/scripts/HumanConfidence.yaml")
            }
        }
    }

    /// Read the script.
    ///
    /// A shipped script is checked by the tests of this crate, so this only
    /// returns faults if the script in `vendor` has been edited.
    pub fn compile(self) -> Result<Script, Faults> {
        Script::compile_named(
            self.text(),
            Some(self.name()),
            &format!("{BUILT_IN_SOURCE} {}", self.name()),
        )
    }

    /// The shipped script of that name, matched without regard to letter case,
    /// which is how configuration names one.
    pub fn named(name: &str) -> Option<BuiltInScript> {
        BuiltInScript::ALL
            .iter()
            .copied()
            .find(|script| script.name().eq_ignore_ascii_case(name))
    }
}
