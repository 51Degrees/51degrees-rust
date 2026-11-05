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

//! Usage sharing records, written byte for byte as the
//! [usage sharing element specification](https://github.com/51Degrees/specifications/blob/ae77ca40c270e04454ae5424b5131c0e105359a7/pipeline-specification/pipeline-elements/usage-sharing-element.md#processing)
//! sets them out, so the same evidence gives the same record in every
//! language.

use std::borrow::Cow;
use std::fmt::{self, Write as _};

use crate::constants::SHARE_USAGE_MAX_EVIDENCE_LENGTH;
use crate::usage_sink::UsageSink;

/// The byte order mark and XML declaration that start every document.
const DOCUMENT_START: &str = "\u{FEFF}<?xml version=\"1.0\" encoding=\"utf-8\"?>";

/// The text put in front of a value cut to the maximum length.
const TRUNCATED_PREFIX: &str = "[TRUNCATED BY USAGE SHARING] ";

/// The product named in every record.
const PRODUCT: &str = "Pipeline";

/// The language named in every record.
const LANGUAGE: &str = "rust";

/// The end of a CDATA section.
const CDATA_END: &str = "]]>";

/// What `]]>` inside a value becomes. It closes the section after `]]` and
/// opens another before `>`.
const CDATA_END_REPLACEMENT: &str = "]]]]><![CDATA[>";

/// The values that are the same in every record an element writes.
#[derive(Debug, Default)]
pub(crate) struct StaticInfo {
    pub(crate) core_version: String,
    pub(crate) language_version: String,
    pub(crate) server_ip: String,
    pub(crate) platform: String,
}

/// One shared request's values, copied out of its flow data so the request
/// can finish before the record is written.
///
/// A [`UsageSink`] reads the records of each batch through
/// [`UsageBatch::records`](crate::UsageBatch::records).
#[derive(Debug, Clone)]
pub struct UsageRecord {
    pub(crate) session_id: String,
    pub(crate) sequence: i32,
    pub(crate) client_ip: String,
    flow_elements: Vec<&'static str>,
    /// Grouped by prefix in the order each prefix first appears.
    groups: Vec<EvidenceGroup>,
}

/// The shared evidence entries with one prefix, in the order the evidence
/// holds them.
#[derive(Debug, Clone)]
struct EvidenceGroup {
    /// The prefix as the key spells it. Empty for keys written whole.
    prefix: String,
    /// Each entry's field, or its whole key when there is no prefix, and its
    /// value.
    entries: Vec<(String, String)>,
}

impl UsageRecord {
    /// A record for a request made by a pipeline of these elements, holding
    /// no evidence yet.
    pub(crate) fn new(flow_elements: Vec<&'static str>) -> Self {
        UsageRecord {
            session_id: String::new(),
            sequence: 0,
            client_ip: String::new(),
            flow_elements,
            groups: Vec::new(),
        }
    }

    /// The session id, or an empty string when the request has none.
    pub fn session_id(&self) -> &str {
        &self.session_id
    }

    /// The sequence number, or 0 when the request has none or it is not a
    /// 32-bit integer.
    pub fn sequence(&self) -> i32 {
        self.sequence
    }

    /// The client IP address, or an empty string when the request has none.
    pub fn client_ip(&self) -> &str {
        &self.client_ip
    }

    /// The type names of the elements in the pipeline that made the request,
    /// in the pipeline's order.
    pub fn flow_elements(&self) -> &[&'static str] {
        &self.flow_elements
    }

    /// The shared evidence as `(prefix, name, value)`, in the order the
    /// record writes it, which groups entries by prefix. The prefix and name
    /// are spelled as the evidence key has them, and the prefix is empty for
    /// a key with no prefix, whose name is then the whole key. A value over
    /// the maximum length is already cut. Characters XML cannot carry are
    /// replaced only when the record is written, and an entry whose element
    /// name cannot be an XML name is left out of the written record.
    pub fn evidence(&self) -> impl Iterator<Item = (&str, &str, &str)> + '_ {
        self.groups.iter().flat_map(|group| {
            group
                .entries
                .iter()
                .map(move |(name, value)| (group.prefix.as_str(), name.as_str(), value.as_str()))
        })
    }

    /// Add a shared evidence entry. The key, spelled as given, is split at its
    /// first `.` into a prefix and a field. A key with no `.`, or that starts
    /// with one, has no prefix. A value over the maximum length is cut here.
    pub(crate) fn add_evidence(&mut self, key: &str, value: &str) {
        let (prefix, field) = match key.find('.') {
            Some(index) if index > 0 => (&key[..index], &key[index + 1..]),
            _ => ("", key),
        };
        let entry = (field.to_owned(), truncate(value).into_owned());
        match self.groups.iter_mut().find(|group| group.prefix == prefix) {
            Some(group) => group.entries.push(entry),
            None => self.groups.push(EvidenceGroup {
                prefix: prefix.to_owned(),
                entries: vec![entry],
            }),
        }
    }
}

