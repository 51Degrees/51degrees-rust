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

//! The 51Did cases.
//!
//! ```text
//! parity-rust fodid-sign --cases <json> --prefix <text> --out <json>
//! parity-rust fodid --corpus <json> [--corpus <json>] --out <jsonl>
//! ```
//!
//! `fodid-sign` signs each payload in the cases file with a key made for
//! the run and writes the list each language reads. `fodid` reads one or
//! more such lists and writes a line per case with every accessor, the
//! parse status and the signature status, named as .NET names them, or the
//! 51Did metadata. `parity/README.md` describes both formats.

use std::collections::{HashMap, HashSet};
use std::fmt::Write as _;
use std::fs::File;
use std::io::{BufWriter, Write as _};
use std::path::Path;

use anyhow::{anyhow, bail, ensure, Context, Result};
use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use base64::Engine as _;
use fiftyone_fodid_core::default_component_metadata;
use fiftyone_pipeline_core::PropertyValueType;
use fiftyone_pipeline_engines_fiftyone::FiftyOneAspectPropertyMetaData;
use fodid::{Creator, Crypto, Error, FodId, IdType, Usage};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

/// The mode that signs the cases.
pub const SIGN: &str = "fodid-sign";

/// The mode that reads the signed lists.
pub const READ: &str = "fodid";

/// The creator domain every payload is signed with.
const DOMAIN: &str = "parity.test";

/// The characters between two spaces in the `url-spaced` encoding.
const SPACED_GROUP_LENGTH: usize = 20;

/// The date as RFC 3339 in UTC. The envelope holds whole minutes.
const DATE_FORMAT: &str = "%Y-%m-%dT%H:%M:%SZ";

/// The status of a 51Did that was read.
const PARSED: &str = "Parsed";

/// One case of the cases file. Exactly one of `payload`, `text` and
/// `metadata` is given, and the other fields apply to a payload only.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Case {
    id: String,
    about: String,
    /// Hex bytes to sign. White space between digits is ignored.
    payload: Option<String>,
    /// Text given to the reader as it is, never signed.
    text: Option<String>,
    #[serde(default)]
    metadata: bool,
    encoding: Option<Encoding>,
    /// Hex bytes added after the envelope before it is encoded.
    trailing: Option<String>,
    /// Changes the last signature byte after signing.
    #[serde(default)]
    tamper: bool,
}

/// How a signed envelope is given to the reader.
#[derive(Deserialize, Clone, Copy)]
#[serde(rename_all = "kebab-case")]
enum Encoding {
    /// The standard alphabet with padding, as the cloud issues it.
    Standard,
    /// The URL-safe alphabet with no padding.
    Url,
    /// `Url` with a space before it and a line feed after it.
    UrlSurrounded,
    /// `Url` with a space between each group of [`SPACED_GROUP_LENGTH`]
    /// characters.
    UrlSpaced,
    /// The envelope bytes.
    Bytes,
}

/// How the reader is given a case's input.
#[derive(Serialize, Deserialize, Clone, Copy)]
#[serde(rename_all = "lowercase")]
enum Form {
    /// Read with `FodId::from_base64`.
    Text,
    /// Hex bytes, read with `FodId::from_byte_array`.
    Bytes,
    /// No input, the line gives the 51Did metadata.
    Metadata,
}

/// One entry of a signed list, the corpus both languages read.
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Signed {
    id: String,
    about: String,
    form: Form,
    input: Option<String>,
    /// The signing key as SPKI PEM, absent for a case that was not signed.
    public_key: Option<String>,
    tamper: bool,
}

/// Reads the value of one 51Did field, given the signing key if any.
type Accessor = fn(&FodId, Option<&str>) -> Value;

