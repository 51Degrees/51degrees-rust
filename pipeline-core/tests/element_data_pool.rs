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

//! Element data reused through the pool, as the resource cleanup
//! specification's "Reusing Element Data" section recommends.

use std::alloc::{GlobalAlloc, Layout, System};
use std::any::Any;
use std::cell::Cell;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use fiftyone_pipeline_core::{
    ElementData, Evidence, EvidenceKeyFilter, EvidenceKeyFilterWhitelist, FlowData, FlowElement,
    MapElementData, NoValueError, Pipeline, PropertyMetaData, PropertyValue, Result, TypedKey,
};

/// Counts the allocations each thread makes, so tests running at the same
/// time do not count each other's.
struct Counting;

thread_local! {
    static ALLOCATIONS: Cell<usize> = const { Cell::new(0) };
}

// Safety: every call is passed to the system allocator unchanged.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let _ = ALLOCATIONS.try_with(|count| count.set(count.get() + 1));
        System.alloc(layout)
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        System.dealloc(ptr, layout)
    }
}

#[global_allocator]
static GLOBAL: Counting = Counting;

fn allocations() -> usize {
    ALLOCATIONS.with(Cell::get)
}

/// How many `Counted` instances were created, rather than taken from the
/// pool.
static CREATED: AtomicUsize = AtomicUsize::new(0);

/// Element data that counts how often it is created.
struct Counted(MapElementData);

impl Default for Counted {
    fn default() -> Self {
        CREATED.fetch_add(1, Ordering::SeqCst);
        Counted(MapElementData::new())
    }
}

