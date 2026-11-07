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

//! Finding the script files a path with a wildcard names.
//!
//! A `*` or `?` in the file name part of a path selects every file in the
//! folder whose name matches, read by the rules .NET's
//! `Directory.GetFiles(folder, pattern)` applies, so one configuration adds
//! the same scripts in both languages. Those are the DOS rules Windows has
//! always used:
//!
//! - `*` matches any run of characters, including none.
//! - `?` matches one character, or none where the name has a `.` or has
//!   ended, so `A?.yaml` matches `A.yaml` and `AB.yaml`.
//! - A `.` followed by `*` or `?` also matches the end of the name, so
//!   `HumanConfidence.*` matches a file called `HumanConfidence`.
//! - A pattern ending `*.` matches only names with no extension.
//! - `*.*` matches every name, with a `.` or without.
//!
//! Letter case is ignored on Windows and Apple systems and matched
//! everywhere else, as .NET does on each. Other characters match only
//! themselves.

use std::path::{Path, PathBuf};

/// Whether names are compared without regard to letter case, as .NET
/// compares them on this platform.
pub(crate) const IGNORE_CASE: bool = cfg!(any(windows, target_vendor = "apple"));

/// True where the path holds a wildcard, anywhere in it.
pub(crate) fn has_wildcard(path: &str) -> bool {
    path.contains(['*', '?'])
}

/// The folder to search and the pattern to match names with, from a path
/// holding a wildcard.
///
/// A path with no folder part searches the current folder. A path ending
/// in a separator has an empty pattern, which matches every name.
pub(crate) fn split(path: &Path) -> (PathBuf, String) {
    let text = path.as_os_str().to_string_lossy();
    let (folder, pattern) = if text.ends_with(std::path::is_separator) {
        (Some(path), String::new())
    } else {
        (
            path.parent(),
            path.file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default(),
        )
    };
    let folder = match folder {
        Some(folder) if !folder.to_string_lossy().trim().is_empty() => folder.to_path_buf(),
        _ => std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
    };
    (folder, pattern)
}

/// One part of a pattern, after the DOS rules have been applied.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Token {
    /// A character that matches itself.
    Char(char),
    /// Any run of characters, including none.
    Star,
    /// Any run of characters up to the last `.` of the name.
    DosStar,
    /// One character, or none at a `.` or the end of the name.
    DosQuestion,
    /// A `.`, or nothing at the end of the name.
    DosDot,
}

/// The pattern as .NET's `TranslateWin32Expression` rewrites it, after the
/// special cases `Directory.GetFiles` applies to the whole pattern.
pub(crate) fn translate(pattern: &str) -> Vec<Token> {
    if pattern.is_empty() || pattern == "." || pattern == "*" || pattern == "*.*" {
        return vec![Token::Star];
    }
    let chars: Vec<char> = pattern.chars().collect();
    let last = chars.len() - 1;
    let mut tokens: Vec<Token> = Vec::with_capacity(chars.len());
    for (index, &ch) in chars.iter().enumerate() {
        match ch {
            '.' if index >= 1 && index == last && chars[index - 1] == '*' => {
                // `*.` at the end becomes the one DOS star.
                if let Some(token) = tokens.last_mut() {
                    *token = Token::DosStar;
                }
            }
            '.' if index < last && matches!(chars[index + 1], '?' | '*') => {
                tokens.push(Token::DosDot);
            }
            '?' => tokens.push(Token::DosQuestion),
            '*' => tokens.push(Token::Star),
            other => tokens.push(Token::Char(other)),
        }
    }
    tokens
}

