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

//! usage-sharing-element.md, "Error handling": when the queue is full,
//! further data is discarded until there is room, and "A message MUST NOT be
//! logged for each entry discarded, as a busy server would flood its log."
//! Kept in its own test binary because a logger is installed once for the
//! whole process.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{channel, Receiver};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use fiftyone_pipeline_core::{Evidence, FlowElement, Pipeline};
use fiftyone_pipeline_engines_fiftyone::{
    ShareUsageConfig, ShareUsageElement, UsageBatch, UsageSink,
};

/// Log records at warning level or above, as "LEVEL message".
static LOGGED: Mutex<Vec<String>> = Mutex::new(Vec::new());

/// Keeps every record at warning level or above in [`LOGGED`].
struct TestLogger;

impl log::Log for TestLogger {
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        metadata.level() <= log::Level::Warn
    }

    fn log(&self, record: &log::Record) {
        if self.enabled(record.metadata()) {
            LOGGED
                .lock()
                .unwrap()
                .push(format!("{} {}", record.level(), record.args()));
        }
    }

    fn flush(&self) {}
}

static LOGGER: TestLogger = TestLogger;

/// How long a held batch waits to be released, so a failing test ends
/// rather than waiting for a release its panic skipped.
const RELEASE_LIMIT_SECONDS: u64 = 10;

/// How long the test waits for the first batch to reach the sink.
const SEND_DEADLINE_SECONDS: u64 = 10;

/// How often the test looks for the first batch.
const POLL_INTERVAL_MILLISECONDS: u64 = 10;

/// A sink that counts each batch and holds it until it is released, so the
/// background thread takes nothing more from the queue meanwhile.
struct Held {
    arrived: Arc<AtomicUsize>,
    release: Mutex<Receiver<()>>,
}

impl UsageSink for Held {
    fn receive(
        &self,
        _batch: UsageBatch,
    ) -> std::result::Result<(), Box<dyn std::error::Error + Send + Sync>> {
        self.arrived.fetch_add(1, Ordering::SeqCst);
        let _ = self
            .release
            .lock()
            .unwrap()
            .recv_timeout(Duration::from_secs(RELEASE_LIMIT_SECONDS));
        Ok(())
    }
}

/// With the first record held by the sink and a queue of one, the second
/// record fills the queue and the next three are discarded, which gives one
/// warning counting the first discard, not three.
#[test]
fn several_discarded_entries_give_one_warning() {
    let _ = log::set_logger(&LOGGER);
    log::set_max_level(log::LevelFilter::Warn);

    let arrived = Arc::new(AtomicUsize::new(0));
    let (release, held) = channel();
    let share = Arc::new(ShareUsageElement::with_sink(
        ShareUsageConfig::builder()
            .minimum_entries_per_message(1)
            .maximum_queue_size(1)
            .take_timeout(Duration::ZERO)
            .build(),
        Held {
            arrived: Arc::clone(&arrived),
            release: Mutex::new(held),
        },
    ));
    let pipeline = Pipeline::builder()
        .add_element(Arc::clone(&share) as Arc<dyn FlowElement>)
        .build()
        .unwrap();
    let process = |agent: &str| {
        let evidence = Evidence::builder().add("header.user-agent", agent).build();
        pipeline.create_flow_data_with(evidence).process().unwrap();
    };

    process("held");
    let deadline = Instant::now() + Duration::from_secs(SEND_DEADLINE_SECONDS);
    while arrived.load(Ordering::SeqCst) == 0 && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(POLL_INTERVAL_MILLISECONDS));
    }
    assert_eq!(
        arrived.load(Ordering::SeqCst),
        1,
        "the first batch was sent"
    );

    for n in 0..4 {
        process(&format!("queued-{n}"));
    }
    let records = std::mem::take(&mut *LOGGED.lock().unwrap());

    // Release the held batch and the queued one, so dropping the element
    // does not wait for the release limit.
    for _ in 0..2 {
        let _ = release.send(());
    }
    drop(pipeline);
    drop(share);

    assert_eq!(records.len(), 1, "{records:?}");
    assert!(
        records[0].starts_with("WARN Usage sharing discarded a record because its queue of 1"),
        "{records:?}"
    );
    assert!(
        records[0].ends_with("Records discarded since the last report: 1."),
        "{records:?}"
    );
}