/// Read a sequence number from evidence text as .NET's `int.TryParse` reads
/// it, being white space, an optional sign, the digits 0 to 9, white space,
/// then any number of NUL characters. White space here is space and U+0009
/// to U+000D. `None` when the text is not a 32-bit integer.
pub(crate) fn parse_sequence(text: &str) -> Option<i32> {
    text.trim_end_matches('\0')
        .trim_matches(is_number_white_space)
        .parse()
        .ok()
}

/// The white space .NET allows around a number.
fn is_number_white_space(c: char) -> bool {
    matches!(c, ' ' | '\t'..='\r')
}

/// Write a batch of records as one `<Devices>` document, with the elements
/// the sink adds to each record. `date_sent` gives each record's `DateSent`
/// as the record is written.
pub(crate) fn write_document(
    records: &[UsageRecord],
    info: &StaticInfo,
    sink: &dyn UsageSink,
    mut date_sent: impl FnMut() -> String,
) -> Result<String, UsageWriteError> {
    let mut out = String::from(DOCUMENT_START);
    out.push_str("<Devices>");
    for record in records {
        write_record(&mut out, record, info, sink, &date_sent())?;
    }
    out.push_str("</Devices>");
    Ok(out)
}

/// Write one `<Device>` element.
fn write_record(
    out: &mut String,
    record: &UsageRecord,
    info: &StaticInfo,
    sink: &dyn UsageSink,
    date_sent: &str,
) -> Result<(), UsageWriteError> {
    out.push_str("<Device>");
    sink.write_before_record(&mut UsageWriter::new(&mut *out, &info.server_ip), record)?;
    let mut writer = RecordWriter {
        out,
        replaced: false,
    };
    writer.element("SessionId", &record.session_id);
    writer.element("Sequence", &record.sequence.to_string());
    writer.element("DateSent", date_sent);
    writer.element("ClientIP", &record.client_ip);
    writer.element("Version", &info.core_version);
    sink.write_pipeline_info(
        &mut UsageWriter::new(&mut *writer.out, &info.server_ip),
        &record.flow_elements,
    )?;
    writer.element("Language", LANGUAGE);
    writer.element("LanguageVersion", &info.language_version);
    writer.element("ServerIP", &info.server_ip);
    writer.element("Platform", &info.platform);
    for group in &record.groups {
        for (field, value) in &group.entries {
            writer.evidence(&group.prefix, field, value);
        }
    }
    if writer.replaced {
        writer.element("BadSchema", "true");
    }
    sink.write_after_record(
        &mut UsageWriter::new(&mut *writer.out, &info.server_ip),
        record,
    )?;
    writer.out.push_str("</Device>");
    Ok(())
}

/// Write the product and one `FlowElement` for each name, as every record
/// has them unless a sink writes its own.
pub(crate) fn write_default_pipeline_info(
    writer: &mut UsageWriter<'_>,
    flow_elements: &[&str],
) -> Result<(), UsageWriteError> {
    writer.element("Product", PRODUCT)?;
    for name in flow_elements {
        writer.element("FlowElement", name)?;
    }
    Ok(())
}

/// Writes elements into a usage record for a [`UsageSink`]'s
/// `write_before_record`, `write_pipeline_info` and `write_after_record`.
pub struct UsageWriter<'a> {
    out: &'a mut String,
    server_ip: &'a str,
}

impl<'a> UsageWriter<'a> {
    fn new(out: &'a mut String, server_ip: &'a str) -> Self {
        UsageWriter { out, server_ip }
    }

