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

//! The "Find out more" section that ends every example's output.
//!
//! Each example passes its own 51degrees.com links, tagged with that example's
//! file, and the repositories that matter to it.

use std::io::{self, Write};

/// The repositories an on-premise example lists: this SDK, the C engines it
/// builds on, and the free data files.
pub const ON_PREMISE_REPOSITORIES: &[&str] = &[
    "https://github.com/51Degrees/51degrees-rust",
    "https://github.com/51Degrees/51degrees-cxx",
    "https://github.com/51Degrees/data",
];

/// The repositories a cloud example lists.
pub const CLOUD_REPOSITORIES: &[&str] = &["https://github.com/51Degrees/51degrees-rust"];

/// The heading every example's section starts with.
pub const FIND_OUT_MORE_HEADING: &str = "Find out more";

/// The rule above and below the section, setting it apart from the example's
/// own output.
const RULE: &str = "----------------------------------------------------------------";

/// Write the section to `out`, being a rule, the heading, each of `links` as
/// its label and address, each of `repositories` as a bare address, and a
/// closing rule.
pub fn write_find_out_more(
    out: &mut dyn Write,
    links: &[(&str, &str)],
    repositories: &[&str],
) -> io::Result<()> {
    writeln!(out)?;
    writeln!(out, "{RULE}")?;
    writeln!(out, "{FIND_OUT_MORE_HEADING}")?;
    for (label, url) in links {
        writeln!(out, "\t{label}: {url}")?;
    }
    for url in repositories {
        writeln!(out, "\t{url}")?;
    }
    writeln!(out, "{RULE}")
}

/// Print the section to standard output. See [`write_find_out_more`].
pub fn print_find_out_more(links: &[(&str, &str)], repositories: &[&str]) {
    let _ = write_find_out_more(&mut io::stdout().lock(), links, repositories);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_section_lists_the_links_then_the_repositories() {
        let mut out = Vec::new();
        write_find_out_more(
            &mut out,
            &[("Pricing", "https://example.com/pricing")],
            ON_PREMISE_REPOSITORIES,
        )
        .unwrap();
        let text = String::from_utf8(out).unwrap();
        let heading = text.find(FIND_OUT_MORE_HEADING).unwrap();
        let link = text.find("\tPricing: https://example.com/pricing").unwrap();
        let repository = text.find("\thttps://github.com/51Degrees/data").unwrap();
        assert!(heading < link && link < repository);
        assert_eq!(text.matches(RULE).count(), 2);
    }
}
