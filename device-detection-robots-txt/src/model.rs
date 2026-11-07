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

//! The crawlers and usages a data file holds, and the robots.txt text written
//! from them.

use std::collections::HashSet;

use icu_collator::options::CollatorOptions;
use icu_collator::{Collator, CollatorPreferences};
use url::Url;

use crate::constants::{LICENSE, NOT_APPLICABLE_USAGE, NO_VALUE};
use crate::notice::{LEGEND, LICENSE_URL};

/// One use a crawler makes of the pages it fetches, such as search, as the
/// data file lists it.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct UsageModel {
    /// The usage's name, as the data file gives it.
    pub name: String,
    /// What the usage means, for a person choosing which usages to allow.
    pub description: String,
}

impl UsageModel {
    /// A usage with its name and description.
    pub fn new(name: impl Into<String>, description: impl Into<String>) -> Self {
        UsageModel {
            name: name.into(),
            description: description.into(),
        }
    }
}

/// One crawler, as the data file describes it.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct CrawlerModel {
    /// The crawler's name.
    pub name: String,
    /// The names of the usages the crawler makes of the pages it fetches.
    pub usages: Vec<String>,
    /// The product tokens the crawler answers to in a robots.txt file.
    pub product_tokens: Vec<String>,
    /// An address with more about the crawler, when the data gives one that
    /// is an absolute URI.
    pub reference_url: Option<Url>,
}

impl CrawlerModel {
    /// A crawler with its name, usages, product tokens and reference address.
    pub fn new(
        name: impl Into<String>,
        usages: Vec<String>,
        product_tokens: Vec<String>,
        reference_url: Option<Url>,
    ) -> Self {
        CrawlerModel {
            name: name.into(),
            usages,
            product_tokens,
            reference_url,
        }
    }
}

/// Every crawler and usage the robots.txt text is written from.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RobotsTxtModel {
    crawlers: Vec<CrawlerModel>,
    usages: Vec<UsageModel>,
}

impl RobotsTxtModel {
    /// A model of the crawlers and usages given, each sorted by name in the
    /// order of the Unicode root collation, which is the order .NET's
    /// invariant culture gives. The sort is stable, so names that compare
    /// equal keep the order they were given in.
    pub fn new(crawlers: Vec<CrawlerModel>, usages: Vec<UsageModel>) -> Self {
        let mut crawlers = crawlers;
        let mut usages = usages;
        sort_by_name(&mut crawlers, crawler_name);
        sort_by_name(&mut usages, usage_name);
        RobotsTxtModel { crawlers, usages }
    }

    /// The crawlers, sorted by name.
    pub fn crawlers(&self) -> &[CrawlerModel] {
        &self.crawlers
    }

    /// The usages, sorted by name, for a person choosing which to allow.
    pub fn usages(&self) -> &[UsageModel] {
        &self.usages
    }

    /// The robots.txt text that refuses every crawler none of whose usages
    /// is in `allowed`, and allows every other crawler through the closing
    /// `User-Agent: *` group, which carries a TDL line for each of `tdls`.
    ///
    /// `allowed` holds usage names as the model gives them. A crawler the
    /// data records no usage for is allowed when `allowed` holds the usage
    /// `N/A`, compared ignoring case. A crawler with no product token other
    /// than a blank one or `N/A` cannot be named in a robots.txt file, so the
    /// text leaves it out, comments included.
    ///
    /// With `annotations` the text starts with the licence and a legend, and
    /// each crawler has comments giving its name, usages and address. Without,
    /// it starts with one copyright line. Each line ends with `\n`.
    pub fn write(&self, allowed: &HashSet<String>, tdls: &[Url], annotations: bool) -> String {
        let not_applicable_allowed = allowed
            .iter()
            .any(|usage| usage.eq_ignore_ascii_case(NOT_APPLICABLE_USAGE));
        let mut refused = String::new();
        let mut allowed_crawlers = Vec::new();
        for crawler in &self.crawlers {
            if !crawler.product_tokens.iter().any(|t| is_usable_token(t)) {
                continue;
            }
            if is_allowed(crawler, allowed, not_applicable_allowed) {
                allowed_crawlers.push(crawler);
                continue;
            }
            if annotations {
                add_annotations(crawler, &mut refused);
            }
            for token in crawler.product_tokens.iter().filter(|t| is_usable_token(t)) {
                refused.push_str("User-Agent: ");
                refused.push_str(token);
                refused.push('\n');
                refused.push_str("Disallow: /\n");
            }
            refused.push('\n');
        }

        let mut text = String::new();
        if annotations {
            add_header(&mut text);
        } else {
            add_copyright(&mut text);
        }
        text.push_str(&refused);

        text.push_str("User-Agent: *\n");
        if annotations {
            for crawler in allowed_crawlers {
                add_annotations(crawler, &mut text);
            }
        }
        for tdl in tdls {
            text.push_str("# Terms ");
            text.push_str(tdl.as_str());
            text.push('\n');
            text.push_str("TDL: ");
            text.push_str(tdl.as_str());
            text.push('\n');
        }
        text.push_str("Allow: /\n");
        text
    }
}