    /// Write `<name>text</name>`, or `<name />` when the text is empty. The
    /// text is escaped, and each character XML cannot carry is replaced as in
    /// the record's own values. These elements are the sink's own, so a
    /// replacement in them does not add `BadSchema` to the record.
    ///
    /// # Errors
    ///
    /// [`UsageWriteError`] when `name` cannot be an XML element name with no
    /// namespace prefix, and nothing is written.
    pub fn element(&mut self, name: &str, text: &str) -> Result<(), UsageWriteError> {
        if !is_xml_name(name) {
            return Err(UsageWriteError {
                name: name.to_owned(),
            });
        }
        push_element(self.out, name, &replace_invalid_xml_chars(text));
        Ok(())
    }

    /// The address of this server, as each record gives it in `ServerIP`.
    pub fn server_ip(&self) -> &'a str {
        self.server_ip
    }
}

/// A [`UsageWriter`] was asked for an element whose name cannot be an XML
/// element name.
///
/// A batch whose sink gives this error is counted as failed and is not
/// passed to [`UsageSink::receive`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UsageWriteError {
    name: String,
}

impl UsageWriteError {
    /// The name that cannot be an XML element name.
    pub fn name(&self) -> &str {
        &self.name
    }
}

impl fmt::Display for UsageWriteError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "'{}' cannot be an XML element name", self.name)
    }
}

impl std::error::Error for UsageWriteError {}

/// Append `<name>text</name>`, or `<name />` when the text is empty, with the
/// text escaped.
fn push_element(out: &mut String, name: &str, text: &str) {
    out.push('<');
    out.push_str(name);
    if text.is_empty() {
        out.push_str(" />");
        return;
    }
    out.push('>');
    push_escaped(out, text, false);
    out.push_str("</");
    out.push_str(name);
    out.push('>');
}

/// Writes the elements of one record, noting whether any character in them
/// was replaced.
struct RecordWriter<'a> {
    out: &'a mut String,
    replaced: bool,
}

impl RecordWriter<'_> {
    /// Write `<name>text</name>`, or `<name />` when the text is empty.
    fn element(&mut self, name: &str, text: &str) {
        let text = self.replace(text);
        push_element(self.out, name, &text);
    }

    /// Write an evidence entry, as
    /// `<prefix Name="field"><![CDATA[value]]></prefix>`, or as
    /// `<key>value</key>` when the key has no prefix. An entry whose element
    /// name cannot be an XML name is left out, because writing it would make
    /// the whole document unreadable.
    fn evidence(&mut self, prefix: &str, field: &str, value: &str) {
        let name = if prefix.is_empty() { field } else { prefix };
        if !is_xml_name(name) {
            let separator = if prefix.is_empty() { "" } else { "." };
            log::debug!(
                "Usage sharing left out the evidence '{prefix}{separator}{field}' \
                 because '{name}' cannot be an XML element name."
            );
            return;
        }
        if prefix.is_empty() {
            self.element(field, value);
            return;
        }
        let field = self.replace(field);
        let value = self.replace(value);
        self.out.push('<');
        self.out.push_str(prefix);
        self.out.push_str(" Name=\"");
        push_escaped(self.out, &field, true);
        self.out.push_str("\"><![CDATA[");
        if value.contains(CDATA_END) {
            self.out
                .push_str(&value.replace(CDATA_END, CDATA_END_REPLACEMENT));
        } else {
            self.out.push_str(&value);
        }
        self.out.push_str(CDATA_END);
        self.out.push_str("</");
        self.out.push_str(prefix);
        self.out.push('>');
    }

    /// `text` with each character XML cannot carry replaced, noting the
    /// replacement for the record's `BadSchema` element.
    fn replace<'t>(&mut self, text: &'t str) -> Cow<'t, str> {
        let text = replace_invalid_xml_chars(text);
        if matches!(text, Cow::Owned(_)) {
            self.replaced = true;
        }
        text
    }
}

/// Append `text`, writing `&`, `<` and `>` as entities, and `"` as well in an
/// attribute value. Every other character, tab, line feed and carriage return
/// included, is written as it is.
fn push_escaped(out: &mut String, text: &str, attribute: bool) {
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' if attribute => out.push_str("&quot;"),
            c => out.push(c),
        }
    }
}

