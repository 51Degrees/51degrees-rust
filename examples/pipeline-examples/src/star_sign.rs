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

//! Shared star-sign building blocks for the pipeline examples.
//!
//! Every star-sign example needs the same three things: a place to store the
//! computed sign (the element data), the table that maps a month-and-day to a
//! sign, and a way to decide a sign from a date. They live here so the six bins
//! share one tested implementation. Each bin still defines its own
//! [`fiftyone_pipeline_core::FlowElement`] (the part the specification is
//! actually demonstrating), but they all populate this same
//! [`StarSignData`] and reuse [`star_sign_for`].

use std::any::Any;

use fiftyone_pipeline_core::{ElementData, MapElementData, NoValueError, PropertyValue};

/// The data key the star-sign element stores its data under, `"starsign"`.
pub const STAR_SIGN_DATA_KEY: &str = "starsign";

/// The property name holding the computed sign.
pub const STAR_SIGN_PROPERTY: &str = "starsign";

/// The property name holding the JavaScript that gathers the birth date on the
/// client, used only by the client-side example.
pub const DOB_JAVASCRIPT_PROPERTY: &str = "dobjavascript";

/// The marker returned when no sign could be determined, `"Unknown"`.
pub const UNKNOWN_STAR_SIGN: &str = "Unknown";

/// One row of the star-sign table.
///
/// A sign runs from `start` (a month and day, inclusive of the day after) up to
/// `end`. The comparison is on the month-and-day only, the year is irrelevant, so
/// a date is just reduced to its `(month, day)` before it is tested against the
/// boundaries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StarSignBoundary {
    /// The sign's name, for example `"Aries"`.
    pub name: &'static str,
    /// The first month-and-day of the sign, exclusive of the boundary itself
    /// (a strict `>` comparison).
    pub start: (u32, u32),
    /// The last month-and-day of the sign, exclusive of the boundary itself
    /// (a strict `<` comparison).
    pub end: (u32, u32),
}

/// The twelve star signs with their date boundaries.
///
/// The comparison is strict on both ends, which leaves the boundary days
/// themselves (and the year-wrapping Capricorn span) resolving to
/// [`UNKNOWN_STAR_SIGN`]. This is preserved deliberately rather than silently
/// "fixing" the astrology.
pub const STAR_SIGNS: [StarSignBoundary; 12] = [
    StarSignBoundary {
        name: "Aries",
        start: (3, 21),
        end: (4, 19),
    },
    StarSignBoundary {
        name: "Taurus",
        start: (4, 20),
        end: (5, 20),
    },
    StarSignBoundary {
        name: "Gemini",
        start: (5, 21),
        end: (6, 20),
    },
    StarSignBoundary {
        name: "Cancer",
        start: (6, 21),
        end: (7, 22),
    },
    StarSignBoundary {
        name: "Leo",
        start: (7, 23),
        end: (8, 22),
    },
    StarSignBoundary {
        name: "Virgo",
        start: (8, 23),
        end: (9, 22),
    },
    StarSignBoundary {
        name: "Libra",
        start: (9, 23),
        end: (10, 22),
    },
    StarSignBoundary {
        name: "Scorpio",
        start: (10, 23),
        end: (11, 21),
    },
    StarSignBoundary {
        name: "Sagittarius",
        start: (11, 22),
        end: (12, 21),
    },
    StarSignBoundary {
        name: "Capricorn",
        start: (12, 22),
        end: (1, 19),
    },
    StarSignBoundary {
        name: "Aquarius",
        start: (1, 20),
        end: (2, 18),
    },
    StarSignBoundary {
        name: "Pisces",
        start: (2, 19),
        end: (3, 20),
    },
];

/// Determine the star sign for a `(month, day)`, searching the supplied
/// boundaries.
///
/// This is the shared lookup the bins call once they have parsed a birth date,
/// whether the boundaries came from the hard-coded [`STAR_SIGNS`] table or were
/// read from an on-premise data file. It uses a strict month-and-day comparison
/// and returns `None` when no sign matches (the caller then stores
/// [`UNKNOWN_STAR_SIGN`]).
pub fn star_sign_for(
    boundaries: &[StarSignBoundary],
    month: u32,
    day: u32,
) -> Option<&'static str> {
    let value = (month, day);
    boundaries
        .iter()
        .find(|sign| value > sign.start && value < sign.end)
        .map(|sign| sign.name)
}