/// True where `name` matches `pattern`, as .NET's `MatchesWin32Expression`
/// decides.
///
/// This follows the state machine .NET runs, step for step, rather than a
/// shorter description of it, because the DOS rules have corners only the
/// state machine defines. Each state is twice the position in the pattern,
/// plus one where a star has finished.
pub(crate) fn matches(pattern: &[Token], name: &str, ignore_case: bool) -> bool {
    let name: Vec<char> = name.chars().collect();
    if pattern.is_empty() || name.is_empty() {
        return false;
    }
    if pattern[0] == Token::Star {
        if pattern.len() == 1 {
            return true;
        }
        // A star followed by plain characters is "ends with".
        let end = &pattern[1..];
        if end.iter().all(|token| matches!(token, Token::Char(_))) {
            if name.len() < end.len() {
                return false;
            }
            let tail = &name[name.len() - end.len()..];
            return end.iter().zip(tail).all(|(token, &ch)| match token {
                Token::Char(expected) => same(*expected, ch, ignore_case),
                _ => false,
            });
        }
    }

    let max_state = pattern.len() * 2;
    let mut prior: Vec<usize> = vec![0];
    let mut name_offset = 0;
    let mut name_char = '\0';
    let mut name_finished = false;
    while !name_finished {
        if name_offset < name.len() {
            name_char = name[name_offset];
            name_offset += 1;
        } else {
            if prior.last() == Some(&max_state) {
                break;
            }
            name_finished = true;
        }

        let mut current: Vec<usize> = Vec::new();
        let mut prior_index = 0;
        let mut checked = 0;
        while prior_index < prior.len() {
            let mut offset = prior[prior_index].div_ceil(2);
            prior_index += 1;
            while offset < pattern.len() {
                let state = offset * 2;
                // Whether the pattern moves on to its next part without
                // using up this character of the name.
                let next = match pattern[offset] {
                    Token::Star => {
                        current.push(state);
                        current.push(state + 1);
                        true
                    }
                    Token::DosStar => {
                        let not_last_period = !name_finished
                            && name_char == '.'
                            && name[name_offset..].contains(&'.');
                        if name_finished || name_char != '.' || not_last_period {
                            current.push(state);
                        }
                        current.push(state + 1);
                        true
                    }
                    Token::DosQuestion => {
                        if name_finished || name_char == '.' {
                            true
                        } else {
                            current.push(state + 2);
                            false
                        }
                    }
                    Token::DosDot => {
                        if name_finished {
                            true
                        } else {
                            if name_char == '.' {
                                current.push(state + 2);
                            }
                            false
                        }
                    }
                    Token::Char(expected) => {
                        if !name_finished && same(expected, name_char, ignore_case) {
                            current.push(state + 2);
                        }
                        false
                    }
                };
                if !next {
                    break;
                }
                offset += 1;
                if offset == pattern.len() {
                    current.push(max_state);
                }
            }

            // Skip the earlier states already passed, so the list stays in
            // order with no state twice, as .NET's does.
            if prior_index < prior.len() && checked < current.len() {
                while checked < current.len() {
                    while prior_index < prior.len() && prior[prior_index] < current[checked] {
                        prior_index += 1;
                    }
                    checked += 1;
                }
            }
        }

        if current.is_empty() {
            return false;
        }
        prior = current;
    }
    prior.last() == Some(&max_state)
}

/// Two characters compared as .NET compares them, ignoring case through each
/// one's upper case form where that is a single character.
fn same(a: char, b: char, ignore_case: bool) -> bool {
    a == b || (ignore_case && upper(a) == upper(b))
}