fn crawler_name(crawler: &CrawlerModel) -> &str {
    &crawler.name
}

fn usage_name(usage: &UsageModel) -> &str {
    &usage.name
}

/// Sort `items` stably by name with the root collation, or in the order of
/// their code points if no collator can be made, which the compiled in data
/// rules out.
fn sort_by_name<T>(items: &mut [T], name: fn(&T) -> &str) {
    match Collator::try_new(CollatorPreferences::default(), CollatorOptions::default()) {
        Ok(collator) => items.sort_by(|a, b| collator.compare(name(a), name(b))),
        Err(error) => {
            log::warn!(
                "No collator could be made for the root locale, so names are \
                 sorted by code point: {error}"
            );
            items.sort_by(|a, b| name(a).cmp(name(b)));
        }
    }
}

/// True if the token can head a group. A blank token cannot, because a
/// reader matches the token against the crawler's name, so a blank
/// `User-Agent:` line would refuse the whole site to every crawler. Nor can
/// the data's no value marker, which names no crawler.
fn is_usable_token(token: &str) -> bool {
    let token = token.trim();
    !token.is_empty() && !token.eq_ignore_ascii_case(NO_VALUE)
}

/// True if the crawler makes at least one of the allowed usages or, where
/// the data records no usage for it, if the `N/A` usage is allowed.
fn is_allowed(crawler: &CrawlerModel, allowed: &HashSet<String>, not_applicable: bool) -> bool {
    if crawler.usages.iter().all(|usage| usage.trim().is_empty()) {
        return not_applicable;
    }
    crawler.usages.iter().any(|usage| allowed.contains(usage))
}

fn add_annotations(crawler: &CrawlerModel, text: &mut String) {
    text.push_str("# N: ");
    text.push_str(&crawler.name);
    text.push('\n');
    text.push_str("# U: ");
    text.push_str(&crawler.usages.join(", "));
    text.push('\n');
    if let Some(url) = &crawler.reference_url {
        text.push_str("# A: ");
        text.push_str(url.as_str());
        text.push('\n');
    }
}

/// The licence, each line as a comment, then the legend.
fn add_header(text: &mut String) {
    for line in LICENSE.lines() {
        text.push_str("# ");
        text.push_str(line.trim());
        text.push('\n');
    }
    text.push('\n');
    for line in LEGEND {
        text.push_str(line);
        text.push('\n');
    }
    text.push('\n');
}

fn add_copyright(text: &mut String) {
    text.push_str("# robots.txt copyright 51Degrees. See ");
    text.push_str(LICENSE_URL);
    text.push_str(" for details.\n");
}

#[cfg(test)]
mod tests {
    use robotstxt::DefaultMatcher;

    use super::*;

    const SITE: &str = "https://example.com/";

