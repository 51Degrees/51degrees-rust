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

//! Builds a signed 51Did the way the 51Degrees cloud would, then reads it back
//! with [`fodid::FodId`] and verifies its signature.
//!
//! Run with: `cargo run --example parse_and_verify`

use fodid::{Creator, Crypto, FodId, SignatureStatus};

/// Where to go next, printed after the results.
const FIND_OUT_MORE: &[(&str, &str)] = &[
    (
        "What a 51Did is and how it is used",
        "https://51degrees.com/documentation/_identifiers_51_did.html?utm_source=code&utm_medium=example&utm_campaign=51degrees-rust&utm_content=fodid-examples-parse_and_verify.rs&utm_term=find-out-more-51did",
    ),
    (
        "The 51Did inspector, a visual breakdown of an identifier",
        "https://51degrees.com/developers/51did-inspector?utm_source=code&utm_medium=example&utm_campaign=51degrees-rust&utm_content=fodid-examples-parse_and_verify.rs&utm_term=find-out-more-51did-inspector",
    ),
    (
        "The layout of a 51Did",
        "https://github.com/51Degrees/specifications/blob/main/did-specification/identifier-layout.md",
    ),
    (
        "The OWID envelope a 51Did travels in",
        "https://github.com/SWAN-community/owid/blob/main/explainer.md",
    ),
    ("51Degrees for Rust", "https://github.com/51Degrees/51degrees-rust"),
    ("OWID for Rust", "https://github.com/SWAN-community/owid-rust"),
];

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // The cloud holds an ECDSA P-256 key and signs every 51Did it issues.
    // Here we stand in for it with a freshly generated key pair.
    let crypto = Crypto::new();
    let creator = Creator::new("51degrees.com", crypto.clone())?;

    // A 38-byte 51Did payload: flags, little endian License Id, 32-byte
    // match key and terms index.
    let mut payload = vec![0u8; 38];
    // The flags byte says a probabilistic identifier (bits 6 and 7 clear) in
    // payload version 0 (bits 4 and 5 clear), created for standard marketing
    // (bits 0 and 1 set), a usage the caller stated directly (bit 3 clear).
    payload[0] = 0b0000_0011;
    payload[1..5].copy_from_slice(&0x1234_5678u32.to_le_bytes()); // License Id
    for (i, b) in payload[5..37].iter_mut().enumerate() {
        *b = 0x20 + i as u8; // a stable, recognizable match key
    }
    // The cloud writes the index of the terms document in the byte after the
    // match key. Index 1 is the Model Terms for Marketing, version 2.
    payload[37] = 1;

    // The cloud creates, signs and base64 encodes the envelope in one step;
    // that string is the 51Did the caller receives.
    let signed = creator.create(payload)?;
    let base64 = signed.as_base64()?;
    println!("51Did (base64): {base64}");

    // The recipient reads it back. Reading answers only whether the string
    // is a 51Did, and says nothing about the signature.
    let fod_id = FodId::from_base64(&base64)?;
    println!("usage     : {:?}", fod_id.usage());
    println!("indirect  : {}", fod_id.usage_is_indirect());
    println!("id_type   : {:?}", fod_id.id_type());
    println!("license_id: {:#010x}", fod_id.license_id());
    println!("match_key : {}", hex(fod_id.match_key()));
    println!("terms     : {:?}", fod_id.terms());

    // OWID level fields are reachable directly through Deref.
    println!("domain    : {}", fod_id.domain());
    println!("date      : {}", fod_id.date());

    // Verify the signature in process against the issuer public key. Only
    // SignatureStatus::Invalid would mean the identifier should be
    // distrusted; a key that cannot be read is reported as a key fault.
    let public_pem = crypto.public_key_pem()?;
    let status = fod_id.verify_status_with_public_key(&public_pem);
    println!("signature : {status}");
    assert_eq!(status, SignatureStatus::Valid);

    println!();
    println!("Find out more");
    println!("-------------");
    for (label, url) in FIND_OUT_MORE {
        println!("{label}");
        println!("  {url}");
    }

    Ok(())
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Runs the example under `cargo test`, so that a change which stops the
/// example working fails the tests.
#[test]
fn the_example_runs_to_the_end() -> Result<(), Box<dyn std::error::Error>> {
    main()
}
