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

//! A period as .NET's `TimeSpan` holds it, read from text as .NET's
//! `TimeSpan.TryParse` reads it and written as its `ToString` writes it.

use std::fmt;
use std::time::Duration;

use super::text::{ConfigType, ParseFailure};

/// The ticks of 100 nanoseconds in one second.
const TICKS_PER_SECOND: i64 = 10_000_000;

/// The nanoseconds in one tick.
const NANOSECONDS_PER_TICK: u128 = 100;

/// The digits of a fraction of a second a tick holds.
const FRACTION_DIGITS: u32 = 7;

/// The largest number of days, hours, minutes and seconds .NET reads.
const MAX_DAYS: u32 = 10_675_199;
const MAX_HOURS: u32 = 23;
const MAX_MINUTES: u32 = 59;
const MAX_SECONDS: u32 = 59;

/// The largest fraction of a second, in ticks.
const MAX_FRACTION: u32 = 9_999_999;

/// .NET refuses a number from 2^28 up.
const NUMBER_LIMIT: u32 = 1 << 28;

/// .NET reads at most five numbers and six separators.
const MAX_NUMBERS: usize = 5;

/// The milliseconds .NET's `TimeSpan` can hold.
const MAX_MILLISECONDS: i64 = i64::MAX / 10_000;

/// A period of whole ticks of 100 nanoseconds, which may be negative, as
/// .NET's `TimeSpan` holds one.
///
/// A builder setting that takes a .NET `TimeSpan` reads its value as this,
/// so a configuration gives a period as it would for .NET, such as
/// `00:05:00` for five minutes, `1.00:00:00` or `1` for a day, and
/// `00:00:00.350` for 350 milliseconds. Its text form is the one .NET
/// writes, `[-][d.]hh:mm:ss[.fffffff]`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct TimeSpan {
    ticks: i64,
}

impl TimeSpan {
    /// The period of `ticks` ticks of 100 nanoseconds.
    pub const fn from_ticks(ticks: i64) -> Self {
        TimeSpan { ticks }
    }

    /// The ticks of 100 nanoseconds in the period.
    pub const fn ticks(self) -> i64 {
        self.ticks
    }

    /// The period `duration` gives, to the tick below, or `None` when it is
    /// too long for a .NET `TimeSpan`.
    pub fn from_duration(duration: Duration) -> Option<Self> {
        i64::try_from(duration.as_nanos() / NANOSECONDS_PER_TICK)
            .ok()
            .map(TimeSpan::from_ticks)
    }

    /// The period as a [`Duration`], or `None` when it is negative.
    pub fn to_duration(self) -> Option<Duration> {
        let ticks = u64::try_from(self.ticks).ok()?;
        let seconds = ticks / TICKS_PER_SECOND as u64;
        let nanoseconds = (ticks % TICKS_PER_SECOND as u64) * NANOSECONDS_PER_TICK as u64;
        Some(Duration::new(seconds, nanoseconds as u32))
    }
}

/// .NET's constant format, `[-][d.]hh:mm:ss[.fffffff]`, which `ToString`
/// gives.
impl fmt::Display for TimeSpan {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.ticks < 0 {
            f.write_str("-")?;
        }
        let ticks = self.ticks.unsigned_abs();
        let fraction = ticks % TICKS_PER_SECOND as u64;
        let seconds = ticks / TICKS_PER_SECOND as u64;
        let days = seconds / 86_400;
        if days > 0 {
            write!(f, "{days}.")?;
        }
        write!(
            f,
            "{:02}:{:02}:{:02}",
            seconds / 3_600 % 24,
            seconds / 60 % 60,
            seconds % 60
        )?;
        if fraction > 0 {
            write!(f, ".{fraction:07}")?;
        }
        Ok(())
    }
}

impl ConfigType for TimeSpan {
    const TYPE_NAME: &'static str = "TimeSpan";

    fn parse_text(text: &str) -> Result<Self, ParseFailure> {
        parse_time_span(text).ok_or_else(ParseFailure::text)
    }
}

/// A number in the text, being its value and the zeros before it, which a
/// fraction of a second needs.
#[derive(Clone, Copy, Default)]
struct Number {
    value: u32,
    zeros: u32,
}