/// Cut a value longer than the maximum, counted in UTF-16 code units, to its
/// first code units after [`TRUNCATED_PREFIX`]. The cut falls before any
/// character that would take it past the maximum.
fn truncate(value: &str) -> Cow<'_, str> {
    // A string is never shorter in UTF-8 bytes than in UTF-16 code units, so
    // a value within the maximum in bytes is within it in code units.
    if value.len() <= SHARE_USAGE_MAX_EVIDENCE_LENGTH {
        return Cow::Borrowed(value);
    }
    let mut units = 0;
    for (index, c) in value.char_indices() {
        units += c.len_utf16();
        if units > SHARE_USAGE_MAX_EVIDENCE_LENGTH {
            return Cow::Owned(format!("{TRUNCATED_PREFIX}{}", &value[..index]));
        }
    }
    Cow::Borrowed(value)
}

/// True for a character an XML 1.0 document can carry.
fn is_xml_char(c: char) -> bool {
    matches!(
        c,
        '\t' | '\n'
            | '\r'
            | '\u{20}'..='\u{D7FF}'
            | '\u{E000}'..='\u{FFFD}'
            | '\u{10000}'..='\u{10FFFF}'
    )
}

/// Replace each character XML cannot carry with `\x` and its code as four
/// lowercase hexadecimal digits. Borrowed when nothing was replaced.
fn replace_invalid_xml_chars(text: &str) -> Cow<'_, str> {
    if text.chars().all(is_xml_char) {
        return Cow::Borrowed(text);
    }
    let mut replaced = String::with_capacity(text.len() + 8);
    for c in text.chars() {
        if is_xml_char(c) {
            replaced.push(c);
        } else {
            let _ = write!(replaced, "\\x{:04x}", u32::from(c));
        }
    }
    Cow::Owned(replaced)
}

/// True if `name` can be an XML element name with no namespace prefix, by
/// the [name rules of XML 1.0](https://www.w3.org/TR/xml/#NT-Name) less the
/// colon.
fn is_xml_name(name: &str) -> bool {
    let mut chars = name.chars();
    chars.next().is_some_and(is_name_start_char) && chars.all(is_name_char)
}

/// XML 1.0's `NameStartChar`, less the colon.
fn is_name_start_char(c: char) -> bool {
    matches!(
        c,
        'A'..='Z'
            | '_'
            | 'a'..='z'
            | '\u{C0}'..='\u{D6}'
            | '\u{D8}'..='\u{F6}'
            | '\u{F8}'..='\u{2FF}'
            | '\u{370}'..='\u{37D}'
            | '\u{37F}'..='\u{1FFF}'
            | '\u{200C}'..='\u{200D}'
            | '\u{2070}'..='\u{218F}'
            | '\u{2C00}'..='\u{2FEF}'
            | '\u{3001}'..='\u{D7FF}'
            | '\u{F900}'..='\u{FDCF}'
            | '\u{FDF0}'..='\u{FFFD}'
            | '\u{10000}'..='\u{EFFFF}'
    )
}

