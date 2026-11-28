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

//! The transaction the engine issues and keeps for each page that asks for
//! Human Confidence, as
//! [The transaction](https://github.com/51Degrees/specifications/blob/main/pipeline-specification/features/human-confidence.md#the-transaction)
//! sets out.
//!
//! Every rule about how a page is given its transaction and how a request
//! names its page is in this module, so it can change in one place.
//!
//! - [`HumanConfidenceTransactions::issue`] gives a page a
//!   [`HumanConfidencePage`]: an id, and a transaction kept here.
//! - The page loads the 51Degrees client script from
//!   [`HumanConfidencePage::script_url`], which carries the transaction to
//!   the remote server, so the page's sealed result is bound to it.
//! - The page sends the id back beside its sealed result as the parameter
//!   `humanconfidencepage`, the evidence [`PAGE_EVIDENCE`]. The id is only a
//!   handle. The transaction is never taken from the request.
//! - The engine takes the kept transaction for that id when it redeems, so
//!   it is used once. A page with none kept is not redeemed.

use std::collections::VecDeque;
use std::sync::{Mutex, PoisonError};
use std::time::Duration;

// std::time::Instant panics on wasm32-unknown-unknown, which has no clock.
use web_time::Instant;

/// The evidence a request names its page in, being the id of the
/// [`HumanConfidencePage`] the server gave that page.
pub const PAGE_EVIDENCE: &str = "query.humanconfidencepage";

/// How long a transaction is kept for a page whose sealed result has not
/// come back.
const LIFETIME: Duration = Duration::from_secs(600);

/// The most pages a transaction is kept for at once. The oldest is let go
/// when another page needs room, and that page is then not redeemed.
const MOST_KEPT: usize = 10_000;

/// The number of random bytes in a page id and in a transaction.
const RANDOM_BYTES: usize = 18;

/// The id and transaction of one page, from
/// [`HumanConfidenceTransactions::issue`].
///
/// Its `Debug` leaves the transaction out, so no log of the page holds it.
#[derive(Clone)]
pub struct HumanConfidencePage {
    id: String,
    transaction: String,
}

impl HumanConfidencePage {
    /// The id the page sends back as the parameter `humanconfidencepage`,
    /// beside its sealed result.
    pub fn id(&self) -> &str {
        &self.id
    }

    /// The address of the 51Degrees client script for this page, under the
    /// cloud API base `endpoint`, which ends in `/api/v4/`. It carries the
    /// page's transaction, so the remote server binds the sealed result the
    /// page gets to it.
    pub fn script_url(&self, endpoint: &str, resource_key: &str) -> String {
        format!(
            "{}/{}.js?transaction={}",
            endpoint.trim_end_matches('/'),
            encode(resource_key),
            encode(&self.transaction)
        )
    }
}

impl std::fmt::Debug for HumanConfidencePage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HumanConfidencePage")
            .field("id", &self.id)
            .finish_non_exhaustive()
    }
}

/// Issues a new transaction for each page and keeps it on the server, so a
/// sealed result redeems only for the page it was made for. Each
/// transaction is used once. One not used within ten minutes, or pushed out
/// by 10,000 newer pages, is let go, and that page is then not redeemed.
///
/// The transactions are kept in this process, so every request for a page
/// has to reach the process that served it.
#[derive(Default)]
pub struct HumanConfidenceTransactions {
    kept: Mutex<VecDeque<Kept>>,
}

struct Kept {
    page: String,
    transaction: String,
    until: Instant,
}

impl HumanConfidenceTransactions {
    /// A new page id and transaction, the transaction kept here. `None`
    /// only where the platform gives no random bytes.
    pub fn issue(&self) -> Option<HumanConfidencePage> {
        let page = HumanConfidencePage {
            id: random()?,
            transaction: random()?,
        };
        let now = Instant::now();
        let mut kept = self.kept.lock().unwrap_or_else(PoisonError::into_inner);
        drop_old(&mut kept, now);
        while kept.len() >= MOST_KEPT {
            kept.pop_front();
        }
        kept.push_back(Kept {
            page: page.id.clone(),
            transaction: page.transaction.clone(),
            until: now + LIFETIME,
        });
        Some(page)
    }

    /// Take the transaction kept for the page with this id, so it is used
    /// once, or `None` where none is kept for it.
    pub(crate) fn take(&self, page_id: &str) -> Option<String> {
        if page_id.is_empty() {
            return None;
        }
        let mut kept = self.kept.lock().unwrap_or_else(PoisonError::into_inner);
        drop_old(&mut kept, Instant::now());
        let at = kept.iter().position(|entry| entry.page == page_id)?;
        kept.remove(at).map(|entry| entry.transaction)
    }
}

/// Let go of every transaction kept past its time. They are kept in the
/// order issued, so these are at the front.
fn drop_old(kept: &mut VecDeque<Kept>, now: Instant) {
    while kept.front().is_some_and(|entry| entry.until <= now) {
        kept.pop_front();
    }
}

/// Random bytes, written as base64url with no padding.
fn random() -> Option<String> {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut bytes = [0u8; RANDOM_BYTES];
    getrandom::fill(&mut bytes).ok()?;
    let mut text = String::with_capacity(RANDOM_BYTES * 4 / 3);
    for chunk in bytes.chunks(3) {
        let n = (u32::from(chunk[0]) << 16) | (u32::from(chunk[1]) << 8) | u32::from(chunk[2]);
        for shift in [18, 12, 6, 0] {
            text.push(char::from(ALPHABET[((n >> shift) & 63) as usize]));
        }
    }
    Some(text)
}

/// A value percent-encoded for a URL, leaving the unreserved characters.
fn encode(value: &str) -> String {
    value
        .bytes()
        .map(|byte| match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                char::from(byte).to_string()
            }
            _ => format!("%{byte:02X}"),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_page_transaction_is_taken_once() {
        let transactions = HumanConfidenceTransactions::default();
        let page = transactions.issue().unwrap();
        assert_eq!(page.id().len(), RANDOM_BYTES * 4 / 3);
        assert_eq!(
            transactions.take(page.id()).as_deref(),
            Some(&*page.transaction)
        );
        assert_eq!(transactions.take(page.id()), None, "used once");
        assert_eq!(transactions.take(""), None);
        assert_eq!(transactions.take("never-issued"), None);
    }

    #[test]
    fn two_pages_never_share_an_id_or_a_transaction() {
        let transactions = HumanConfidenceTransactions::default();
        let first = transactions.issue().unwrap();
        let second = transactions.issue().unwrap();
        assert_ne!(first.id(), second.id());
        assert_ne!(first.transaction, second.transaction);
    }

    #[test]
    fn the_oldest_page_is_let_go_when_the_store_is_full() {
        let transactions = HumanConfidenceTransactions::default();
        let oldest = transactions.issue().unwrap();
        for _ in 0..MOST_KEPT {
            transactions.issue();
        }
        assert_eq!(transactions.take(oldest.id()), None);
    }

    #[test]
    fn the_script_url_carries_the_transaction_and_debug_does_not() {
        let transactions = HumanConfidenceTransactions::default();
        let page = transactions.issue().unwrap();
        let url = page.script_url("https://cloud.example.test/api/v4/", "a key/x");
        assert_eq!(
            url,
            format!(
                "https://cloud.example.test/api/v4/a%20key%2Fx.js?transaction={}",
                page.transaction
            )
        );
        let shown = format!("{page:?}");
        assert!(shown.contains(page.id()));
        assert!(!shown.contains(&page.transaction));
    }
}