impl ElementData for Counted {
    fn get(&self, name: &str) -> std::result::Result<PropertyValue, NoValueError> {
        self.0.get(name)
    }
    fn keys(&self) -> Vec<String> {
        self.0.keys()
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
    fn reset(&mut self) -> bool {
        self.0.reset()
    }
}

// WebAssembly has no threads to spawn, so it leaves out the test across
// threads and what only that test uses.

/// How many `Moved` instances were created, rather than taken from the
/// pool. Only the test across threads adds `Moved`, so other tests running
/// at the same time do not change the count.
#[cfg(not(target_family = "wasm"))]
static MOVED_CREATED: AtomicUsize = AtomicUsize::new(0);

/// Element data that counts how often it is created, for the test across
/// threads.
#[cfg(not(target_family = "wasm"))]
struct Moved(MapElementData);

#[cfg(not(target_family = "wasm"))]
impl Default for Moved {
    fn default() -> Self {
        MOVED_CREATED.fetch_add(1, Ordering::SeqCst);
        Moved(MapElementData::new())
    }
}

#[cfg(not(target_family = "wasm"))]
impl ElementData for Moved {
    fn get(&self, name: &str) -> std::result::Result<PropertyValue, NoValueError> {
        self.0.get(name)
    }
    fn keys(&self) -> Vec<String> {
        self.0.keys()
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
    fn reset(&mut self) -> bool {
        self.0.reset()
    }
}

/// How an element adds its data.
#[derive(Clone, Copy)]
enum Adds {
    /// A new `MapElementData` each time, with `get_or_add`.
    New,
    /// A `MapElementData` from the pool, with `get_or_add_pooled`.
    PooledMap,
    /// A `Counted` from the pool, with `get_or_add_pooled`.
    PooledCounted,
    /// A `Moved` from the pool, with `get_or_add_pooled`.
    #[cfg(not(target_family = "wasm"))]
    PooledMoved,
}

/// An element that sets the property the `query.name` evidence names, or
/// three fixed properties when there is none.
struct Element {
    adds: Adds,
    filter: EvidenceKeyFilterWhitelist,
}

const KEY: &str = "pooled";

fn names(data: &FlowData) -> Vec<String> {
    match data.evidence().get("query.name") {
        Some(name) => vec![name.to_owned()],
        None => vec!["one".into(), "two".into(), "three".into()],
    }
}

impl FlowElement for Element {
    fn process(&self, data: &mut FlowData) -> Result<()> {
        let names = names(data);
        match self.adds {
            Adds::New => {
                let map =
                    data.get_or_add(TypedKey::<MapElementData>::new(KEY), MapElementData::new)?;
                for name in names {
                    map.insert(name, true);
                }
            }
            Adds::PooledMap => {
                let map = data.get_or_add_pooled(TypedKey::<MapElementData>::new(KEY))?;
                for name in names {
                    map.insert(name, true);
                }
            }
            Adds::PooledCounted => {
                let counted = data.get_or_add_pooled(TypedKey::<Counted>::new(KEY))?;
                for name in names {
                    counted.0.insert(name, true);
                }
            }
            #[cfg(not(target_family = "wasm"))]
            Adds::PooledMoved => {
                let moved = data.get_or_add_pooled(TypedKey::<Moved>::new(KEY))?;
                for name in names {
                    moved.0.insert(name, true);
                }
            }
        }
        Ok(())
    }
    fn data_key(&self) -> &str {
        KEY
    }
    fn evidence_key_filter(&self) -> &dyn EvidenceKeyFilter {
        &self.filter
    }
    fn properties(&self) -> &[PropertyMetaData] {
        &[]
    }
}

fn pipeline(adds: Adds) -> Arc<Pipeline> {
    Pipeline::builder()
        .add_element(Arc::new(Element {
            adds,
            filter: EvidenceKeyFilterWhitelist::new(["query.name"]),
        }))
        .build()
        .unwrap()
}

fn processed(pipeline: &Arc<Pipeline>, name: Option<&str>) -> FlowData {
    let mut evidence = Evidence::builder();
    if let Some(name) = name {
        evidence = evidence.add("query.name", name);
    }
    let mut data = pipeline.create_flow_data_with(evidence.build());
    data.process().unwrap();
    data
}

/// The allocations this thread makes processing and dropping `requests`
/// flow data, after as many again to fill the pool.
fn allocations_for(pipeline: &Arc<Pipeline>, requests: usize) -> usize {
    for _ in 0..requests {
        drop(processed(pipeline, None));
    }
    let before = allocations();
    for _ in 0..requests {
        drop(processed(pipeline, None));
    }
    allocations() - before
}

/// Element data taken from the pool needs no new instance, map or list of
/// names, so each request makes at least three allocations fewer than one
/// that creates its data.
#[test]
fn pooled_element_data_allocates_less_for_each_request() {
    const REQUESTS: usize = 1000;
    let new = allocations_for(&pipeline(Adds::New), REQUESTS);
    let pooled = allocations_for(&pipeline(Adds::PooledMap), REQUESTS);
    assert!(
        new >= pooled + 3 * REQUESTS,
        "{new} allocations creating the data, {pooled} taking it from the pool"
    );
}

/// Data goes back to the pool only when its flow data is dropped, starts
/// with no values when it is taken again, and the pool keeps a bounded
/// number of each type.
#[test]
fn data_goes_back_to_the_pool_only_when_its_flow_data_is_dropped() {
    let pipeline = pipeline(Adds::PooledCounted);
    let created = || CREATED.load(Ordering::SeqCst);
    let start = created();

    let first = processed(&pipeline, Some("first"));
    let second = processed(&pipeline, Some("second"));
    assert_eq!(
        created(),
        start + 2,
        "a flow data still held keeps its data"
    );
    drop(first);
    drop(second);

    let third = processed(&pipeline, Some("third"));
    assert_eq!(
        created(),
        start + 2,
        "the data of a dropped flow data is reused"
    );
    assert_eq!(third.get_str(KEY).unwrap().keys(), ["third"]);
    drop(third);

    // More flow data held at once than the pool keeps of one type.
    const HELD: usize = 100;
    let held: Vec<FlowData> = (0..HELD).map(|_| processed(&pipeline, None)).collect();
    let after_holding = created();
    assert_eq!(after_holding, start + HELD);
    drop(held);
    let held: Vec<FlowData> = (0..HELD).map(|_| processed(&pipeline, None)).collect();
    let kept = HELD - (created() - after_holding);
    assert!(kept > 0 && kept < HELD, "the pool kept {kept} of {HELD}");
    drop(held);
}

/// Data a flow data gives back when it is dropped on one thread is taken by
/// a request processed on another, as in a web server that processes on a
/// blocking thread and drops the flow data on an asynchronous one.
// WebAssembly has no threads to spawn.
#[cfg(not(target_family = "wasm"))]
#[test]
fn data_given_back_on_one_thread_is_taken_on_another() {
    let pipeline = pipeline(Adds::PooledMoved);
    let created = || MOVED_CREATED.load(Ordering::SeqCst);
    let on_a_new_thread = |name: &'static str| {
        let pipeline = Arc::clone(&pipeline);
        std::thread::spawn(move || processed(&pipeline, Some(name)))
            .join()
            .expect("the request is processed")
    };

    let first = on_a_new_thread("first");
    let start = created();
    std::thread::spawn(move || drop(first))
        .join()
        .expect("the flow data is dropped");
    let second = on_a_new_thread("second");
    assert_eq!(
        created(),
        start,
        "the data given back on another thread was not taken"
    );
    assert_eq!(second.get_str(KEY).unwrap().keys(), ["second"]);
}

/// Data whose `reset` is the default is never reused, and a property bag
/// clears itself.
#[test]
fn only_data_that_resets_is_reused() {
    struct Plain(MapElementData);
    impl ElementData for Plain {
        fn get(&self, name: &str) -> std::result::Result<PropertyValue, NoValueError> {
            self.0.get(name)
        }
        fn keys(&self) -> Vec<String> {
            self.0.keys()
        }
        fn as_any(&self) -> &dyn Any {
            self
        }
        fn as_any_mut(&mut self) -> &mut dyn Any {
            self
        }
    }
    let mut plain = Plain(MapElementData::new().set("kept", true));
    assert!(!plain.reset());
    assert_eq!(plain.keys(), ["kept"]);

    let mut map = MapElementData::new().set("cleared", true);
    assert!(map.reset());
    assert!(map.is_empty());
    assert!(map.keys().is_empty());
}
