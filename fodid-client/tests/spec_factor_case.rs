//! A factor word is read whatever its letter case.
//!
//! did-specification/package-surface.md lines 94 to 97 at
//! dbaf099c712e2bd3a2a80bfb4eaf284b079f05eb: "The server sends these as the
//! lower case words `verified`, `mismatch`, `misconfigured` and
//! `notrecorded`. The comparison ignores case, as .NET does, so a package
//! whose language compares exactly, such as Java, MUST lower-case the word
//! before it compares."

use fodid_client::FactorOutcome;

#[test]
fn each_factor_word_is_read_whatever_its_letter_case() {
    let cases = [
        ("Verified", FactorOutcome::Verified),
        ("VERIFIED", FactorOutcome::Verified),
        ("vErIfIeD", FactorOutcome::Verified),
        ("Mismatch", FactorOutcome::Mismatch),
        ("MISMATCH", FactorOutcome::Mismatch),
        ("Misconfigured", FactorOutcome::Misconfigured),
        ("MISCONFIGURED", FactorOutcome::Misconfigured),
        ("NotRecorded", FactorOutcome::NotRecorded),
        ("NOTRECORDED", FactorOutcome::NotRecorded),
    ];
    for (word, expected) in cases {
        assert_eq!(
            FactorOutcome::from_cloud(Some(word)),
            expected,
            "the comparison ignores case, so {word:?} reads as {expected:?}"
        );
    }
}

#[test]
fn the_lower_case_words_are_still_read() {
    for expected in [
        FactorOutcome::Verified,
        FactorOutcome::Mismatch,
        FactorOutcome::Misconfigured,
        FactorOutcome::NotRecorded,
    ] {
        assert_eq!(
            FactorOutcome::from_cloud(Some(expected.as_cloud())),
            expected
        );
    }
}

#[test]
fn an_unknown_word_in_any_case_is_still_a_mismatch() {
    for word in ["Unknown", "PASS", ""] {
        assert_eq!(
            FactorOutcome::from_cloud(Some(word)),
            FactorOutcome::Mismatch
        );
    }
    assert_eq!(FactorOutcome::from_cloud(None), FactorOutcome::Mismatch);
}
