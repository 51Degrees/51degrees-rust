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

//! Terms Document Locator (TDL) sources: short ids a caller can give in
//! place of a TDL URI, and where to find the latest URI for each.

use std::collections::HashMap;
use std::path::Path;

use fiftyone_pipeline_core::{Error, Result};
use regex::Regex;
use serde_json::{Map, Value};
use url::Url;

/// The name of the group in [`TdlSource::file_pattern`] that captures the
/// version number of a file.
pub const VERSION_GROUP_NAME: &str = "n";

/// The placeholder in [`TdlSource::url_template`] that the latest file's name
/// replaces.
pub const NAME_PLACEHOLDER: &str = "{name}";

/// The TDL sources compiled in, which [`TdlSources::load_default`] reads.
const DEFAULT_SOURCES: &str = include_str!("tdl-sources.json");

/// What a caller gives in place of a TDL URI to mean the latest file in a
/// GitHub folder of versioned terms documents, and how that file's public
/// URI is formed.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct TdlSource {
    /// The id a caller gives, such as `MOW-SOCW`, matched ignoring case.
    pub id: String,
    /// The GitHub repository, as `owner/name`.
    pub repository: String,
    /// The folder in the repository that holds the versioned files.
    pub directory: String,
    /// A regular expression over file names, with a group named `n` that
    /// captures the version number. The highest number is the latest file.
    pub file_pattern: String,
    /// The public URI of a file, with `{name}` where the file's name goes.
    pub url_template: String,
}

impl TdlSource {
    /// A source with each of its parts. [`TdlSources`] checks them.
    pub fn new(
        id: impl Into<String>,
        repository: impl Into<String>,
        directory: impl Into<String>,
        file_pattern: impl Into<String>,
        url_template: impl Into<String>,
    ) -> Self {
        TdlSource {
            id: id.into(),
            repository: repository.into(),
            directory: directory.into(),
            file_pattern: file_pattern.into(),
            url_template: url_template.into(),
        }
    }
}

/// A checked list of TDL sources, looked up by id ignoring case.
///
/// Read from the JSON form .NET's `TdlSourcesLoader` reads, an array of
/// objects with the members `Id`, `Repository`, `Directory`, `FilePattern`
/// and `UrlTemplate`, whose names are matched ignoring case. Every source is
/// checked when the list is read, so a mistake fails at start-up rather than
/// when a request names the source.
#[derive(Debug, Clone, Default)]
pub struct TdlSources {
    /// Each source and its compiled file pattern, keyed by its id in lower
    /// case.
    by_id: HashMap<String, (TdlSource, Regex)>,
}

impl TdlSources {
    /// The sources compiled into this crate, which name the Movement for an
    /// Open Web's Standard Online Content Wording as `MOW-SOCW`.
    pub fn load_default() -> Result<Self> {
        Self::parse(DEFAULT_SOURCES, "embedded:tdl-sources.json")
    }

    /// The sources in the JSON file at `path`, to use in place of the
    /// default list.
    pub fn load_from_file(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        if path.as_os_str().is_empty() || path.to_string_lossy().trim().is_empty() {
            return Err(Error::configuration("File path must be supplied."));
        }
        if !path.is_file() {
            return Err(Error::configuration(format!(
                "TDL sources file not found at '{}'.",
                path.display()
            )));
        }
        let json = std::fs::read_to_string(path).map_err(|error| {
            Error::configuration(format!(
                "TDL sources file '{}' could not be read: {error}",
                path.display()
            ))
        })?;
        Self::parse(&json, &path.display().to_string())
    }

    /// The sources in the JSON text given.
    pub fn load_from_json(json: &str) -> Result<Self> {
        Self::parse(json, "<inline>")
    }

    /// The source with the id given, matched ignoring case.
    pub fn get(&self, id: &str) -> Option<&TdlSource> {
        self.by_id.get(&id.to_lowercase()).map(|(source, _)| source)
    }

    /// True if a source has the id given, matched ignoring case.
    pub fn contains(&self, id: &str) -> bool {
        !id.is_empty() && self.by_id.contains_key(&id.to_lowercase())
    }

    /// The number of sources.
    pub fn len(&self) -> usize {
        self.by_id.len()
    }

    /// True if there are no sources.
    pub fn is_empty(&self) -> bool {
        self.by_id.is_empty()
    }

    /// The source with the id given and its compiled file pattern.
    pub(crate) fn get_compiled(&self, id: &str) -> Option<(&TdlSource, &Regex)> {
        self.by_id
            .get(&id.to_lowercase())
            .map(|entry| (&entry.0, &entry.1))
    }

