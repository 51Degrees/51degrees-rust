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

//! The repeat-evidence tracker for usage sharing.
//!
//! A user browsing a site generates many requests with identical usage data.
//! That duplicate data is discarded as early as possible, per the
//! [session-tracking section](https://github.com/51Degrees/specifications/blob/main/pipeline-specification/pipeline-elements/usage-sharing-element.md#session-tracking).
//! The tracker remembers recently seen evidence in a bounded, sharded LRU
//! cache. A request whose evidence matches a recent entry is not shared. The
//! matching entry's time is refreshed, so the suppression window slides while
//! a session stays active. Once an entry is older than the configured interval
//! it is treated as new again. When the cache is full the least recently seen
//! entry is evicted.
//!
//! Session id and sequence are excluded from the dedup key (by the
//! [`crate::EvidenceKeyFilterShareUsageTracker`]) so they cannot make every
//! request look unique.
//!
//! The key is made from the values that would be shared, being the flow
//! data's evidence and, when a web integration gave it, the web request, as
//! the usage sharing element finds them. The element accepts no evidence, so
//! there is no accepted evidence to make it from.

use std::time::{Duration, Instant};

use fiftyone_caching::{Cache, CacheBuilder, LruCache, PutCache};
use fiftyone_pipeline_core::{DataKey, Evidence, EvidenceKeyFilter, FlowData};

use crate::share_usage_element::found_evidence;

/// Tracks recently shared evidence to suppress duplicate usage data.
///
/// `track` returns `true` when the supplied flow data should be shared and
/// `false` when it duplicates a recently shared request. The entries live in a
/// sharded LRU cache, so concurrent requests mostly take different locks and no
/// request ever scans the whole set.
pub struct ShareUsageTracker {
    filter: Box<dyn EvidenceKeyFilter>,
    interval: Duration,
    entries: LruCache<DataKey, Instant>,
}

impl ShareUsageTracker {
    /// Create a tracker.
    ///
    /// `interval` is the sliding-window length (the repeat-evidence interval).
    /// `max_size` bounds the number of tracked keys, beyond which the least
    /// recently seen key is evicted. `filter` selects which evidence forms the
    /// dedup key.
    pub fn new(interval: Duration, max_size: usize, filter: Box<dyn EvidenceKeyFilter>) -> Self {
        ShareUsageTracker {
            filter,
            interval,
            entries: CacheBuilder::new().size(max_size.max(1)).build(),
        }
    }

    /// Decide whether the given flow data should be shared.
    ///
    /// Returns `true` if the evidence has not been seen within the interval (and
    /// records it), or `false` if it matches a recent entry (refreshing that
    /// entry's time so the window slides). The evidence is the flow data's
    /// with the web request, when a web integration gave one.
    pub fn track(&self, data: &FlowData) -> bool {
        self.track_evidence(&found_evidence(data.evidence()))
    }

    /// [`ShareUsageTracker::track`] for the evidence the usage sharing
    /// element has already found, so it is not found twice.
    pub(crate) fn track_evidence(&self, evidence: &Evidence) -> bool {
        let key = evidence.generate_key(self.filter.as_ref());
        // Evidence that produces an empty key (nothing matched the filter) is
        // always shared; there is nothing meaningful to deduplicate on.
        if key.is_empty() {
            return true;
        }

        let now = Instant::now();
        let share = match self.entries.get(&key) {
            Some(last_seen) => now.duration_since(last_seen) >= self.interval,
            None => true,
        };
        self.entries.put(key, now);
        share
    }

    /// The number of tracked entries. Intended for tests and diagnostics.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// True if no entries are tracked.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The most entries the tracker holds before it evicts. The cache rounds
    /// the requested size up to a whole number of entries per shard, so this
    /// can be slightly above the size passed to [`ShareUsageTracker::new`].
    pub fn capacity(&self) -> usize {
        self.entries.capacity()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use fiftyone_pipeline_core::{
        Evidence, EvidenceKeyFilterWhitelist, FlowElement, Pipeline, PropertyMetaData, Result,
    };

    use super::*;

    /// An element that does nothing, so a pipeline can create flow data.
    struct Noop {
        filter: EvidenceKeyFilterWhitelist,
    }

    impl FlowElement for Noop {
        fn process(&self, _data: &mut FlowData) -> Result<()> {
            Ok(())
        }
        fn data_key(&self) -> &str {
            "noop"
        }
        fn evidence_key_filter(&self) -> &dyn EvidenceKeyFilter {
            &self.filter
        }
        fn properties(&self) -> &[PropertyMetaData] {
            &[]
        }
    }

    fn pipeline() -> Arc<Pipeline> {
        Pipeline::builder()
            .add_element(Arc::new(Noop {
                filter: EvidenceKeyFilterWhitelist::new(["header.user-agent"]),
            }))
            .build()
            .unwrap()
    }

    fn tracker(interval: Duration, max_size: usize) -> ShareUsageTracker {
        ShareUsageTracker::new(
            interval,
            max_size,
            Box::new(EvidenceKeyFilterWhitelist::new(["header.user-agent"])),
        )
    }

    fn flow_data(pipeline: &Arc<Pipeline>, agent: &str) -> FlowData {
        pipeline.create_flow_data_with(Evidence::builder().add("header.user-agent", agent).build())
    }

    #[test]
    fn a_repeat_within_the_interval_is_not_shared() {
        let pipeline = pipeline();
        let tracker = tracker(Duration::from_secs(60), 100);
        assert!(tracker.track(&flow_data(&pipeline, "a")));
        assert!(!tracker.track(&flow_data(&pipeline, "a")));
        assert!(tracker.track(&flow_data(&pipeline, "b")));
    }

    #[test]
    fn a_repeat_after_the_interval_is_shared_again() {
        let pipeline = pipeline();
        let tracker = tracker(Duration::ZERO, 100);
        assert!(tracker.track(&flow_data(&pipeline, "a")));
        assert!(tracker.track(&flow_data(&pipeline, "a")));
    }

    #[test]
    fn the_tracker_stays_within_its_capacity() {
        let pipeline = pipeline();
        let tracker = tracker(Duration::from_secs(60), 100);
        for i in 0..10_000 {
            assert!(tracker.track(&flow_data(&pipeline, &format!("agent-{i}"))));
        }
        // The cache rounds the size up to a whole number per shard, so the
        // capacity is at most one entry per shard over the size asked for.
        assert!(
            tracker.capacity() < 100 + fiftyone_caching::default_concurrency(),
            "capacity {} for a size of 100",
            tracker.capacity()
        );
        assert!(
            tracker.len() <= tracker.capacity(),
            "{} entries in a tracker of capacity {}",
            tracker.len(),
            tracker.capacity()
        );
    }
}
