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

//! Element data is written only by the element that provides it, or the
//! elements that share it, and only while they process, as the
//! thread-safety specification's Element Data section requires.

use std::any::Any;
use std::sync::Arc;

use fiftyone_pipeline_core::{
    ElementData, Error, EvidenceKeyFilter, EvidenceKeyFilterWhitelist, FlowData, FlowElement,
    MapElementData, NoValueError, Pipeline, PropertyMetaData, PropertyValue, Result, TypedKey,
};

/// What an element does when it processes.
type Action = Box<dyn Fn(&mut FlowData) -> Result<()> + Send + Sync>;

/// An element that runs `action` under its data key.
struct Element {
    key: &'static str,
    action: Action,
    filter: EvidenceKeyFilterWhitelist,
}

impl FlowElement for Element {
    fn process(&self, data: &mut FlowData) -> Result<()> {
        (self.action)(data)
    }
    fn data_key(&self) -> &str {
        self.key
    }
    fn evidence_key_filter(&self) -> &dyn EvidenceKeyFilter {
        &self.filter
    }
    fn properties(&self) -> &[PropertyMetaData] {
        &[]
    }
}

fn element(
    key: &'static str,
    action: impl Fn(&mut FlowData) -> Result<()> + Send + Sync + 'static,
) -> Arc<dyn FlowElement> {
    Arc::new(Element {
        key,
        action: Box::new(action),
        filter: EvidenceKeyFilterWhitelist::default(),
    })
}

/// Set `name` to `value` in the data under `key`, adding the data if absent.
fn write(data: &mut FlowData, key: &'static str, name: &str, value: &str) -> Result<()> {
    data.get_or_add(TypedKey::<MapElementData>::new(key), MapElementData::new)?
        .insert(name, value);
    Ok(())
}

/// An element that writes `value` into its own data.
fn writer(key: &'static str, value: &'static str) -> Arc<dyn FlowElement> {
    element(key, move |data| write(data, key, "value", value))
}

/// The message of a write to the data under `key` once it is read only.
fn read_only(key: &str) -> String {
    format!(
        "Element data under key '{key}' cannot be written, because the element \
         that writes it is not processing. Element data is read only once its \
         element has finished."
    )
}

fn message(error: &Error) -> String {
    match error {
        Error::PipelineConfiguration { message } => message.clone(),
        other => panic!("not a configuration error: {other}"),
    }
}

fn value(data: &FlowData, key: &str) -> Option<String> {
    data.get_str(key)?
        .get("value")
        .ok()?
        .as_str()
        .map(str::to_owned)
}

#[test]
fn a_write_after_processing_fails_and_changes_nothing() {
    let pipeline = Pipeline::builder()
        .add_element(writer("first", "1"))
        .add_element(writer("second", "2"))
        .build()
        .unwrap();
    let mut data = pipeline.create_flow_data();
    data.process().unwrap();

    for key in ["first", "second", "SECOND"] {
        let error = write(&mut data, key, "value", "changed").unwrap_err();
        assert_eq!(message(&error), read_only(key));
    }
    assert_eq!(value(&data, "first").as_deref(), Some("1"));
    assert_eq!(value(&data, "second").as_deref(), Some("2"));
}

#[test]
fn a_later_element_cannot_write_an_earlier_elements_data() {
    let pipeline = Pipeline::builder()
        .add_element(writer("first", "1"))
        .add_element(element("second", |data| {
            write(data, "first", "value", "changed")
        }))
        .suppress_process_exceptions(true)
        .build()
        .unwrap();
    let mut data = pipeline.create_flow_data();
    data.process().unwrap();

    let errors = data.errors();
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].element_data_key, "second");
    assert_eq!(message(&errors[0].source), read_only("first"));
    assert_eq!(value(&data, "first").as_deref(), Some("1"));
}

#[test]
fn an_element_writes_its_own_data_as_often_as_it_likes_while_it_processes() {
    let pipeline = Pipeline::builder()
        .add_element(element("own", |data| {
            write(data, "own", "value", "1")?;
            write(data, "own", "value", "2")?;
            write(data, "added", "value", "3")?;
            write(data, "added", "value", "4")
        }))
        .build()
        .unwrap();
    let mut data = pipeline.create_flow_data();
    data.process().unwrap();
    assert_eq!(value(&data, "own").as_deref(), Some("2"));
    assert_eq!(value(&data, "added").as_deref(), Some("4"));
    let error = write(&mut data, "added", "value", "5").unwrap_err();
    assert_eq!(message(&error), read_only("added"));
}

#[test]
fn an_element_sharing_a_key_writes_the_data_again_while_it_processes() {
    let pipeline = Pipeline::builder()
        .add_element(element("Shared", |data| {
            write(data, "shared", "first", "1")
        }))
        .add_element(element("shared", |data| {
            write(data, "SHARED", "second", "2")
        }))
        .build()
        .unwrap();
    let mut data = pipeline.create_flow_data();
    data.process().unwrap();
    let shared = data.get_str("shared").unwrap();
    assert_eq!(shared.get("first").unwrap().as_str(), Some("1"));
    assert_eq!(shared.get("second").unwrap().as_str(), Some("2"));
    let error = write(&mut data, "shared", "third", "3").unwrap_err();
    assert_eq!(message(&error), read_only("shared"));
}

#[test]
fn the_caller_writes_what_it_added_until_processing_starts() {
    let pipeline = Pipeline::builder()
        .add_element(writer("element", "1"))
        .build()
        .unwrap();
    let mut data = pipeline.create_flow_data();
    write(&mut data, "caller", "value", "1").unwrap();
    write(&mut data, "caller", "value", "2").unwrap();
    data.process().unwrap();
    assert_eq!(value(&data, "caller").as_deref(), Some("2"));
    let error = write(&mut data, "caller", "value", "3").unwrap_err();
    assert_eq!(message(&error), read_only("caller"));
}

#[test]
fn a_group_member_writes_only_while_it_processes() {
    let pipeline = Pipeline::builder()
        .add_element(writer("before", "0"))
        .add_parallel_elements([
            element("one", |data| {
                write(data, "one", "value", "1")?;
                write(data, "one", "value", "2")
            }),
            writer("two", "3"),
        ])
        .add_element(element("after", |data| {
            write(data, "one", "value", "changed")
        }))
        .suppress_process_exceptions(true)
        .build()
        .unwrap();
    let mut data = pipeline.create_flow_data();
    data.process().unwrap();

    assert_eq!(value(&data, "one").as_deref(), Some("2"));
    assert_eq!(value(&data, "two").as_deref(), Some("3"));
    let errors = data.errors();
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].element_data_key, "after");
    assert_eq!(message(&errors[0].source), read_only("one"));
    for key in ["before", "one", "two"] {
        let error = write(&mut data, key, "value", "changed").unwrap_err();
        assert_eq!(message(&error), read_only(key));
    }
}

#[test]
fn a_write_with_the_wrong_type_after_processing_still_fails_as_read_only() {
    let pipeline = Pipeline::builder()
        .add_element(writer("first", "1"))
        .build()
        .unwrap();
    let mut data = pipeline.create_flow_data();
    data.process().unwrap();
    let error = data
        .get_or_add(TypedKey::<Other>::new("first"), || Other)
        .err()
        .expect("the write fails");
    assert_eq!(message(&error), read_only("first"));
}

/// Element data of another type than the elements here add.
struct Other;

impl ElementData for Other {
    fn get(&self, name: &str) -> std::result::Result<PropertyValue, NoValueError> {
        Err(NoValueError::new(name))
    }
    fn keys(&self) -> Vec<String> {
        Vec::new()
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}