    fn parse(json: &str, origin: &str) -> Result<Self> {
        let not_json =
            || Error::configuration(format!("TDL sources at '{origin}' is not valid JSON."));
        let value: Value = serde_json::from_str(json).map_err(|_| not_json())?;
        let entries = match value {
            Value::Array(entries) => entries,
            Value::Null => {
                return Err(Error::configuration(format!(
                    "TDL sources at '{origin}' must contain a JSON array."
                )))
            }
            _ => return Err(not_json()),
        };
        let mut by_id = HashMap::new();
        for (index, entry) in entries.iter().enumerate() {
            let members = match entry {
                Value::Object(members) => members,
                Value::Null => {
                    return Err(Error::configuration(format!(
                        "TDL source at index {index} in '{origin}' is null."
                    )))
                }
                _ => return Err(not_json()),
            };
            let source = TdlSource {
                id: member(members, "Id").ok_or_else(not_json)?,
                repository: member(members, "Repository").ok_or_else(not_json)?,
                directory: member(members, "Directory").ok_or_else(not_json)?,
                file_pattern: member(members, "FilePattern").ok_or_else(not_json)?,
                url_template: member(members, "UrlTemplate").ok_or_else(not_json)?,
            };
            let pattern = validate(&source, index, origin)?;
            let key = source.id.to_lowercase();
            if by_id.contains_key(&key) {
                return Err(Error::configuration(format!(
                    "Duplicate TDL source id '{}' in '{origin}'.",
                    source.id
                )));
            }
            by_id.insert(key, (source, pattern));
        }
        Ok(TdlSources { by_id })
    }
}

/// The text of the member named `name`, matched ignoring case, with the last
/// such member taken. Empty when there is none or it is null, and [`None`]
/// when it is neither text nor null, which is not a source.
fn member(members: &Map<String, Value>, name: &str) -> Option<String> {
    let mut found = String::new();
    for (key, value) in members {
        if !key.eq_ignore_ascii_case(name) {
            continue;
        }
        found = match value {
            Value::String(text) => text.clone(),
            Value::Null => String::new(),
            _ => return None,
        };
    }
    Some(found)
}

/// Check every part of `source`, giving its compiled file pattern.
fn validate(source: &TdlSource, index: usize, origin: &str) -> Result<Regex> {
    let parts = [
        (&source.id, "Id"),
        (&source.repository, "Repository"),
        (&source.directory, "Directory"),
        (&source.file_pattern, "FilePattern"),
        (&source.url_template, "UrlTemplate"),
    ];
    for (value, field) in parts {
        if value.trim().is_empty() {
            return Err(Error::configuration(format!(
                "TDL source at index {index} in '{origin}' is missing '{field}'."
            )));
        }
    }
    let id = &source.id;
    let pattern = Regex::new(&source.file_pattern).map_err(|error| {
        Error::configuration(format!(
            "TDL source '{id}' in '{origin}' has an invalid FilePattern. {error}"
        ))
    })?;
    if !pattern
        .capture_names()
        .flatten()
        .any(|name| name == VERSION_GROUP_NAME)
    {
        return Err(Error::configuration(format!(
            "TDL source '{id}' in '{origin}' FilePattern must declare a named group \
             '(?<{VERSION_GROUP_NAME}>...)'."
        )));
    }
    if !source.url_template.contains(NAME_PLACEHOLDER) {
        return Err(Error::configuration(format!(
            "TDL source '{id}' in '{origin}' UrlTemplate must include the \
             '{NAME_PLACEHOLDER}' placeholder."
        )));
    }
    Ok(pattern)
}

/// Turns a TDL source id a caller gives into the latest TDL URI for it.
///
/// The engine asks for any TDL entry that is not an absolute URI. One that
/// is not known, or that cannot be resolved, is left out of the text.
/// Implementations must be `Send + Sync`, because one engine serves many
/// requests at once.
pub trait TdlSourceResolver: Send + Sync {
    /// True if `id` names a source, compared ignoring case.
    fn is_known(&self, id: &str) -> bool;

    /// The latest URI for the source `id` names, or [`None`] when the id is
    /// not known, or the lookup failed and there is no earlier answer to
    /// give.
    fn resolve(&self, id: &str) -> Option<Url>;
}