fn upper(ch: char) -> char {
    let mut upper = ch.to_uppercase();
    match (upper.next(), upper.next()) {
        (Some(single), None) => single,
        _ => ch,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A pattern in .NET's own notation, where `<`, `>` and `"` are the DOS
    /// star, question mark and dot.
    fn dos(pattern: &str) -> Vec<Token> {
        pattern
            .chars()
            .map(|ch| match ch {
                '*' => Token::Star,
                '<' => Token::DosStar,
                '>' => Token::DosQuestion,
                '"' => Token::DosDot,
                other => Token::Char(other),
            })
            .collect()
    }

    /// Every case of `SimpleMatchData` and `Win32MatchData` in .NET's
    /// `FileSystemNameTests.cs` at dotnet/runtime 5c99482, with the answer
    /// .NET's test expects, apart from the null pattern Rust cannot pass.
    #[test]
    fn every_case_dotnet_tests_its_matcher_with() {
        let cases: &[(&str, &str, bool, bool)] = &[
            ("*", "", false, false),
            ("*", "", true, false),
            ("*", "ab", false, true),
            ("*", "AB", true, true),
            ("*foo", "foo", false, true),
            ("*foo", "foo", true, true),
            ("*foo", "FOO", false, false),
            ("*foo", "FOO", true, true),
            ("*foo", "nofoo", true, true),
            ("*foo", "NoFOO", true, true),
            ("*foo", "noFOO", false, false),
            ("*", "foo.txt", true, true),
            (".", "foo.txt", true, false),
            (".", "footxt", true, false),
            ("*.*", "foo.txt", true, true),
            ("*.*", "foo.", true, true),
            ("*.*", ".foo", true, true),
            ("*.*", "footxt", true, false),
            ("<\"*", "footxt", true, true),
            ("<\"*", "foo.txt", true, true),
            ("<\"*", ".foo", true, true),
            ("<\"*", "foo.", true, true),
            (">\">", "a.b", true, true),
            (">\">", "a.", true, true),
            (">\">", "a", true, true),
            (">\">", "ab", true, false),
            (">\">", "a.bc", true, false),
            (">\">", "ab.c", true, false),
            (">>\">>", "a.b", true, true),
            (">>\"\">>", "a.b", true, false),
            (">>\">>", "a.bc", true, true),
            (">>\">>", "ab.ba", true, true),
            (">>\">>", "ab.", true, true),
            (">>\"\"\">>", "ab.", true, true),
            (">>b\">>", "ab.ba", true, false),
            ("a>>\">>", "ab.ba", true, true),
            (">>\">>a", "ab.ba", true, false),
            (">>\"b>>", "ab.ba", true, true),
            (">>\"b>>", "ab.b", true, true),
            (">>b.>>", "ab.ba", true, false),
            ("a>>.>>", "ab.ba", true, true),
            (">>.>>a", "ab.ba", true, false),
            (">>.b>>", "ab.ba", true, true),
            (">>.b>>", "ab.b", true, true),
            (">>\">>\">>", "ab.ba", true, true),
            (">>\">>\">>", "abba", true, false),
            (">>\"ab\"ba", "ab.ba", true, false),
            ("ab\"ba\">>", "ab.ba", true, true),
            ("ab\">>\"ba", "ab.ba", true, false),
            (">>\">>\">>>", "ab.ba.cab", true, true),
            ("a>>\"b>>\"c>>>", "ab.ba.cab", true, true),
            ("<", "a", true, true),
            ("<", "a.", true, true),
            ("<", "a. ", true, false),
            ("<", "a.b", true, false),
            ("foo<", "foo.", true, true),
            ("foo<", "foo. ", true, false),
            ("<<", "a.b", true, true),
            ("<<", "a.b.c", true, true),
            ("<\"", "a.b.c", true, false),
            ("<.", "a", true, false),
            ("<.", "a.", true, true),
            ("<.", "a.b", true, false),
        ];
        for &(pattern, name, ignore_case, expected) in cases {
            assert_eq!(
                matches(&dos(pattern), name, ignore_case),
                expected,
                "'{pattern}' against '{name}', ignoring case {ignore_case}"
            );
        }
    }

    /// The `TranslateExpression` cases of the same .NET test file, and some
    /// more. `.` becomes `*`, as `Directory.GetFiles` makes it before
    /// rewriting.
    #[test]
    fn a_pattern_is_rewritten_as_dotnet_rewrites_it() {
        for (pattern, expected) in [
            ("", "*"),
            ("*.*", "*"),
            ("*", "*"),
            (".", "*"),
            ("?", ">"),
            ("*.", "<"),
            ("?.?", ">\">"),
            ("foo*.", "foo<"),
            ("*.yaml", "*.yaml"),
            ("Human*.y?ml", "Human*.y>ml"),
            ("x.*", "x\"*"),
        ] {
            assert_eq!(translate(pattern), dos(expected), "'{pattern}'");
        }
    }

    /// What the rules mean for script files.
    #[test]
    fn script_file_patterns_match_as_dotnet_matches_them() {
        for (pattern, name, expected) in [
            ("*.yaml", "HumanConfidence.yaml", true),
            ("*.yaml", "HumanConfidence.yml", false),
            ("*.yaml", "HumanConfidence.yaml.bak", false),
            // `?` matches no character only at a `.` or the end of the name.
            ("*.y?ml", "HumanConfidence.yml", false),
            ("*.y?ml", "HumanConfidence.yaml", true),
            ("A?.yaml", "A.yaml", true),
            ("A?.yaml", "AB.yaml", true),
            ("A?.yaml", "ABC.yaml", false),
            ("?.yaml", "A.yaml", true),
            ("HumanConfidence.*", "HumanConfidence", true),
            ("HumanConfidence.*", "HumanConfidence.json", true),
            ("*.*", "README", true),
            ("*.", "README", true),
            ("*.", "README.md", false),
            ("Human*", "HumanConfidence.yaml", true),
            ("*Confidence*", "HumanConfidence.yaml", true),
        ] {
            assert_eq!(
                matches(&translate(pattern), name, false),
                expected,
                "'{pattern}' against '{name}'"
            );
        }
    }

    #[test]
    fn letter_case_is_ignored_only_where_asked() {
        let pattern = translate("human*.YAML");
        assert!(matches(&pattern, "HumanConfidence.yaml", true));
        assert!(!matches(&pattern, "HumanConfidence.yaml", false));
        // ß has no single upper case character, so it matches only itself.
        assert!(!matches(&translate("STRASSE*"), "straße.yaml", true));
        assert!(matches(&translate("é*"), "É.yaml", true));
    }

    // A test on WebAssembly may have no current folder.
    #[cfg(not(target_family = "wasm"))]
    #[test]
    fn the_folder_and_pattern_come_from_the_path() {
        let current = std::env::current_dir().unwrap();
        assert_eq!(
            split(Path::new("*.yaml")),
            (current.clone(), "*.yaml".to_owned())
        );
        assert_eq!(
            split(Path::new("derived/*.yaml")),
            (PathBuf::from("derived"), "*.yaml".to_owned())
        );
        assert_eq!(
            split(Path::new("derived/")),
            (PathBuf::from("derived/"), String::new())
        );
        assert_eq!(split(Path::new(" /x*.yaml")).0, current);
        assert!(has_wildcard("derived/*.yaml"));
        assert!(has_wildcard("derived/A?.yaml"));
        assert!(has_wildcard("der*ived/A.yaml"));
        assert!(!has_wildcard("derived/A.yaml"));
    }
}
