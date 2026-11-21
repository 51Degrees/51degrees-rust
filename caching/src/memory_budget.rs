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

//! The bytes a cache in memory may hold, from the memory the machine has.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::machine::{self, MemoryInfo};

/// The share of memory a budget gives, as a percentage, when no share is
/// given.
const DEFAULT_SHARE_PERCENT: u8 = 25;

/// How often a budget reads the machine's memory, when no period is given.
const DEFAULT_PERIOD: Duration = Duration::from_secs(1);

/// The bytes a budget gives when the machine's memory cannot be read.
const FALLBACK_BYTES: usize = 256 * 1024 * 1024;

/// Reads the machine's memory.
type Probe = Arc<dyn Fn() -> Option<MemoryInfo> + Send + Sync>;

/// The bytes a cache in memory may hold, taken from the memory the machine
/// has and read again as the cache is used, so the cache gives memory back
/// when the machine comes under pressure.
///
/// The budget is a share, a quarter by default, of the memory the cache
/// could use: the bytes it holds now plus the memory still available. As
/// other work takes memory, the available memory falls, the budget falls
/// with it and the cache evicts its least recently used entries. As memory
/// is freed the budget rises again. Where the system gives only the total
/// memory, the budget is half the share of the total.
///
/// Give one to [`LruStoreBuilder::memory_budget`](crate::LruStoreBuilder::memory_budget).
/// Native targets only.
#[derive(Clone)]
pub struct MemoryBudget {
    probe: Probe,
    share_percent: u8,
    period: Duration,
    last_read: Arc<Mutex<Option<Instant>>>,
}

impl MemoryBudget {
    /// A budget from the machine's memory, a quarter of the memory a cache
    /// could use, read at most once a second.
    pub fn from_machine() -> Self {
        Self::with_probe(machine::memory)
    }

    /// A budget from the memory `probe` reports, for a host that knows its
    /// own limits, such as a container's, or for a test.
    pub fn with_probe(probe: impl Fn() -> Option<MemoryInfo> + Send + Sync + 'static) -> Self {
        MemoryBudget {
            probe: Arc::new(probe),
            share_percent: DEFAULT_SHARE_PERCENT,
            period: DEFAULT_PERIOD,
            last_read: Arc::new(Mutex::new(None)),
        }
    }

    /// The share of the memory a cache could use that it may hold, from 1
    /// to 100 percent. Defaults to 25.
    pub fn share_percent(mut self, percent: u8) -> Self {
        self.share_percent = percent.clamp(1, 100);
        self
    }

    /// How often the machine's memory is read again. Defaults to a second.
    pub fn period(mut self, period: Duration) -> Self {
        self.period = period;
        self
    }

    /// The bytes a cache that now holds `held` bytes may hold.
    pub fn bytes_for(&self, held: usize) -> usize {
        let share = |bytes: u64| bytes / 100 * u64::from(self.share_percent);
        let bytes = match (self.probe)() {
            Some(MemoryInfo {
                available: Some(available),
                ..
            }) => share(available.saturating_add(held as u64)),
            Some(MemoryInfo { total, .. }) => share(total) / 2,
            None => FALLBACK_BYTES as u64,
        };
        usize::try_from(bytes).unwrap_or(usize::MAX).max(1)
    }

    /// The bytes a cache that now holds `held` bytes may hold, when the
    /// period has passed since the memory was last read, otherwise `None`.
    pub(crate) fn bytes_if_due(&self, held: usize) -> Option<usize> {
        {
            let mut last_read = self.last_read.lock().unwrap_or_else(|e| e.into_inner());
            let now = Instant::now();
            if last_read.is_some_and(|last| now.duration_since(last) < self.period) {
                return None;
            }
            *last_read = Some(now);
        }
        Some(self.bytes_for(held))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_budget_is_a_share_of_what_the_cache_could_use() {
        let budget = MemoryBudget::with_probe(|| {
            Some(MemoryInfo {
                total: 16_000,
                available: Some(3_000),
            })
        });
        assert_eq!(budget.bytes_for(1_000), 1_000);
        let budget = budget.share_percent(50);
        assert_eq!(budget.bytes_for(1_000), 2_000);
    }

    #[test]
    fn with_only_the_total_the_budget_is_half_the_share() {
        let budget = MemoryBudget::with_probe(|| {
            Some(MemoryInfo {
                total: 16_000,
                available: None,
            })
        });
        assert_eq!(budget.bytes_for(0), 2_000);
    }

    #[test]
    fn the_memory_is_read_again_only_after_the_period() {
        let budget = MemoryBudget::with_probe(|| {
            Some(MemoryInfo {
                total: 1_000,
                available: Some(400),
            })
        })
        .period(Duration::from_secs(3600));
        assert_eq!(budget.bytes_if_due(0), Some(100));
        assert_eq!(budget.bytes_if_due(0), None);
    }
}