/// The part of a period a number gives.
#[derive(Clone, Copy)]
enum Part {
    Days,
    Hours,
    Minutes,
    Seconds,
    Fraction,
}

use Part::{Days, Fraction, Hours, Minutes, Seconds};

/// One way of writing a period, being the separators between its numbers
/// and the part each number gives.
struct Shape {
    separators: &'static [&'static str],
    parts: &'static [Part],
}

/// The ways .NET reads a period, in the order it tries them. A shape whose
/// separators match is tried, and the next is tried when its numbers are
/// out of range. The invariant culture writes days as `d.hh` and a
/// culture's own form, whose decimal separator is a full stop here, writes
/// them as `d:hh`, which is why `24:00:00` is 24 days.
const SHAPES: &[Shape] = &[
    Shape {
        separators: &[],
        parts: &[Days],
    },
    Shape {
        separators: &[":"],
        parts: &[Hours, Minutes],
    },
    Shape {
        separators: &[":", ":"],
        parts: &[Hours, Minutes, Seconds],
    },
    Shape {
        separators: &[".", ":"],
        parts: &[Days, Hours, Minutes],
    },
    Shape {
        separators: &[":", ":."],
        parts: &[Hours, Minutes, Fraction],
    },
    Shape {
        separators: &[":", ":"],
        parts: &[Days, Hours, Minutes],
    },
    Shape {
        separators: &[":", ":", "."],
        parts: &[Hours, Minutes, Seconds, Fraction],
    },
    Shape {
        separators: &[".", ":", ":"],
        parts: &[Days, Hours, Minutes, Seconds],
    },
    Shape {
        separators: &[".", ":", ":."],
        parts: &[Days, Hours, Minutes, Fraction],
    },
    Shape {
        separators: &[":", ":", ":"],
        parts: &[Days, Hours, Minutes, Seconds],
    },
    Shape {
        separators: &[":", ":", ":."],
        parts: &[Days, Hours, Minutes, Fraction],
    },
    Shape {
        separators: &[".", ":", ":", "."],
        parts: &[Days, Hours, Minutes, Seconds, Fraction],
    },
    Shape {
        separators: &[":", ":", ":", "."],
        parts: &[Days, Hours, Minutes, Seconds, Fraction],
    },
];

/// Read `text` as .NET's `TimeSpan.TryParse(string, out TimeSpan)` does
/// under a culture whose decimal separator is a full stop, such as the
/// invariant culture or English. White space around the text is ignored,
/// an optional `-` comes first, and the numbers are written in one of the
/// [`SHAPES`].
fn parse_time_span(text: &str) -> Option<TimeSpan> {
    let text = text.trim();
    if text.is_empty() {
        return None;
    }
    let (numbers, separators) = split(text)?;
    let negative = match separators.first().copied() {
        Some("") => false,
        Some("-") => true,
        _ => return None,
    };
    if separators.last().copied() != Some("") {
        return None;
    }
    let between = &separators[1..separators.len() - 1];
    let ticks = SHAPES
        .iter()
        .filter(|shape| shape.parts.len() == numbers.len() && shape.separators == between)
        .find_map(|shape| ticks(negative, shape.parts, &numbers))?;
    if !negative {
        return Some(TimeSpan::from_ticks(ticks));
    }
    // .NET negates with wrapping arithmetic, so the most negative period,
    // whose magnitude wrapped, comes through.
    let ticks = ticks.wrapping_neg();
    (ticks <= 0).then_some(TimeSpan::from_ticks(ticks))
}

/// The numbers in `text` and the separators before, between and after
/// them, of which the first and last may be empty. Any character that is
/// not an ASCII digit is part of a separator.
fn split(text: &str) -> Option<(Vec<Number>, Vec<&str>)> {
    let mut numbers = Vec::new();
    let mut separators = Vec::new();
    let mut rest = text;
    if rest.starts_with(|c: char| c.is_ascii_digit()) {
        separators.push("");
    }
    while !rest.is_empty() {
        let end = rest
            .find(|c: char| !c.is_ascii_digit())
            .unwrap_or(rest.len());
        if end == 0 {
            let end = rest
                .find(|c: char| c.is_ascii_digit())
                .unwrap_or(rest.len());
            separators.push(&rest[..end]);
            rest = &rest[end..];
            continue;
        }
        numbers.push(number(&rest[..end])?);
        rest = &rest[end..];
    }
    if separators.len() == numbers.len() {
        separators.push("");
    }
    (numbers.len() <= MAX_NUMBERS).then_some((numbers, separators))
}

