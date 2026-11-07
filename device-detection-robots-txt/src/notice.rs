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

//! The fixed lines that name the robots.txt licence and explain the
//! annotations. The two addresses are written exactly as the other 51Degrees
//! packages write them, because the text is compared with theirs byte for
//! byte.

/// The address of the licence the robots.txt text is given under, which the
/// copyright line of the plain text names.
pub const LICENSE_URL: &str = "https://51degrees.com/terms/robots-txt";

/// The legend the text with annotations gives under the licence, one line
/// each.
pub(crate) const LEGEND: [&str; 6] = [
    "# Legend",
    "# N: Name of the crawler",
    "# U: Usages that the crawler makes of obtained content",
    "# A: Address of any reference URLs available for more information",
    "# TDL: Terms Document Locator (immutable terms URL applied to the Allow block)",
    "# See https://51degrees.com/robots-txt for further details",
];