/// The latest file's public URI among the GitHub folder entries `listing`
/// holds, read as GitHub's contents API gives them, an array of objects with
/// `name` and `type`. [`None`] when no file matches the pattern, and an error
/// when the listing is not such an array.
pub(crate) fn latest_url(
    source: &TdlSource,
    pattern: &Regex,
    listing: &[u8],
) -> std::result::Result<Option<Url>, String> {
    let value: Value = serde_json::from_slice(listing)
        .map_err(|error| format!("the folder listing is not JSON: {error}"))?;
    let entries = match value {
        Value::Array(entries) => entries,
        Value::Null => return Ok(None),
        _ => return Err("the folder listing is not a JSON array".to_owned()),
    };
    let mut winner: Option<(i64, String)> = None;
    for entry in &entries {
        let members = match entry {
            Value::Object(members) => members,
            Value::Null => continue,
            _ => return Err("an entry of the folder listing is not an object".to_owned()),
        };
        let kind = member(members, "type").ok_or("an entry's type is not text")?;
        if !kind.eq_ignore_ascii_case("file") {
            continue;
        }
        let name = member(members, "name").ok_or("an entry's name is not text")?;
        if name.is_empty() {
            continue;
        }
        let Some(captures) = pattern.captures(&name) else {
            continue;
        };
        let Some(version) = captures.name(VERSION_GROUP_NAME) else {
            continue;
        };
        let Ok(version) = version.as_str().trim().parse::<i64>() else {
            continue;
        };
        if winner.as_ref().is_none_or(|(best, _)| version > *best) {
            winner = Some((version, name));
        }
    }
    Ok(winner.and_then(|(_, name)| {
        Url::parse(&source.url_template.replace(NAME_PLACEHOLDER, &name)).ok()
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn message(result: Result<TdlSources>) -> String {
        result.expect_err("the sources are refused").to_string()
    }

    #[test]
    fn the_default_sources_name_the_standard_online_content_wording() {
        let sources = TdlSources::load_default().expect("the default sources load");
        assert_eq!(sources.len(), 1);
        let source = sources.get("mow-socw").expect("MOW-SOCW is known");
        assert_eq!(source.id, "MOW-SOCW");
        assert_eq!(
            source.repository,
            "movement-for-an-open-web/terms-documents"
        );
        assert_eq!(source.directory, "socw");
        assert_eq!(source.url_template, "https://m4ow.uk/socw/{name}");
        assert!(sources.contains("MOW-SOCW"));
        assert!(!sources.contains(""));
        assert!(!sources.contains("other"));
    }

    #[test]
    fn member_names_are_matched_ignoring_case() {
        let sources = TdlSources::load_from_json(
            r#"[{"id":"A","REPOSITORY":"o/r","directory":"d",
                 "filePattern":"^(?<n>\\d+)$","urltemplate":"https://x/{name}"}]"#,
        )
        .expect("the sources load");
        assert_eq!(sources.get("a").expect("A is known").repository, "o/r");
    }

    #[test]
    fn a_list_that_is_not_an_array_of_sources_is_refused() {
        assert!(message(TdlSources::load_from_json("{")).contains("is not valid JSON"));
        assert!(message(TdlSources::load_from_json("{}")).contains("is not valid JSON"));
        assert!(message(TdlSources::load_from_json("null")).contains("must contain a JSON array"));
        assert!(message(TdlSources::load_from_json("[null]"))
            .contains("TDL source at index 0 in '<inline>' is null."));
    }

    #[test]
    fn each_part_of_a_source_is_checked() {
        let source = |id: &str, pattern: &str, template: &str| {
            format!(
                r#"[{{"Id":"{id}","Repository":"o/r","Directory":"d",
                     "FilePattern":"{pattern}","UrlTemplate":"{template}"}}]"#
            )
        };
        assert!(message(TdlSources::load_from_json(&source(
            " ", "(?<n>x)", "{name}"
        )))
        .contains("is missing 'Id'"));
        assert!(
            message(TdlSources::load_from_json(&source("A", "(", "{name}")))
                .contains("has an invalid FilePattern")
        );
        assert!(message(TdlSources::load_from_json(&source(
            "A", "(?<m>x)", "{name}"
        )))
        .contains("must declare a named group '(?<n>...)'"));
        assert!(
            message(TdlSources::load_from_json(&source("A", "(?<n>x)", "x")))
                .contains("must include the '{name}' placeholder")
        );
        let twice = r#"[
            {"Id":"A","Repository":"o/r","Directory":"d","FilePattern":"(?<n>x)","UrlTemplate":"{name}"},
            {"Id":"a","Repository":"o/r","Directory":"d","FilePattern":"(?<n>x)","UrlTemplate":"{name}"}]"#;
        assert!(message(TdlSources::load_from_json(twice)).contains("Duplicate TDL source id 'a'"));
    }

    #[test]
    fn a_missing_file_is_refused() {
        assert!(message(TdlSources::load_from_file("")).contains("File path must be supplied."));
        assert!(
            message(TdlSources::load_from_file("no-such-tdl-sources.json"))
                .contains("TDL sources file not found")
        );
    }

    #[test]
    fn the_highest_numbered_file_is_the_latest() {
        let sources = TdlSources::load_default().unwrap();
        let (source, pattern) = sources.get_compiled("MOW-SOCW").unwrap();
        let listing = br#"[
            {"name":"1.txt","type":"file"},
            {"name":"10.txt","type":"file"},
            {"name":"9.txt","type":"file"},
            {"name":"11.txt","type":"dir"},
            {"name":"12.md","type":"file"},
            {"name":"README.md","type":"file"},
            null
        ]"#;
        let url = latest_url(source, pattern, listing).unwrap();
        assert_eq!(url.unwrap().as_str(), "https://m4ow.uk/socw/10.txt");
        assert_eq!(latest_url(source, pattern, b"[]").unwrap(), None);
        assert_eq!(latest_url(source, pattern, b"null").unwrap(), None);
        assert!(latest_url(source, pattern, b"{}").is_err());
    }
}
