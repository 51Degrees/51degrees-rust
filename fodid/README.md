# 51Degrees Identifier

[![51Degrees](https://51degrees.com/img/logo.png?utm_source=github&utm_medium=readme&utm_campaign=51degrees-rust&utm_content=fodid-readme.md&utm_term=logo "Data rewards the curious")](https://51degrees.com/?utm_source=github&utm_medium=readme&utm_campaign=51degrees-rust&utm_content=fodid-readme.md&utm_term=logo)
**Pipeline API**

[Developer Documentation](https://51degrees.com/documentation/index.html?utm_source=github&utm_medium=readme&utm_campaign=51degrees-rust&utm_content=fodid-readme.md&utm_term=documentation)

## Introduction

A Rust reader for the **51Degrees identifier** (51Did) returned by the
51Degrees cloud service. The
[identifiers documentation](https://51degrees.com/documentation/_identifiers__index.html?utm_source=github&utm_medium=readme&utm_campaign=51degrees-rust&utm_content=fodid-readme.md&utm_term=51did)
describes what a 51Did is and how it is used. This crate parses the 51Did byte
layout, which is carried in a signed
[OWID](https://github.com/SWAN-community/owid) envelope. For the OWID envelope
concept see the
[OWID explainer](https://github.com/SWAN-community/owid/blob/main/explainer.md).

The code blocks in this file are compiled and run as documentation tests of
the crate, so they stay true to the code.

## What a 51Did is

A 51Did is described at three levels, and this crate keeps them distinct.

- The **51Did** is the identifier as a whole, meaning the concept together
  with the rules for how it is issued, compared and licensed. "A 51Did" means
  the identifier in this complete sense, not any single field.
- The **envelope** (also called the **wrapper**) is the data model that
  carries a 51Did. It is a signed
  [OWID](https://github.com/SWAN-community/owid) (the SWAN community schema
  that defines the binary layout, signature and verification rules), holding
  the version, domain, date, payload and signature. It changes byte for byte
  every time the cloud issues one, even for the same inputs, because the date
  and signature change with each call.
- The **match key** is the part of the envelope that is stable and
  comparable. It is the payload bytes after the flags and license fields,
  read through `FodId::match_key`. Two 51Dids for the same inputs share the
  same match key even though their envelopes differ. Compare match keys,
  never envelopes.

## Identifier types

The flags byte carries the identifier type, read through `FodId::id_type`,
which determines the length and meaning of the match key:

- `IdType::Probabilistic` (the default; legacy identifiers decode as this)
  and `IdType::HashedEmail` carry a 32-byte SHA-256.
- `IdType::Random` carries a 16-byte server-generated GUID.
- `IdType::Reserved` is not yet assigned and is parsed best effort.

## The terms the identifier was created under

The byte after the match key says which terms document the 51Did was created
under, so that the terms travel with the identifier rather than beside it. It
is an index into a table published in the
[specification](https://github.com/51Degrees/specifications/blob/main/did-specification/identifier-layout.md)
and is not a version number. `FodId::terms` answers with the address of the
document, so a caller never handles the byte.

| Index | Document | `FodId::terms` |
| --- | --- | --- |
| `0` | Not stated in the identifier | `None` |
| `1` | Model Terms for Marketing, version 2 | `Some("https://m4ow.uk/mtm/2.txt")` |
| any other | One this crate cannot name | `None` |

An identifier whose payload ends at the match key carries no terms byte, and
a missing byte is index 0, which answers with no address. An index added to
the specification after this release answers with no address as well, and no
address is ever built from an index this crate cannot name, since that would
name a document nobody wrote. A caller therefore cannot tell an index of
zero from an index this crate cannot name, which is deliberate, because both
lead to the same place. This crate answers with the address and never
fetches it.

## The payload version

Bits 4 and 5 of the flags byte say which payload layout the identifier
follows, and this crate reads version 0. A payload naming version 1, 2 or 3
is refused with `Error::UnsupportedPayloadVersion`, which names the version
it found.

No field is read under the layout this crate knows once the version says
otherwise. A later version exists precisely because a field moved, so
reading such a payload here would answer with values that are wrong rather
than absent, which is worse than refusing. A version that nothing checks
protects nothing.

The version is not exposed. Either this crate read the layout, in which case
the accessors are the answer, or it did not, in which case there is no
identifier to read fields from.

## A payload with no usage

`Usage` has exactly three values, being `NonMarketing`, `Standard` and
`Personalized`. A payload whose usage bits 0 to 2 are all clear is refused
with `Error::NoUsage`. Every usage the cloud accepts sets bit 0, so such a
payload is damaged or forged, and the only safe answer to it is not to pass
the identifier on, which the refusal already gives.

`usage_is_indirect()` answers whether the usage was stated by the caller
(`false`) or worked out by the issuer from another signal the caller sent
(`true`). Today a consent string is the only such signal.

## Payload layout

The payload is a five byte header, being a flags byte and a four byte little
endian License Id, followed by the match key. Every field is read through a
typed accessor on `FodId`, and the offsets and lengths are internal to the
crate, because the only use a caller has for an offset is to read a field out
of the payload by hand, and that is how the usage comes out wrong. The layout
is specified at
[identifier-layout.md](https://github.com/51Degrees/specifications/blob/main/did-specification/identifier-layout.md)
and the accessors every 51Did package offers at
[package-surface.md](https://github.com/51Degrees/specifications/blob/main/did-specification/package-surface.md),
and those two pages are the authority rather than any summary here.

A terms byte follows the match key, read through `FodId::terms`, which
answers with the address of the document the identifier was created under.
Where it sits depends on the match key length the type requires, which is
one reason the offsets stay internal.

The lengths given there are lower bounds. The payload must hold the header
before the type can be read, and then the value the type requires, being 16
GUID bytes for a random identifier and 32 hash bytes for a probabilistic or
hashed email one. The terms byte follows the value, so where it sits depends
on the value length the type requires, and a payload that ends at the value
carries none. A payload may carry more bytes after the terms, which this
crate accepts and leaves in place. There is no upper bound on a 51Did in this
crate, so a reader built today keeps reading identifiers issued in a newer,
longer shape.

`FodId` derefs to the underlying `owid::Owid`, so a `FodId` value can be used
directly for all OWID level concerns (domain, date, payload bytes, signature,
base64 round tripping and signature verification) and adds typed accessors
for the payload fields on top.

## Usage

Reading answers one question, which is whether the input is a 51Did. It never
touches a key, so a `FodId` that comes back is not necessarily
cryptographically valid. Verifying the signature is a second question, asked
of the parsed value, and only `SignatureStatus::Invalid` means the identifier
should be distrusted.

```rust
use fodid::{FodId, SignatureStatus};

fn read(base64_from_cloud_service: &str, public_pem: &str) -> Result<(), fodid::Error> {
    let fod_id = FodId::from_base64(base64_from_cloud_service)?;

    let usage = fod_id.usage();          // the highest usage granted
    let indirect = fod_id.usage_is_indirect(); // stated or worked out
    let id_type = fod_id.id_type();      // IdType
    let license_id = fod_id.license_id(); // u32
    let match_key = fod_id.match_key();  // the match key bytes (SHA-256 or GUID)

    // The terms the identifier was created under, and the address of that
    // document where this crate knows the index.
    let terms = fod_id.terms();             // Option<&'static str>

    // Inherited OWID level fields and operations, available through Deref.
    let domain = fod_id.domain();
    let round_trip = fod_id.as_base64()?;

    // The second question, asked separately.
    let genuine = fod_id.verify_status_with_public_key(public_pem)
        == SignatureStatus::Valid;

    let _ = (usage, indirect, id_type, license_id, match_key);
    let _ = (domain, round_trip, genuine);
    let _ = terms;
    Ok(())
}
```

## Why a read can fail

A 51Did arrives from a cookie, a link or a response body that anyone could
have written, so malformed input is expected and a failed read is an ordinary
`Err` naming the reason, never a panic. Every result carries three facts:
whether the read succeeded, the value (present only on success, never a
partly read `FodId`), and the status, which is the `Error` variant on failure.
The status vocabulary is the OWID one plus four 51Did statuses, checked in
this order.

| Status | Meaning |
|---|---|
| `Error::Parse` | The bytes are not an OWID envelope. The OWID reason is kept unchanged inside and read with `.status()`, for example `ParseStatus::MissingInput`, `InvalidBase64`, `UnexpectedEnd` or `ByteCountMismatch`. |
| `Error::PayloadTooShort` | The envelope is fine, but the payload cannot hold the 5 byte 51Did header, so the identifier type cannot be read. |
| `Error::UnsupportedPayloadVersion` | The flags byte names a payload version other than 0, and the variant names the version found. |
| `Error::NoUsage` | The flags byte sets none of usage bits 0 to 2. |
| `Error::InvalidTypePayloadLength` | The header was read, and the payload is shorter than the value the identifier type requires (21 bytes in all for random, 37 for probabilistic and hashed email). |

All five are data results. `Error::Owid` is the one exceptional variant, and
no read produces it. It appears only when a caller uses `?` on an OWID
operation of a parsed value, such as serialising it again.

```rust
use fodid::{Error, FodId, ParseStatus};

let result = FodId::from_base64("not base 64!");
assert!(result.is_err());
match result.unwrap_err() {
    Error::Parse(e) => assert_eq!(e.status(), ParseStatus::InvalidBase64),
    Error::PayloadTooShort { expected, actual } => {
        println!("header needs {expected} bytes, {actual} present")
    }
    Error::InvalidTypePayloadLength { id_type, expected, actual } => {
        println!("{id_type:?} needs {expected} bytes, {actual} present")
    }
    other => unreachable!("a read never produces {other:?}"),
}
```

This crate applies no size limit to its input. Where an application needs
one, for example to bound what a public end point will accept, the limit
belongs at that application's own boundary, before the input reaches this
crate, and is that application's policy rather than a property of the 51Did
format.

## Comparing two 51Dids

Two 51Dids issued for the same device + IP + usage differ at the byte level
because the envelope embeds a fresh timestamp and signature on each call. The
byte-level difference is in the **envelope**. The **match key** carried inside
is stable. To decide whether two 51Dids refer to the same browser instance,
compare the match keys, never the full base64 envelopes.

```rust
use fodid::FodId;

fn same_browser(idprobglobal_a: &str, idprobglobal_b: &str) -> Result<bool, fodid::Error> {
    let a = FodId::from_base64(idprobglobal_a)?;
    let b = FodId::from_base64(idprobglobal_b)?;

    // a.date() and a.signature() differ from b's on every issue, because the
    // envelope is fresh each time. The match key is what stays the same.
    Ok(a.match_key() == b.match_key())
}
```

Use `match_key()` (a 32-byte SHA-256 or 16-byte GUID) as the cache /
dedup key.

## Migrating from the `owid` 1.0 crate surface

The OWID implementation this crate builds on was hardened so that an OWID
reaches a caller only from a successful read or from a creator that signs it,
and at the same time this crate stopped depending on an `owid` crate (see
"Where the OWID code comes from" below). Callers who reached the envelope
through this crate will find five changes.

OWID types are named through `fodid` rather than through an `owid` crate,
because there is no `owid` dependency to add any more. A test that signs an
envelope turns on the `creator` feature of `fodid`.

```text
// Before                                // After
use owid::{Owid, ParseStatus};           use fodid::{Owid, ParseStatus};
use owid::{Creator, Crypto};             use fodid::{Creator, Crypto};
                                         // with features = ["creator"]
```

The envelope fields are read through accessors rather than public fields, so
an OWID can no longer be altered after it was read or signed.

```text
// Before                                // After
let domain = &fod_id.domain;             let domain = fod_id.domain();
let issued = fod_id.date;                let issued = fod_id.date();
let bytes = &fod_id.payload;             let bytes = fod_id.payload();
let sig = &fod_id.signature;             let sig = fod_id.signature();
```

A failed read is `Error::Parse` carrying a `ParseError` with a named status,
where it used to be `Error::Owid` carrying the OWID error type (re-exported as
`fodid::OwidError`) whose only detail was its message.

```text
// Before
match FodId::from_base64(input) {
    Err(fodid::Error::Owid(e)) => log(e.to_string()),
    ..
}
// After
match FodId::from_base64(input) {
    Err(fodid::Error::Parse(e)) => log(e.status()),
    ..
}
```

Code that built a signed envelope in a test used `Creator::sign_bytes`, which
is now `Creator::create`. Nothing can construct an `Owid` directly any more,
and there is no unsigned state.

```text
// Before                                // After
creator.sign_bytes(payload)?             creator.create(payload)?
```

The functions that check a signature take the key alone, because a signature
covers its own OWID and nothing else. They used to take a list of other OWIDs
as well, and for a 51Did that list was always empty. A creator signs the same
way, so `Creator::create_with_others` is gone.

```text
// Before                                // After
fod_id.verify_with_public_key(pem, &[])  fod_id.verify_with_public_key(pem)
```

## Non goals

- **Signature verification on construction.** Reading a `FodId` does not check
  the signature. Call `verify_status_with_public_key` (inherited from
  `fodid::Owid` through `Deref`) when needed.
- **Construction of new 51Dids.** This is a reader. New 51Dids are issued by
  the 51Degrees cloud, which alone holds the signing key. The `creator`
  feature exists so tests and tools can stand in for the cloud, and is off by
  default.

## Where the OWID code comes from

This crate does not depend on an `owid` crate from crates.io or from git. The
OWID library is compiled into `fodid` as a private module from the
`owid-rust` submodule of this repository,
[SWAN-community/owid-rust](https://github.com/SWAN-community/owid-rust).
The script `ci/copy-owid-source.ps1` copies the source into `fodid/src/owid`
before every build, together with a `NOTICE` naming the exact commit the copy
came from and the library's own Apache 2.0 `LICENSE`, and the published crate
carries that copy. No OWID package therefore has to exist on any registry for
this crate to build, be published or be used.

The submodule follows the main branch of SWAN-community/owid-rust. A nightly
job raises a pull request in this repository whenever that branch moves, and
once the pull request passes its checks and is merged, the next release of
`fodid` carries the new OWID code.

The OWID types a caller needs are re-exported from `fodid` itself: `Owid`,
`ParseError`, `ParseStatus`, `SignatureStatus`, `Crypto`, `Version`,
`ParseDetail`, `OwidError` and `SIGNATURE_LENGTH`. The two types that create
and sign a new envelope, `Creator` and `Configuration`, are behind the
`creator` feature.

Working from a clone of the repository, run `git submodule update --init` and
then `pwsh ./ci/copy-owid-source.ps1` once before `cargo build`. The copied
directory is ignored by git, and the script can be run again at any time.

## See also

- [SWAN-community/owid-rust](https://github.com/SWAN-community/owid-rust) -
  the OWID envelope library compiled into this crate.
- The [51Did inspector](https://51degrees.com/developers/51did-inspector?utm_source=github&utm_medium=readme&utm_campaign=51degrees-rust&utm_content=fodid-readme.md&utm_term=51did-inspector) for a
  visual breakdown of the same byte layout.

## License

EUPL-1.2. See [LICENSE](../LICENSE).