/// `fodid-sign`: signs each payload of `--cases` and writes the list, with
/// `--prefix` before every id.
pub fn sign(args: &[String]) -> Result<()> {
    let mut flags = flag_values(args, &["--cases", "--prefix", "--out"])?;
    let cases_path = single(&mut flags, "--cases")?;
    let prefix = single(&mut flags, "--prefix")?;
    let out = single(&mut flags, "--out")?;
    let cases: Vec<Case> = read_json(Path::new(&cases_path))?;
    if let Some(id) = repeated(cases.iter().map(|case| case.id.as_str())) {
        bail!("{cases_path}: case {id} is given more than once");
    }

    let crypto = Crypto::new();
    let public_key = crypto
        .public_key_pem()
        .map_err(|error| anyhow!("writing the public key: {error}"))?;
    let creator =
        Creator::new(DOMAIN, crypto).map_err(|error| anyhow!("making the creator: {error}"))?;
    let mut signed = Vec::with_capacity(cases.len());
    for case in &cases {
        signed.push(sign_case(case, &prefix, &creator, &public_key)?);
    }

    let mut writer = BufWriter::new(File::create(&out).with_context(|| format!("creating {out}"))?);
    serde_json::to_writer_pretty(&mut writer, &signed)?;
    writer.write_all(b"\n")?;
    writer.flush()?;
    eprintln!("{} 51Did cases signed into {out}", signed.len());
    Ok(())
}

fn sign_case(case: &Case, prefix: &str, creator: &Creator, public_key: &str) -> Result<Signed> {
    let id = format!("{prefix}{}", case.id);
    let about = case.about.clone();
    let (form, input, public_key, tamper) = match (&case.payload, &case.text, case.metadata) {
        (Some(payload), None, false) => {
            let mut bytes = creator
                .create(from_hex(payload)?)
                .and_then(|owid| owid.as_byte_array())
                .map_err(|error| anyhow!("{}: signing: {error}", case.id))?;
            if case.tamper {
                let last = bytes
                    .last_mut()
                    .context("an envelope ends with its signature")?;
                *last ^= 1;
            }
            if let Some(trailing) = &case.trailing {
                bytes.extend(from_hex(trailing)?);
            }
            let (form, input) = encode(&bytes, case.encoding.unwrap_or(Encoding::Standard));
            (form, Some(input), Some(public_key.to_owned()), case.tamper)
        }
        (None, Some(text), false) => {
            payload_only(case)?;
            (Form::Text, Some(text.clone()), None, false)
        }
        (None, None, true) => {
            payload_only(case)?;
            (Form::Metadata, None, None, false)
        }
        _ => bail!(
            "{}: a case has exactly one of payload, text and metadata",
            case.id
        ),
    };
    Ok(Signed {
        id,
        about,
        form,
        input,
        public_key,
        tamper,
    })
}

/// Fails when a case that is not a payload has a payload's options.
fn payload_only(case: &Case) -> Result<()> {
    ensure!(
        case.encoding.is_none() && case.trailing.is_none() && !case.tamper,
        "{}: encoding, trailing and tamper apply to a payload only",
        case.id
    );
    Ok(())
}

fn encode(bytes: &[u8], encoding: Encoding) -> (Form, String) {
    match encoding {
        Encoding::Standard => (Form::Text, STANDARD.encode(bytes)),
        Encoding::Url => (Form::Text, URL_SAFE_NO_PAD.encode(bytes)),
        Encoding::UrlSurrounded => (Form::Text, format!(" {}\n", URL_SAFE_NO_PAD.encode(bytes))),
        Encoding::UrlSpaced => (Form::Text, spaced(&URL_SAFE_NO_PAD.encode(bytes))),
        Encoding::Bytes => (Form::Bytes, hex(bytes)),
    }
}

/// `text` with a space between each group of [`SPACED_GROUP_LENGTH`]
/// characters.
fn spaced(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + text.len() / SPACED_GROUP_LENGTH);
    for (index, character) in text.chars().enumerate() {
        if index > 0 && index.is_multiple_of(SPACED_GROUP_LENGTH) {
            out.push(' ');
        }
        out.push(character);
    }
    out
}

/// `fodid`: reads every case of each `--corpus` list and writes a line for
/// each to `--out`.
pub fn read(args: &[String]) -> Result<()> {
    let mut flags = flag_values(args, &["--corpus", "--out"])?;
    let corpora = flags.remove("--corpus").context("--corpus is required")?;
    let out = single(&mut flags, "--out")?;
    let mut entries: Vec<Signed> = Vec::new();
    for path in &corpora {
        entries.extend(read_json::<Vec<Signed>>(Path::new(path))?);
    }
    if let Some(id) = repeated(entries.iter().map(|entry| entry.id.as_str())) {
        bail!("case {id} is given more than once");
    }

    let mut writer = BufWriter::new(File::create(&out).with_context(|| format!("creating {out}"))?);
    for entry in &entries {
        serde_json::to_writer(&mut writer, &line(entry)?)?;
        writer.write_all(b"\n")?;
    }
    writer.flush()?;
    eprintln!("{} 51Did cases written to {out}", entries.len());
    Ok(())
}