/// Read a run of digits, refusing a value from 2^28 up as .NET does.
fn number(digits: &str) -> Option<Number> {
    let significant = digits.trim_start_matches('0');
    let zeros = (digits.len() - significant.len()) as u32;
    let mut value: u32 = 0;
    for digit in significant.bytes() {
        value = value * 10 + u32::from(digit - b'0');
        if value >= NUMBER_LIMIT {
            return None;
        }
    }
    Some(Number { value, zeros })
}

/// The ticks the numbers give as `parts`, before any sign, or `None` when
/// one is out of range or the period is too long, as .NET's
/// `TryTimeToTicks` works them out.
fn ticks(negative: bool, parts: &[Part], numbers: &[Number]) -> Option<i64> {
    let (mut days, mut hours, mut minutes, mut seconds) = (0, 0, 0, 0);
    let mut fraction = Number::default();
    for (part, number) in parts.iter().zip(numbers) {
        match part {
            Days => days = number.value,
            Hours => hours = number.value,
            Minutes => minutes = number.value,
            Seconds => seconds = number.value,
            Fraction => fraction = *number,
        }
    }
    if days > MAX_DAYS || hours > MAX_HOURS || minutes > MAX_MINUTES || seconds > MAX_SECONDS {
        return None;
    }
    let fraction = fraction_ticks(fraction)?;
    let milliseconds = (i64::from(days) * 86_400
        + i64::from(hours) * 3_600
        + i64::from(minutes) * 60
        + i64::from(seconds))
        * 1_000;
    if milliseconds > MAX_MILLISECONDS {
        return None;
    }
    let ticks = (milliseconds * 10_000).wrapping_add(i64::from(fraction));
    (negative || ticks >= 0).then_some(ticks)
}