    fn strings(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_owned()).collect()
    }

    fn crawler(name: &str, usages: &[&str], tokens: &[&str], url: Option<&str>) -> CrawlerModel {
        CrawlerModel::new(
            name,
            strings(usages),
            strings(tokens),
            url.map(|url| Url::parse(url).expect("a test address")),
        )
    }

    /// A model with a crawler for each case the text handles, given out of
    /// name order.
    fn model() -> RobotsTxtModel {
        RobotsTxtModel::new(
            vec![
                crawler("Search Bot", &["Search"], &["SearchBot"], None),
                crawler(
                    "ai trainer",
                    &["Training", "Search"],
                    &["AiTrainer", "AiTrainer-Extended"],
                    Some("https://Example.com"),
                ),
                crawler("Monitor", &["Monitoring"], &["MonitorBot"], None),
                crawler("No Usage", &[], &["NoUsageBot"], None),
                crawler("Blank Usage", &[" "], &["BlankUsageBot"], None),
                crawler("NotCrawler", &["N/A"], &["N/A"], None),
                crawler("Blank Token", &["Training"], &[" ", ""], None),
                crawler(
                    "Some Blank",
                    &["Training"],
                    &["", "SomeBlankBot", " n/a "],
                    None,
                ),
            ],
            vec![
                UsageModel::new("Training", "Training a model"),
                UsageModel::new("Search", "Search"),
                UsageModel::new("N/A", "Not applicable"),
                UsageModel::new("Monitoring", "Monitoring"),
            ],
        )
    }

    /// The first line of the plain text. The address is the one in
    /// `notice.rs`, the one file the link check lets name it untagged.
    fn copyright() -> String {
        format!("# robots.txt copyright 51Degrees. See {LICENSE_URL} for details.")
    }

    fn allowed(usages: &[&str]) -> HashSet<String> {
        usages.iter().map(|usage| (*usage).to_owned()).collect()
    }

    #[test]
    fn crawlers_and_usages_are_sorted_by_name_in_the_root_collation() {
        let model = model();
        let names: Vec<&str> = model.crawlers().iter().map(|c| c.name.as_str()).collect();
        assert_eq!(
            names,
            [
                "ai trainer",
                "Blank Token",
                "Blank Usage",
                "Monitor",
                "No Usage",
                "NotCrawler",
                "Search Bot",
                "Some Blank",
            ]
        );
        let usages: Vec<&str> = model.usages().iter().map(|u| u.name.as_str()).collect();
        assert_eq!(usages, ["Monitoring", "N/A", "Search", "Training"]);
    }

    #[test]
    fn the_plain_text_refuses_each_crawler_with_no_allowed_usage() {
        let text = model().write(&allowed(&["Search"]), &[], false);
        let copyright = copyright();
        let expected = [
            copyright.as_str(),
            "User-Agent: BlankUsageBot",
            "Disallow: /",
            "",
            "User-Agent: MonitorBot",
            "Disallow: /",
            "",
            "User-Agent: NoUsageBot",
            "Disallow: /",
            "",
            "User-Agent: SomeBlankBot",
            "Disallow: /",
            "",
            "User-Agent: *",
            "Allow: /",
            "",
        ];
        assert_eq!(text, expected.join("\n"));
    }

    #[test]
    fn the_annotated_text_gives_the_licence_legend_and_each_crawler() {
        let tdl = Url::parse("https://m4ow.uk/socw/1.txt").unwrap();
        let text = model().write(&allowed(&["Search", "N/A"]), &[tdl], true);
        let mut expected: Vec<String> = LICENSE
            .lines()
            .map(|line| format!("# {}", line.trim()))
            .collect();
        expected.extend(
            [
                "",
                "# Legend",
                "# N: Name of the crawler",
                "# U: Usages that the crawler makes of obtained content",
                "# A: Address of any reference URLs available for more information",
                "# TDL: Terms Document Locator (immutable terms URL applied to the Allow block)",
                LEGEND[5],
                "",
                "# N: Monitor",
                "# U: Monitoring",
                "User-Agent: MonitorBot",
                "Disallow: /",
                "",
                "# N: Some Blank",
                "# U: Training",
                "User-Agent: SomeBlankBot",
                "Disallow: /",
                "",
                "User-Agent: *",
                "# N: ai trainer",
                "# U: Training, Search",
                "# A: https://example.com/",
                "# N: Blank Usage",
                "# U:  ",
                "# N: No Usage",
                "# U: ",
                "# N: Search Bot",
                "# U: Search",
                "# Terms https://m4ow.uk/socw/1.txt",
                "TDL: https://m4ow.uk/socw/1.txt",
                "Allow: /",
                "",
            ]
            .map(str::to_owned),
        );
        assert_eq!(text, expected.join("\n"));
    }

    #[test]
    fn the_licence_is_written_one_comment_a_line() {
        let text = model().write(&allowed(&[]), &[], true);
        let start = [
            "# This Original Work is copyright of 51 Degrees Mobile Experts Limited.",
            "# Copyright 2026 51 Degrees Mobile Experts Limited, Davidson House,",
            "# Forbury Square, Reading, Berkshire, United Kingdom RG1 3EU.",
            "# ",
            "# This Original Work is licensed under the European Union Public Licence",
        ]
        .join("\n");
        assert!(text.starts_with(&start), "{text}");
        let end = [
            "# such notice(s) shall fulfill the requirements of that article.",
            "",
            "# Legend",
        ]
        .join("\n");
        assert!(text.contains(&end), "{text}");
    }

    #[test]
    fn the_not_applicable_usage_is_matched_ignoring_case() {
        let model = model();
        let lower = model.write(&allowed(&["n/a"]), &[], false);
        let upper = model.write(&allowed(&["N/A"]), &[], false);
        assert_eq!(lower, upper);
        assert!(!lower.contains("NoUsageBot"));
        assert!(!lower.contains("BlankUsageBot"));
    }

    #[test]
    fn a_crawler_with_no_usable_token_is_left_out_entirely() {
        let model = model();
        for annotations in [false, true] {
            let text = model.write(&allowed(&[]), &[], annotations);
            assert!(!text.contains("NotCrawler"));
            assert!(!text.contains("Blank Token"));
            assert!(!text.contains("User-Agent: N/A"));
            assert!(!text.contains("User-Agent: \n"));
            assert!(!text.contains("User-Agent:  \n"));
            assert!(!text.contains(" n/a "));
        }
    }

    #[test]
    fn an_empty_usage_is_written_in_its_place() {
        let model = RobotsTxtModel::new(
            vec![crawler("Image Bot", &["", "Search"], &["ImageBot"], None)],
            vec![UsageModel::new("Search", "Search")],
        );
        for usages in [&["Search"][..], &[]] {
            let text = model.write(&allowed(usages), &[], true);
            let lines = "# N: Image Bot\n# U: , Search\n";
            assert!(text.contains(lines), "{text}");
        }
    }

    #[test]
    fn every_tdl_is_written_in_the_closing_group_in_order() {
        let tdls = [
            Url::parse("https://m4ow.uk/socw/2.txt").unwrap(),
            Url::parse("HTTPS://Example.COM:443").unwrap(),
        ];
        let text = RobotsTxtModel::default().write(&allowed(&[]), &tdls, false);
        let copyright = copyright();
        let expected = [
            copyright.as_str(),
            "User-Agent: *",
            "# Terms https://m4ow.uk/socw/2.txt",
            "TDL: https://m4ow.uk/socw/2.txt",
            "# Terms https://example.com/",
            "TDL: https://example.com/",
            "Allow: /",
            "",
        ];
        assert_eq!(text, expected.join("\n"));
    }

    /// The usages a case allows, and each agent with whether it may fetch
    /// the site.
    type Case = (&'static [&'static str], &'static [(&'static str, bool)]);

    /// What a crawler reads from each text, through a port of Google's
    /// robots.txt parser, which is the reference reader for RFC 9309.
    #[test]
    fn a_crawler_reads_the_answer_the_allowed_usages_give() {
        let model = model();
        let cases: [Case; 4] = [
            (
                &[],
                &[
                    ("SearchBot", false),
                    ("AiTrainer", false),
                    ("AiTrainer-Extended", false),
                    ("MonitorBot", false),
                    ("NoUsageBot", false),
                    ("SomeBlankBot", false),
                    ("UnknownBot", true),
                ],
            ),
            (
                &["Search"],
                &[
                    ("SearchBot", true),
                    ("AiTrainer", true),
                    ("AiTrainer-Extended", true),
                    ("MonitorBot", false),
                    ("NoUsageBot", false),
                    ("UnknownBot", true),
                ],
            ),
            (
                &["Training", "N/A"],
                &[
                    ("SearchBot", false),
                    ("AiTrainer", true),
                    ("MonitorBot", false),
                    ("NoUsageBot", true),
                    ("BlankUsageBot", true),
                    ("SomeBlankBot", true),
                ],
            ),
            (
                &["Monitoring", "Search", "Training", "N/A"],
                &[
                    ("SearchBot", true),
                    ("AiTrainer", true),
                    ("MonitorBot", true),
                    ("NoUsageBot", true),
                    ("SomeBlankBot", true),
                    ("UnknownBot", true),
                ],
            ),
        ];
        for (usages, answers) in cases {
            for annotations in [false, true] {
                let text = model.write(&allowed(usages), &[], annotations);
                for (agent, expected) in answers {
                    let mut matcher = DefaultMatcher::default();
                    assert_eq!(
                        matcher.one_agent_allowed_by_robots(&text, agent, SITE),
                        *expected,
                        "{agent} with {usages:?}, annotations {annotations}"
                    );
                }
            }
        }
    }
}