fn line(entry: &Signed) -> Result<Value> {
    let input = || {
        entry
            .input
            .as_deref()
            .with_context(|| format!("{}: no input", entry.id))
    };
    let result = match entry.form {
        Form::Metadata => return Ok(json!({"id": entry.id, "metadata": metadata()})),
        Form::Text => FodId::from_base64(input()?),
        Form::Bytes => FodId::from_byte_array(&from_hex(input()?)?),
    };
    Ok(json!({
        "id": entry.id,
        "fodid": fodid_fields(result, entry.public_key.as_deref()),
    }))
}

/// The parse status, then every accessor's value, each null when the
/// 51Did was not read. The signature status is null for a case with no key.
fn fodid_fields(result: std::result::Result<FodId, Error>, public_key: Option<&str>) -> Value {
    let (status, fodid) = match result {
        Ok(fodid) => (PARSED.to_owned(), Some(fodid)),
        Err(error) => (status_name(&error), None),
    };
    let accessors: [(&str, Accessor); 15] = [
        ("type", |f, _| json!(type_name(f.id_type()))),
        ("usage", |f, _| json!(usage_name(f.usage()))),
        ("idUsage", |f, _| json!(f.usage().id_usage())),
        ("usageIsIndirect", |f, _| json!(f.usage_is_indirect())),
        ("licenseId", |f, _| json!(f.license_id())),
        ("matchKey", |f, _| json!(hex(f.match_key()))),
        ("terms", |f, _| json!(f.terms())),
        ("base64Url", |f, _| text_or_error(f.as_base64_url())),
        ("version", |f, _| json!(f.version().as_byte())),
        ("domain", |f, _| json!(f.domain())),
        ("date", |f, _| {
            json!(f.date().format(DATE_FORMAT).to_string())
        }),
        ("payload", |f, _| json!(hex(f.payload()))),
        ("signature", |f, _| json!(hex(f.signature()))),
        ("base64", |f, _| text_or_error(f.as_base64())),
        ("signatureStatus", |f, key| {
            key.map_or(Value::Null, |key| {
                json!(f.verify_status_with_public_key(key).as_str())
            })
        }),
    ];
    let mut fields = Map::new();
    fields.insert("status".to_owned(), Value::String(status));
    for (name, accessor) in accessors {
        let value = fodid
            .as_ref()
            .map_or(Value::Null, |fodid| accessor(fodid, public_key));
        fields.insert(name.to_owned(), value);
    }
    Value::Object(fields)
}

/// The .NET `FodIdParseStatus` name for a read that failed. The OWID
/// statuses already carry the shared names. Anything .NET has no name for
/// is written with a `rust:` prefix, so it shows as a difference.
fn status_name(error: &Error) -> String {
    match error {
        Error::Parse(parse) => parse.status().as_str().to_owned(),
        Error::PayloadTooShort { .. } => "PayloadTooShort".to_owned(),
        Error::InvalidTypePayloadLength { .. } => "InvalidTypePayloadLength".to_owned(),
        Error::UnsupportedPayloadVersion { .. } => "UnsupportedPayloadVersion".to_owned(),
        Error::NoUsage => "NoUsage".to_owned(),
        other => format!("rust:{other:?}"),
    }
}

/// The .NET `IdType` name.
fn type_name(id_type: IdType) -> &'static str {
    match id_type {
        IdType::Probabilistic => "Probabilistic",
        IdType::Random => "Random",
        IdType::HashedEmail => "HashedEmail",
        IdType::Reserved => "Reserved",
    }
}

/// The .NET `Usage` name.
fn usage_name(usage: Usage) -> &'static str {
    match usage {
        Usage::NonMarketing => "NonMarketing",
        Usage::Standard => "Standard",
        Usage::Personalized => "Personalized",
    }
}