/// The ticks a fraction of a second gives, as .NET's
/// `NormalizeAndValidateFraction` reads its digits. Seven digits are ticks,
/// fewer are padded, and more are rounded half away from zero, unless the
/// fraction has no leading zero and is more than seven digits.
fn fraction_ticks(fraction: Number) -> Option<u32> {
    if fraction.value == 0 {
        return Some(0);
    }
    if fraction.zeros == 0 && fraction.value > MAX_FRACTION {
        return None;
    }
    let digits = fraction.value.ilog10() + 1 + fraction.zeros;
    if digits <= FRACTION_DIGITS {
        return Some(fraction.value * 10u32.pow(FRACTION_DIGITS - digits));
    }
    let excess = digits - FRACTION_DIGITS;
    // A value below 2^28 over ten digits or more rounds to nothing.
    if excess >= 10 {
        return Some(0);
    }
    let divisor = 10u64.pow(excess);
    Some(((u64::from(fraction.value) + divisor / 2) / divisor) as u32)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Each text with the ticks .NET 8.0.33's `TimeSpan.TryParse` gave for
    /// it under the culture `en-GB`, or `None` where it failed, and the text
    /// .NET's `ToString` gave for those ticks.
    const DOTNET: &[(&str, Option<(i64, &str)>)] = &[
        ("0", Some((0, "00:00:00"))),
        ("5", Some((4320000000000, "5.00:00:00"))),
        ("-5", Some((-4320000000000, "-5.00:00:00"))),
        ("+5", None),
        (" 5 ", Some((4320000000000, "5.00:00:00"))),
        ("05", Some((4320000000000, "5.00:00:00"))),
        ("0005", Some((4320000000000, "5.00:00:00"))),
        ("1:2", Some((37200000000, "01:02:00"))),
        ("01:02", Some((37200000000, "01:02:00"))),
        ("1:2:3", Some((37230000000, "01:02:03"))),
        ("01:02:03", Some((37230000000, "01:02:03"))),
        ("1.02:03", Some((937800000000, "1.02:03:00"))),
        ("1.02:03:04", Some((937840000000, "1.02:03:04"))),
        ("1.02:03:04.5", Some((937845000000, "1.02:03:04.5000000"))),
        ("02:03:04.5", Some((73845000000, "02:03:04.5000000"))),
        ("02:03:04.1234567", Some((73841234567, "02:03:04.1234567"))),
        ("02:03:04.12345678", None),
        ("02:03:04.123456789", None),
        ("00:00:00.350", Some((3500000, "00:00:00.3500000"))),
        ("1.00:00:00", Some((864000000000, "1.00:00:00"))),
        ("1:00:00:00", Some((864000000000, "1.00:00:00"))),
        ("1:02:03:04.5", Some((937845000000, "1.02:03:04.5000000"))),
        ("24:00:00", Some((20736000000000, "24.00:00:00"))),
        ("23:59:59", Some((863990000000, "23:59:59"))),
        ("00:60:00", None),
        ("00:59:60", None),
        ("-00:01:00", Some((-600000000, "-00:01:00"))),
        ("- 00:01:00", None),
        ("-1.02:03:04", Some((-937840000000, "-1.02:03:04"))),
        (
            "10675199.02:48:05.4775807",
            Some((9223372036854775807, "10675199.02:48:05.4775807")),
        ),
        ("10675199.02:48:05.4775808", None),
        ("10675200", None),
        ("10675199", Some((9223371936000000000, "10675199.00:00:00"))),
        ("365", Some((315360000000000, "365.00:00:00"))),
        ("366", Some((316224000000000, "366.00:00:00"))),
        ("365.00:00:00", Some((315360000000000, "365.00:00:00"))),
        (
            "365.00:00:00.0000001",
            Some((315360000000001, "365.00:00:00.0000001")),
        ),
        ("", None),
        (" ", None),
        ("x", None),
        ("1e3", None),
        ("1.5", None),
        ("1,5", None),
        ("00:00:00,5", None),
        ("0:0:0", Some((0, "00:00:00"))),
        ("000000001:00", Some((36000000000, "01:00:00"))),
        ("00:00:00.", None),
        (".5", None),
        ("1.", None),
        ("1:", None),
        (":1", None),
        ("1::2", None),
        ("1:2:3:4:5", None),
        ("00:01:00\u{0}", None),
        ("\u{9}00:01:00\u{a}", Some((600000000, "00:01:00"))),
        ("\u{ff11}", None),
        ("00:00:00.0000000001", Some((0, "00:00:00"))),
        ("00:00:00.0000001", Some((1, "00:00:00.0000001"))),
        ("00:00:00.00000001", Some((0, "00:00:00"))),
        ("00:00:00.00000005", Some((1, "00:00:00.0000001"))),
        ("00:00:00.00000000", Some((0, "00:00:00"))),
        ("2147483647", None),
        ("99999999", None),
        ("0000000000000000001", Some((864000000000, "1.00:00:00"))),
        ("01:02:03.000", Some((37230000000, "01:02:03"))),
        ("1.2:3", Some((937800000000, "1.02:03:00"))),
        ("1.2", None),
        ("1.2.3", None),
        ("P1D", None),
        ("1d", None),
        ("00:01", Some((600000000, "00:01:00"))),
        ("0:1:2", Some((620000000, "00:01:02"))),
        ("-0", Some((0, "00:00:00"))),
        ("-00:00:00", Some((0, "00:00:00"))),
        ("23:59:59.9999999", Some((863999999999, "23:59:59.9999999"))),
        ("00:00:00.9999999", Some((9999999, "00:00:00.9999999"))),
        ("1:2:3.4", Some((37234000000, "01:02:03.4000000"))),
        ("1.2:3:4.5", Some((937845000000, "1.02:03:04.5000000"))),
        ("1.2:3.4", None),
        ("10:20:30:40", Some((9378400000000, "10.20:30:40"))),
        ("1:24:00:00", None),
        ("1:23:60:00", None),
        ("001.02:03:04", Some((937840000000, "1.02:03:04"))),
        ("1.002:03:04", Some((937840000000, "1.02:03:04"))),
        ("00:00:000", Some((0, "00:00:00"))),
        ("00:00:00.0123456", Some((123456, "00:00:00.0123456"))),
        ("00:00:00.01234560", Some((123456, "00:00:00.0123456"))),
        ("-0:0:0.1", Some((-1000000, "-00:00:00.1000000"))),
        ("--1", None),
        ("+-1", None),
        ("-+1", None),
        ("+00:01:00", None),
        ("0x10", None),
        ("1 ", Some((864000000000, "1.00:00:00"))),
        (" 1", Some((864000000000, "1.00:00:00"))),
        ("1\u{a0}", Some((864000000000, "1.00:00:00"))),
        ("1 .2:3", None),
        ("1. 2:3", None),
        ("1 :2", None),
        ("1: 2", None),
        ("00:00:00 .5", None),
        ("00:00:00. 5", None),
        ("01:02:03\u{0}\u{0}", None),
        ("1\u{0}", None),
        ("\u{0}1", None),
        ("1:2:3\u{3000}", Some((37230000000, "01:02:03"))),
        ("00:00:59.9999999", Some((599999999, "00:00:59.9999999"))),
        (
            "106751.23:47:16.8547758",
            Some((92233720368547758, "106751.23:47:16.8547758")),
        ),
        ("1:2:3:4.5", Some((937845000000, "1.02:03:04.5000000"))),
        (
            "11:22:33:44.55",
            Some((10316245500000, "11.22:33:44.5500000")),
        ),
        ("0:00:00:00", Some((0, "00:00:00"))),
        ("00:00:00:00", Some((0, "00:00:00"))),
        ("-1:2:3:4", Some((-937840000000, "-1.02:03:04"))),
        ("1:2.3", None),
        ("2147483648", None),
        (
            "10675199.02:48:05",
            Some((9223372036850000000, "10675199.02:48:05")),
        ),
        ("10675199.02:48:06", None),
        (
            "10675198.23:59:59.9999999",
            Some((9223371935999999999, "10675198.23:59:59.9999999")),
        ),
        (
            "-10675199.02:48:05.4775808",
            Some((-9223372036854775808, "-10675199.02:48:05.4775808")),
        ),
        ("-10675199.02:48:05.4775809", None),
        ("365.0:0:0", Some((315360000000000, "365.00:00:00"))),
        ("365:0:0:0", Some((315360000000000, "365.00:00:00"))),
        ("00:00:00.0268435456", None),
        ("00:00:00.0268435455", Some((268435, "00:00:00.0268435"))),
        ("00:00:00.0999999995", None),
        ("00:00:00.0123456789", Some((123457, "00:00:00.0123457"))),
        ("00:00:00.000000150", Some((2, "00:00:00.0000002"))),
        ("00:00:00.000000250", Some((3, "00:00:00.0000003"))),
        ("00:00:00.000000015", Some((0, "00:00:00"))),
        ("00:00:00.00000000000000005", Some((0, "00:00:00"))),
        ("00:00:00.0000000268435455", Some((0, "00:00:00"))),
        (
            "00:00:00.0000000000000000000000000000000000000001",
            Some((0, "00:00:00")),
        ),
        ("1.2:3:4.0268435456", None),
        ("-00:00:00.0000001", Some((-1, "-00:00:00.0000001"))),
        (
            "00000000000000000000000000000001.00:00:00",
            Some((864000000000, "1.00:00:00")),
        ),
        ("1:2:3:4:5:6", None),
        ("1.2.3.4.5.6", None),
        ("-1", Some((-864000000000, "-1.00:00:00"))),
        ("-1:2", Some((-37200000000, "-01:02:00"))),
        ("- 1", None),
        ("\u{2212}1", None),
        ("1 2", None),
        ("\u{ff11}:2", None),
        ("1:\u{ff12}", None),
        ("1.1:2:3.4", Some((901234000000, "1.01:02:03.4000000"))),
        ("268435455", None),
        ("268435456", None),
        ("00:268435456", None),
        ("1:2:3:.4", Some((937804000000, "1.02:03:00.4000000"))),
        ("1.2:3:.4", Some((937804000000, "1.02:03:00.4000000"))),
        ("1:2:3:4:.5", None),
        ("1:2:.3", Some((37203000000, "01:02:00.3000000"))),
        ("00:02:00", Some((1200000000, "00:02:00"))),
        ("2:30", Some((90000000000, "02:30:00"))),
        ("00:00:01.5", Some((15000000, "00:00:01.5000000"))),
        ("1.2:3:4", Some((937840000000, "1.02:03:04"))),
        (" 00:01:00 ", Some((600000000, "00:01:00"))),
        ("0:0:0.12345678", None),
        ("10675200.00:00:00", None),
        (":30", None),
        ("-1.00:00:00", Some((-864000000000, "-1.00:00:00"))),
        ("00:00:59.9", Some((599000000, "00:00:59.9000000"))),
        ("0:0", Some((0, "00:00:00"))),
        ("01:02:03:04", Some((937840000000, "1.02:03:04"))),
        ("-", None),
        ("1.1.1", None),
        ("00:00:60", None),
        ("00:59", Some((35400000000, "00:59:00"))),
        ("99:00", None),
        ("1:0:60", None),
        ("0.23:59:59", Some((863990000000, "23:59:59"))),
        ("1 00:00:00", None),
        ("-0:0:1", Some((-10000000, "-00:00:01"))),
        ("00:00:01.0000001", Some((10000001, "00:00:01.0000001"))),
        (
            "24855.03:14:07",
            Some((21474836470000000, "24855.03:14:07")),
        ),
        (
            "24855.03:14:08",
            Some((21474836480000000, "24855.03:14:08")),
        ),
        (
            "-24855.03:14:08",
            Some((-21474836480000000, "-24855.03:14:08")),
        ),
        (
            "-24855.03:14:09",
            Some((-21474836490000000, "-24855.03:14:09")),
        ),
        ("1:2.5", None),
        ("1:2:3.", None),
        ("0000000000001", Some((864000000000, "1.00:00:00"))),
        ("000:000:000", Some((0, "00:00:00"))),
        ("1:02:3", Some((37230000000, "01:02:03"))),
        ("12345678901234567890", None),
        ("10675199.23:59:59", None),
        ("0:0:0:0", Some((0, "00:00:00"))),
        ("00:00:00.0000000", Some((0, "00:00:00"))),
        ("\u{a0}00:01:00", Some((600000000, "00:01:00"))),
        ("00:01:00\u{a0}", Some((600000000, "00:01:00"))),
        ("\u{ff11}:00", None),
        ("00:01:00\u{d}\u{a}", Some((600000000, "00:01:00"))),
        ("1:2:3:4:5.6", None),
        ("1.2:3:4:5", None),
        ("1:2:3.4.5", None),
    ];

    #[test]
    fn periods_are_read_as_dotnet_reads_them() {
        for (text, expected) in DOTNET {
            let parsed = parse_time_span(text);
            assert_eq!(
                parsed.map(TimeSpan::ticks),
                expected.map(|(ticks, _)| ticks),
                "{text:?}"
            );
        }
    }

    #[test]
    fn periods_are_written_as_dotnet_writes_them() {
        for (text, expected) in DOTNET {
            if let Some((ticks, written)) = expected {
                assert_eq!(
                    TimeSpan::from_ticks(*ticks).to_string(),
                    *written,
                    "{text:?}"
                );
            }
        }
    }

    #[test]
    fn a_period_converts_to_and_from_a_duration() {
        let span = TimeSpan::parse_text("1.02:03:04.5").unwrap();
        let duration = Duration::new(93_784, 500_000_000);
        assert_eq!(span.to_duration(), Some(duration));
        assert_eq!(TimeSpan::from_duration(duration), Some(span));
        assert_eq!(TimeSpan::from_ticks(-1).to_duration(), None);
        assert_eq!(
            TimeSpan::from_duration(Duration::from_nanos(150)),
            Some(TimeSpan::from_ticks(1))
        );
        assert_eq!(TimeSpan::from_duration(Duration::MAX), None);
        assert_eq!(
            TimeSpan::from_ticks(i64::MAX).to_duration(),
            Some(Duration::new(922_337_203_685, 477_580_700))
        );
    }

    #[test]
    fn a_period_that_is_not_text_dotnet_reads_is_refused() {
        assert_eq!(TimeSpan::parse_text("x"), Err(ParseFailure::text()));
        assert_eq!(TimeSpan::TYPE_NAME, "TimeSpan");
    }
}
