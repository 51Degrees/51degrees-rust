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

//! The names, evidence keys and fixed text the engine uses.

/// The element data key the engine writes under.
pub const ELEMENT_DATA_KEY: &str = "robotstxt";

/// The property holding the robots.txt with a comment for each crawler, for
/// a person to read rather than for production.
pub const PROPERTY_ANNOTATED_TEXT: &str = "AnnotatedText";

/// The property holding the robots.txt with only the copyright comment,
/// ready to serve.
pub const PROPERTY_PLAIN_TEXT: &str = "PlainText";

/// The category both properties are in.
pub const PROPERTY_CATEGORY: &str = "Bots";

/// The start of the evidence key for each usage. The usage's name in lower
/// case follows, as in `query.robotstxt.search`.
pub const USAGE_EVIDENCE_KEY_PREFIX: &str = "query.robotstxt.";

/// The evidence key for one or more Terms Document Locator URIs, or TDL
/// source ids, separated by `,` or `|`.
pub const TDL_EVIDENCE_KEY: &str = "query.robotstxt.tdl";

/// The evidence values that allow a usage, compared ignoring case.
pub const ALLOW_VALUES: [&str; 5] = ["allow", "true", "yes", "enabled", "on"];

/// The evidence values that refuse a usage, compared ignoring case. Refusing
/// a usage wins over allowing it.
pub const DISALLOW_VALUES: [&str; 5] = ["disallow", "false", "no", "disabled", "off"];

/// The usage a crawler is governed by when the data records no usage for
/// it. Matched ignoring case, because the evidence key carries only its
/// lower case form.
pub const NOT_APPLICABLE_USAGE: &str = "N/A";

/// The value the data gives a crawler property that has none, such as the
/// product tokens of the profile every browser resolves to.
pub(crate) const NO_VALUE: &str = "N/A";

/// The data file property listing what a crawler uses pages for.
pub(crate) const CRAWLER_USAGE: &str = "CrawlerUsage";

/// The data file property holding a crawler's name.
pub(crate) const CRAWLER_NAME: &str = "CrawlerName";

/// The data file property listing the product tokens a crawler answers to
/// in a robots.txt file.
pub(crate) const CRAWLER_PRODUCT_TOKENS: &str = "CrawlerProductTokens";

/// The data file property holding an address with more about a crawler.
pub(crate) const CRAWLER_URL: &str = "CrawlerUrl";

/// The licence the robots.txt text is given under, written at the top of the
/// text with annotations.
pub const LICENSE: &str = include_str!("license-robots.txt");