fn text_or_error<E: std::fmt::Display>(result: std::result::Result<String, E>) -> Value {
    match result {
        Ok(text) => json!(text),
        Err(error) => json!(format!("rust:{error}")),
    }
}

/// The 51Did component and its properties in list order.
fn metadata() -> Value {
    let component = default_component_metadata();
    let properties: Vec<Value> = component
        .properties()
        .iter()
        .map(property_metadata)
        .collect();
    json!({
        "name": component.name,
        "componentId": component.component_id,
        "properties": properties,
    })
}

fn property_metadata(property: &FiftyOneAspectPropertyMetaData) -> Value {
    let aspect = property.aspect();
    let core = property.core();
    json!({
        "name": property.name(),
        "category": core.category,
        "component": property.component_name(),
        "componentId": property.component_id(),
        "type": value_type_name(core.value_type),
        "defaultValue": property.default_value().map(|value| value.name.as_str()),
        "available": aspect.available(),
        "dataTiers": aspect.data_tiers_where_present(),
        "description": aspect.description(),
        "displayOrder": property.display_order(),
        "mandatory": property.mandatory(),
        "list": property.list(),
        "obsolete": property.obsolete(),
        "show": property.show(),
        "showValues": property.show_values(),
        "url": property.url(),
        "valueCount": property.values().len(),
        "delayExecution": core.delay_execution,
        "evidenceProperties": core.evidence_properties,
    })
}

/// The type a property's values have, by the names the other lines use.
fn value_type_name(value_type: PropertyValueType) -> String {
    let name = match value_type {
        PropertyValueType::String => "string",
        PropertyValueType::Bool => "bool",
        PropertyValueType::Integer => "integer",
        PropertyValueType::Double => "double",
        PropertyValueType::Float => "float",
        PropertyValueType::StringList => "stringList",
        PropertyValueType::JavaScript => "javascript",
        PropertyValueType::KeyValueList => "keyValueList",
        other => return format!("rust:{other:?}"),
    };
    name.to_owned()
}

/// Lower case hex.
fn hex(bytes: &[u8]) -> String {
    let mut text = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        // Writing to a String cannot fail.
        let _ = write!(text, "{byte:02x}");
    }
    text
}

/// The bytes written as hex in `text`, ignoring white space.
fn from_hex(text: &str) -> Result<Vec<u8>> {
    let digits = text
        .chars()
        .filter(|character| !character.is_whitespace())
        .map(|character| {
            character
                .to_digit(16)
                .with_context(|| format!("{character:?} in {text:?} is not a hex digit"))
        })
        .collect::<Result<Vec<u32>>>()?;
    ensure!(
        digits.len().is_multiple_of(2),
        "{text:?} has an odd number of hex digits"
    );
    Ok(digits
        .chunks(2)
        .map(|pair| (pair[0] * 16 + pair[1]) as u8)
        .collect())
}

/// The values given for each flag. Every flag takes a value, and only the
/// flags in `allowed` are accepted.
fn flag_values(args: &[String], allowed: &[&str]) -> Result<HashMap<String, Vec<String>>> {
    let mut values: HashMap<String, Vec<String>> = HashMap::new();
    let mut args = args.iter();
    while let Some(flag) = args.next() {
        ensure!(allowed.contains(&flag.as_str()), "unknown argument {flag}");
        let value = args
            .next()
            .with_context(|| format!("{flag} needs a value"))?;
        values.entry(flag.clone()).or_default().push(value.clone());
    }
    Ok(values)
}

/// The one value given for `flag`.
fn single(values: &mut HashMap<String, Vec<String>>, flag: &str) -> Result<String> {
    match values.remove(flag).as_deref() {
        Some([value]) => Ok(value.clone()),
        Some(_) => bail!("{flag} may be given once"),
        None => bail!("{flag} is required"),
    }
}

/// The first id given twice, if any.
fn repeated<'a>(mut ids: impl Iterator<Item = &'a str>) -> Option<&'a str> {
    let mut seen = HashSet::new();
    ids.find(|id| !seen.insert(*id))
}

fn read_json<T: DeserializeOwned>(path: &Path) -> Result<T> {
    let text =
        std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    serde_json::from_str(&text).with_context(|| format!("parsing {}", path.display()))
}