/// Parse a `dd/mm/yyyy` (or `dd/mm`) date string into a `(month, day)` pair.
///
/// The client-side example gathers the birth date as a slash-separated string
/// from a cookie, so this accepts that format. Only the month and day are
/// needed, but a trailing year is tolerated. Returns `None` if the string is not
/// two or three numeric, slash-separated fields.
pub fn parse_day_month(date: &str) -> Option<(u32, u32)> {
    let mut parts = date.split('/');
    let day: u32 = parts.next()?.trim().parse().ok()?;
    let month: u32 = parts.next()?.trim().parse().ok()?;
    if (1..=12).contains(&month) && (1..=31).contains(&day) {
        Some((month, day))
    } else {
        None
    }
}

/// The element data produced by every star-sign example.
///
/// It embeds a [`MapElementData`] so it gets the dynamic property bag (and the
/// case-insensitive `get`/`keys`) for free, exactly as the core's `DataBase`
/// pattern intends. The strongly-typed [`StarSignData::star_sign`] and
/// [`StarSignData::dob_javascript`] accessors read back the two properties the
/// examples populate.
#[derive(Debug, Clone, Default)]
pub struct StarSignData {
    inner: MapElementData,
}

impl StarSignData {
    /// Create an empty star-sign data.
    pub fn new() -> Self {
        StarSignData::default()
    }

    /// Set the computed star sign.
    pub fn set_star_sign(&mut self, sign: impl Into<String>) {
        self.inner.insert(STAR_SIGN_PROPERTY, sign.into());
    }

    /// Set the client-side JavaScript that gathers the birth date. Stored as a
    /// [`PropertyValue::JavaScript`] so the JavaScript builder element bundles it
    /// for execution on the client.
    pub fn set_dob_javascript(&mut self, javascript: impl Into<String>) {
        self.inner.insert(
            DOB_JAVASCRIPT_PROPERTY,
            PropertyValue::JavaScript(javascript.into()),
        );
    }

    /// The computed star sign, or `None` if it was never set.
    pub fn star_sign(&self) -> Option<&str> {
        self.inner
            .get_value(STAR_SIGN_PROPERTY)
            .and_then(PropertyValue::as_str)
    }

    /// The client-side birth-date JavaScript, or `None` if there is none.
    pub fn dob_javascript(&self) -> Option<&str> {
        self.inner
            .get_value(DOB_JAVASCRIPT_PROPERTY)
            .and_then(PropertyValue::as_str)
    }
}

impl ElementData for StarSignData {
    fn get(&self, name: &str) -> Result<PropertyValue, NoValueError> {
        self.inner.get(name)
    }

    fn keys(&self) -> Vec<String> {
        self.inner.keys()
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn looks_up_known_signs() {
        // 18 December is Sagittarius (start 22/11, end 21/12).
        assert_eq!(star_sign_for(&STAR_SIGNS, 12, 18), Some("Sagittarius"));
        // 15 July is Cancer.
        assert_eq!(star_sign_for(&STAR_SIGNS, 7, 15), Some("Cancer"));
    }

    #[test]
    fn unmatched_date_returns_none() {
        // The strict boundary comparison leaves the boundary day itself
        // unmatched.
        assert_eq!(star_sign_for(&STAR_SIGNS, 4, 19), None);
    }

    #[test]
    fn parses_day_month() {
        assert_eq!(parse_day_month("18/12/1992"), Some((12, 18)));
        assert_eq!(parse_day_month("15/7"), Some((7, 15)));
        assert_eq!(parse_day_month("not-a-date"), None);
        assert_eq!(parse_day_month("40/12"), None);
    }

    #[test]
    fn data_round_trips() {
        let mut data = StarSignData::new();
        data.set_star_sign("Sagittarius");
        assert_eq!(data.star_sign(), Some("Sagittarius"));
        assert_eq!(
            data.get(STAR_SIGN_PROPERTY).unwrap().as_str(),
            Some("Sagittarius")
        );
    }
}