/// XML 1.0's `NameChar`, less the colon.
fn is_name_char(c: char) -> bool {
    is_name_start_char(c)
        || matches!(
            c,
            '-' | '.' | '0'..='9' | '\u{B7}' | '\u{300}'..='\u{36F}' | '\u{203F}'..='\u{2040}'
        )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::usage_sink::UsageBatch;

    /// The result of a sink's `receive`.
    type Received = Result<(), Box<dyn std::error::Error + Send + Sync>>;

    /// A sink that adds nothing to a record.
    struct Plain;

    impl UsageSink for Plain {
        fn receive(&self, _batch: UsageBatch) -> Received {
            Ok(())
        }
    }

    /// A sink that adds two elements before each record and one after it,
    /// and names its own product and the elements by their last segment.
    struct Around;

    impl UsageSink for Around {
        fn receive(&self, _batch: UsageBatch) -> Received {
            Ok(())
        }

        fn write_before_record(
            &self,
            writer: &mut UsageWriter<'_>,
            record: &UsageRecord,
        ) -> Result<(), UsageWriteError> {
            writer.element("Received", "2026-11-02T14:05:10")?;
            writer.element("Entries", &record.evidence().count().to_string())
        }

        fn write_pipeline_info(
            &self,
            writer: &mut UsageWriter<'_>,
            flow_elements: &[&str],
        ) -> Result<(), UsageWriteError> {
            writer.element("Product", "Pipeline.Custom")?;
            for &name in flow_elements {
                writer.element("FlowElement", name.rsplit("::").next().unwrap_or(name))?;
            }
            Ok(())
        }

        fn write_after_record(
            &self,
            writer: &mut UsageWriter<'_>,
            _record: &UsageRecord,
        ) -> Result<(), UsageWriteError> {
            let server_ip = writer.server_ip();
            writer.element("Host", server_ip)
        }
    }

    /// A sink that writes one element before each record.
    struct Before(&'static str, &'static str);

    impl UsageSink for Before {
        fn receive(&self, _batch: UsageBatch) -> Received {
            Ok(())
        }

        fn write_before_record(
            &self,
            writer: &mut UsageWriter<'_>,
            _record: &UsageRecord,
        ) -> Result<(), UsageWriteError> {
            writer.element(self.0, self.1)
        }
    }

    /// The time every test record is written.
    const DATE_SENT: &str = "2026-11-02T14:05:09";

    /// The values from `Version` to `Platform` that [`info`] gives.
    const STATIC_XML: &str = "<Version>4.5.2</Version><Product>Pipeline</Product>\
                              <FlowElement>example::Element</FlowElement>\
                              <Language>rust</Language>\
                              <LanguageVersion>1.94.0</LanguageVersion>\
                              <ServerIP>192.0.2.10</ServerIP>\
                              <Platform>linux 6.8.0</Platform>";

    fn info() -> StaticInfo {
        StaticInfo {
            core_version: "4.5.2".to_owned(),
            language_version: "1.94.0".to_owned(),
            server_ip: "192.0.2.10".to_owned(),
            platform: "linux 6.8.0".to_owned(),
        }
    }

    /// A record with no session id or client IP and the evidence given.
    fn record(evidence: &[(&str, &str)]) -> UsageRecord {
        let mut record = UsageRecord::new(vec!["example::Element"]);
        for (key, value) in evidence {
            record.add_evidence(key, value);
        }
        record
    }

    fn written(record: &UsageRecord) -> String {
        written_with(record, &Plain)
    }

    fn written_with(record: &UsageRecord, sink: &dyn UsageSink) -> String {
        let mut out = String::new();
        write_record(&mut out, record, &info(), sink, DATE_SENT).unwrap();
        out
    }

    /// What follows `Platform` in a record.
    fn entries(xml: &str) -> &str {
        let end = "</Platform>";
        &xml[xml.find(end).unwrap() + end.len()..]
    }

    #[test]
    fn the_specifications_example_is_written_byte_for_byte() {
        let xml = written(&record(&[
            ("header.User-Agent", "Mozilla/5.0"),
            ("header.X-Test", "a]]>b"),
            ("query.51D_Test", "x\u{1}y"),
        ]));
        let expected = format!(
            "<Device><SessionId /><Sequence>0</Sequence>\
             <DateSent>{DATE_SENT}</DateSent><ClientIP />{STATIC_XML}\
             <header Name=\"User-Agent\"><![CDATA[Mozilla/5.0]]></header>\
             <header Name=\"X-Test\"><![CDATA[a]]]]><![CDATA[>b]]></header>\
             <query Name=\"51D_Test\"><![CDATA[x\\x0001y]]></query>\
             <BadSchema>true</BadSchema></Device>"
        );
        assert_eq!(xml, expected);
    }

    #[test]
    fn a_document_has_no_white_space_between_elements() {
        let records = [
            record(&[("header.User-Agent", "one")]),
            record(&[("header.User-Agent", "two")]),
        ];
        let xml = write_document(&records, &info(), &Plain, || DATE_SENT.to_owned()).unwrap();
        assert!(
            xml.as_bytes().starts_with(
                b"\xEF\xBB\xBF<?xml version=\"1.0\" encoding=\"utf-8\"?>\
                  <Devices><Device><SessionId />"
            ),
            "{xml}"
        );
        assert!(xml.ends_with("</Device></Devices>"), "{xml}");
        assert_eq!(xml.matches("<Device>").count(), 2, "{xml}");
        assert!(!xml.contains('\n') && !xml.contains("> <"), "{xml}");
    }

    #[test]
    fn entries_are_grouped_by_prefix_in_the_order_given() {
        let xml = written(&record(&[
            ("query.51D_b", "1"),
            ("header.Accept", "2"),
            ("query.51D_a", "3"),
            ("Header.Accept-Language", "4"),
            ("header.User-Agent", "5"),
        ]));
        assert_eq!(
            entries(&xml),
            "<query Name=\"51D_b\"><![CDATA[1]]></query>\
             <query Name=\"51D_a\"><![CDATA[3]]></query>\
             <header Name=\"Accept\"><![CDATA[2]]></header>\
             <header Name=\"User-Agent\"><![CDATA[5]]></header>\
             <Header Name=\"Accept-Language\"><![CDATA[4]]></Header></Device>"
        );
    }

    #[test]
    fn a_key_without_a_prefix_is_written_whole_as_text() {
        let xml = written(&record(&[("plain", "a & <b> \"c\""), ("empty", "")]));
        assert_eq!(
            entries(&xml),
            "<plain>a &amp; &lt;b&gt; \"c\"</plain><empty /></Device>"
        );
    }

    #[test]
    fn names_and_values_are_escaped_as_xml_needs() {
        let xml = written(&record(&[
            ("header.X-<&>\"'", "<&>\"'"),
            ("header.X-Space", "a\tb\nc\rd"),
            ("header.X-Empty", ""),
        ]));
        assert_eq!(
            entries(&xml),
            "<header Name=\"X-&lt;&amp;&gt;&quot;'\"><![CDATA[<&>\"']]></header>\
             <header Name=\"X-Space\"><![CDATA[a\tb\nc\rd]]></header>\
             <header Name=\"X-Empty\"><![CDATA[]]></header></Device>"
        );
    }

    #[test]
    fn control_characters_are_replaced_and_mark_the_record() {
        let xml = written(&record(&[("header.X\u{1f}Name", "a\u{0}b\u{8}c\u{1b}")]));
        assert_eq!(
            entries(&xml),
            "<header Name=\"X\\x001fName\">\
             <![CDATA[a\\x0000b\\x0008c\\x001b]]></header>\
             <BadSchema>true</BadSchema></Device>"
        );
    }

    #[test]
    fn a_value_over_the_maximum_in_utf16_code_units_is_cut_and_marked() {
        let max = SHARE_USAGE_MAX_EVIDENCE_LENGTH;
        // é is two bytes in UTF-8 and one UTF-16 code unit.
        assert_eq!(truncate(&"é".repeat(max)), "é".repeat(max));
        assert_eq!(
            truncate(&"é".repeat(max + 1)),
            format!("[TRUNCATED BY USAGE SHARING] {}", "é".repeat(max))
        );
        // U+1F600 is two UTF-16 code units.
        let face = "\u{1F600}";
        assert_eq!(truncate(&face.repeat(max / 2)), face.repeat(max / 2));
        assert_eq!(
            truncate(&face.repeat(max / 2 + 1)),
            format!("[TRUNCATED BY USAGE SHARING] {}", face.repeat(max / 2))
        );

        let long = "a".repeat(max + 1);
        let xml = written(&record(&[("header.X-Long", long.as_str())]));
        assert!(
            xml.contains(&format!(
                "<![CDATA[[TRUNCATED BY USAGE SHARING] {}]]>",
                "a".repeat(max)
            )),
            "{xml}"
        );
    }

    #[test]
    fn a_sequence_is_read_as_dotnet_reads_an_integer() {
        for (text, expected) in [
            ("3", Some(3)),
            (" \t+7\r\n", Some(7)),
            ("-2", Some(-2)),
            ("0005", Some(5)),
            ("2147483647", Some(i32::MAX)),
            ("-2147483648", Some(i32::MIN)),
            ("5\0\0", Some(5)),
            ("5 \0", Some(5)),
            ("2147483648", None),
            ("5\0 ", None),
            ("\u{a0}5", None),
            ("\u{663}", None),
            ("1.0", None),
            ("five", None),
            ("+", None),
            (" ", None),
            ("", None),
        ] {
            assert_eq!(parse_sequence(text), expected, "{text:?}");
        }
    }

    #[test]
    fn an_element_name_can_be_any_xml_name() {
        // U+00B7 MIDDLE DOT is an XML name character that is neither a
        // letter nor a digit.
        let xml = written(&record(&[
            ("café.Name", "kept"),
            ("_x-1.y", "kept too"),
            ("a\u{B7}b.z", "kept as well"),
        ]));
        assert_eq!(
            entries(&xml),
            "<café Name=\"Name\"><![CDATA[kept]]></café>\
             <_x-1 Name=\"y\"><![CDATA[kept too]]></_x-1>\
             <a\u{B7}b Name=\"z\"><![CDATA[kept as well]]></a\u{B7}b></Device>"
        );
        assert!(!'\u{B7}'.is_alphanumeric());
    }

    #[test]
    fn evidence_that_cannot_be_an_element_name_is_left_out() {
        let xml = written(&record(&[
            ("51d_value", "dropped"),
            ("plain", "kept"),
            ("my prefix.field", "dropped"),
        ]));
        assert_eq!(entries(&xml), "<plain>kept</plain></Device>");
    }

    #[test]
    fn a_record_gives_its_values_in_the_order_written() {
        let mut shared = record(&[
            ("query.51D_b", "1"),
            ("header.Accept", "2"),
            ("plain", "3"),
            ("query.51D_a", "4"),
        ]);
        shared.session_id = "session".to_owned();
        shared.sequence = 7;
        shared.client_ip = "192.0.2.7".to_owned();
        assert_eq!(shared.session_id(), "session");
        assert_eq!(shared.sequence(), 7);
        assert_eq!(shared.client_ip(), "192.0.2.7");
        assert_eq!(shared.flow_elements(), ["example::Element"]);
        assert_eq!(
            shared.evidence().collect::<Vec<_>>(),
            [
                ("query", "51D_b", "1"),
                ("query", "51D_a", "4"),
                ("header", "Accept", "2"),
                ("", "plain", "3"),
            ]
        );
    }

    #[test]
    fn a_sink_writes_around_each_record_and_names_the_pipeline() {
        let xml = written_with(
            &record(&[
                ("header.User-Agent", "Mozilla/5.0"),
                ("query.51D_Test", "x\u{1}y"),
            ]),
            &Around,
        );
        let expected = format!(
            "<Device><Received>2026-11-02T14:05:10</Received>\
             <Entries>2</Entries><SessionId /><Sequence>0</Sequence>\
             <DateSent>{DATE_SENT}</DateSent><ClientIP />\
             <Version>4.5.2</Version><Product>Pipeline.Custom</Product>\
             <FlowElement>Element</FlowElement><Language>rust</Language>\
             <LanguageVersion>1.94.0</LanguageVersion>\
             <ServerIP>192.0.2.10</ServerIP><Platform>linux 6.8.0</Platform>\
             <header Name=\"User-Agent\"><![CDATA[Mozilla/5.0]]></header>\
             <query Name=\"51D_Test\"><![CDATA[x\\x0001y]]></query>\
             <BadSchema>true</BadSchema><Host>192.0.2.10</Host></Device>"
        );
        assert_eq!(xml, expected);
    }

    #[test]
    fn a_sinks_text_is_escaped_and_replaced_without_marking_the_record() {
        let xml = written_with(
            &record(&[("header.User-Agent", "agent")]),
            &Before("Note", "a<b&c\u{1}"),
        );
        assert!(
            xml.starts_with("<Device><Note>a&lt;b&amp;c\\x0001</Note><SessionId />"),
            "{xml}"
        );
        assert!(!xml.contains("BadSchema"), "{xml}");
    }

    #[test]
    fn a_sink_element_that_cannot_be_named_stops_the_document() {
        for name in ["", "1st", "Two words", "ns:name", "a<b"] {
            let error = write_document(&[record(&[])], &info(), &Before(name, "x"), || {
                DATE_SENT.to_owned()
            })
            .unwrap_err();
            assert_eq!(error.name(), name);
            assert_eq!(
                error.to_string(),
                format!("'{name}' cannot be an XML element name")
            );
        }

        let mut out = String::new();
        let mut writer = UsageWriter::new(&mut out, "");
        assert!(writer.element("Two words", "x").is_err());
        writer.element("Kept", "").unwrap();
        assert_eq!(out, "<Kept />", "the refused element wrote nothing");
    }
}
