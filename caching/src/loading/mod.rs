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

//! The loading cache, its stores and its loaders. The crate documentation
//! describes how they fit together.

mod byte_store;
mod cache;
mod clock;
mod codec;
mod entry;
mod flight;
mod loader;
mod lru_store;
mod shards;
mod spawn;
mod store;

pub use byte_store::{ByteLookup, ByteStore, EncodedStore, EncodedStoreBuilder, ListKeys};
pub use cache::{LoadingCache, LoadingCacheBuilder, LruLoadingCache};
pub use clock::Clock;
#[cfg(not(all(target_family = "wasm", target_os = "unknown")))]
pub use clock::SystemClock;
pub use codec::{
    decode_entry, encode_entry, encode_entry_published, Codec, DecodeError, Raw, Stored, Utf8,
    ENTRY_FORMAT,
};
#[cfg(all(feature = "disk", not(target_family = "wasm")))]
pub(crate) use codec::{read_header, Header};
pub use entry::{Entry, Loaded};
pub use loader::{from_fn, FnLoader, ValueLoader};
pub use lru_store::{LruStore, LruStoreBuilder};
pub use spawn::{
    Inline, LoadRunner, LoadTask, Spawn, SpawnLocal, Spawned, SpawnedLocal, StartLoad,
};
pub use store::{Lookup, Store};
