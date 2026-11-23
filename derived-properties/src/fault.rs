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

//! What is wrong with a script.
//!
//! Every fault carries the script name, where the script came from, a path in
//! the document such as `Rules[3].When.All[1]`, a line where the parser can
//! supply one, and a plain message. All the faults in one script are collected
//! and reported together, so one build shows everything wrong with a file
//! rather than stopping at the first problem.

use std::fmt;

/// One thing wrong with a script.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fault {
    /// The file name the script was read under, without its extension, or
    /// `None` where the script came from a string with no name attached.
    pub script: Option<String>,
    /// Where the script came from: a path, `built in` and the name of a script
    /// shipped in this crate, or `code`.
    pub source: String,
    /// The place in the document, such as `Rules[3].When.All[1]`. Empty for a
    /// fault about the document as a whole, such as text that will not parse.
    pub path: String,
    /// The one-based line, where the parser can supply one.
    ///
    /// This implementation supplies a line for a parse fault only, because the
    /// YAML reader it uses hands back a value tree without node positions.
    /// Format 1 allows the line to differ between languages and to be absent,
    /// and the path is the same in every language either way.
    pub line: Option<usize>,
    /// What is wrong, in plain words.
    pub message: String,
}

impl fmt::Display for Fault {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let where_ = if self.path.is_empty() {
            "(document)"
        } else {
            self.path.as_str()
        };
        let script = self.script.as_deref().unwrap_or("script");
        write!(f, "{script} ({})", self.source)?;
        if let Some(line) = self.line {
            write!(f, " line {line}")?;
        }
        write!(f, " at {where_}: {}", self.message)
    }
}

/// Every fault found in one script, which is what rejecting a script returns.
///
/// Displays as one line per fault, in the order they were found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Faults(Vec<Fault>);

impl Faults {
    /// Build a set of faults from the faults found.
    pub fn new(faults: Vec<Fault>) -> Self {
        Faults(faults)
    }

    /// The faults, in the order they were found.
    pub fn faults(&self) -> &[Fault] {
        &self.0
    }

    /// The number of faults.
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// True where there is nothing wrong.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Take the faults out, keeping the allocation.
    pub fn into_inner(self) -> Vec<Fault> {
        self.0
    }
}

impl fmt::Display for Faults {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (index, fault) in self.0.iter().enumerate() {
            if index > 0 {
                writeln!(f)?;
            }
            write!(f, "{fault}")?;
        }
        Ok(())
    }
}

impl std::error::Error for Faults {}
