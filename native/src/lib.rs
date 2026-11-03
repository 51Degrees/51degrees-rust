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

//! [![51Degrees](https://51degrees.com/img/logo.png?utm_source=docs.rs&utm_medium=docs&utm_campaign=51degrees-rust&utm_content=fiftyone-native-lib.rs&utm_term=logo "Data rewards the curious")](https://51degrees.com/?utm_source=docs.rs&utm_medium=docs&utm_campaign=51degrees-rust&utm_content=fiftyone-native-lib.rs&utm_term=logo)
//!
//! Safe RAII wrapper over the native on-premise engines.
//!
//! This crate is the single safe boundary over the three raw FFI crates
//! ([`fiftyone_common_sys`], [`fiftyone_device_detection_sys`] and
//! [`fiftyone_ip_intelligence_sys`]). It turns the manual, unchecked native
//! life cycle (initialize a resource manager, take handles, create results,
//! process, read values, free in the right order) into ordinary Rust values
//! that free themselves and enforce the correct ordering through ownership.
//!
//! # What it provides
//!
//! - A [`PerformanceProfile`] that selects one of the predefined native
//!   configurations (in memory, high performance, low memory, balanced or the
//!   engine default).
//! - A `Manager` per engine ([`dd::Manager`], [`ipi::Manager`]) that owns a
//!   loaded data set. It is reference counted (returned inside an [`Arc`](std::sync::Arc)) and
//!   is [`Send`] and [`Sync`], because the native resource manager is internally
//!   reference counted and lock protected, so one loaded data set is shared
//!   across threads cheaply. When the data set reads from its file after it is
//!   loaded, no more calls that can read the file run at once than it has file
//!   handles, and a call beyond them waits for one.
//! - A `Results` per engine ([`dd::Results`], [`ipi::Results`]) that owns a
//!   native results structure and holds an [`Arc`](std::sync::Arc) to its manager so the data
//!   set cannot be freed while a result still references it. It is [`Send`] but
//!   deliberately not [`Sync`], because a native results structure is per-thread
//!   scratch that the engine mutates in place.
//! - Lazy property name to required-property-index resolution, computed once per
//!   manager and cached, plus value reads into a reusable, growable per-thread
//!   byte buffer so a fast lookup does not allocate per property.
//! - Evidence marshalling from the pipeline `prefix.field` model into the native
//!   representation, reusing a thread-local pooled evidence array and value
//!   buffers (see [`evidence`]).
//! - Mapping of every native non-success status code and set exception onto
//!   [`fiftyone_pipeline_core::Error::Native`], so the safe layer raises one
//!   error type.
//!
//! # Device Detection and IP Intelligence coexist
//!
//! The two engines compile their own copies of the shared common C layer.
//! The IP Intelligence build widens the file offset types for large data file
//! support, which makes its common C library objects ABI incompatible with the
//! Device Detection (and `fiftyone-common-sys`) build. Both copies once exported
//! the same `fiftyoneDegrees*` C symbols, so linking both engines into one
//! binary made the linker bind each duplicated symbol to a single definition and
//! left one engine reading its data through the wrong common code.
//!
//! That collision is resolved at the source. IP Intelligence compiles its
//! private copy of the common C library into an `ipi_` prefixed symbol
//! namespace (see `fiftyone-ip-intelligence-sys`), so the two copies no longer
//! share a symbol and both engines load real data files side by side in one
//! process. The two engines still get separate [`dd`] and [`ipi`] modules with
//! their own `Manager` and `Results`, because the native types and life cycles
//! differ, and they share the safe machinery in this crate (the profile, the
//! status and exception mapping, the value buffer). Concretely, Device
//! Detection builds a native evidence array through the `fiftyone-common-sys`
//! evidence functions, while IP Intelligence is driven from the client IP
//! string, which is the natural entry point for that engine.
//!
//! # Example
//!
//! ```no_run
//! use fiftyone_native::{PerformanceProfile, dd};
//!
//! # fn main() -> fiftyone_pipeline_core::Result<()> {
//! // Load a Hash data file once and share it across threads.
//! let manager = dd::Manager::open("51Degrees-LiteV4.1.hash", PerformanceProfile::Default)?;
//!
//! // One results structure per thread.
//! let mut results = manager.create_results()?;
//! results.process_user_agent(
//!     "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36",
//! )?;
//!
//! if let Some(is_mobile) = results.value_as_string("IsMobile", ",")? {
//!     println!("IsMobile = {is_mobile}");
//! }
//! # Ok(())
//! # }
//! ```

#![warn(missing_docs)]

// The value buffer is shared machinery used by whichever product is enabled, so
// it is only needed when at least one product feature is on.
#[cfg(any(feature = "dd", feature = "ipi"))]
mod buffer;
#[cfg(any(feature = "dd", feature = "ipi"))]
mod file_reads;
mod status;

// Device Detection is gated behind the `dd` feature and IP Intelligence behind
// `ipi`, and both are on by default. Each product links its own copy of the
// common C library, but IP Intelligence's copy is compiled into an `ipi_`
// prefixed symbol namespace, so the two no longer collide and a binary may
// enable both (see this crate's Cargo.toml). The features remain a slimming
// knob for a consumer that wants only one engine. The product-agnostic items
// below (the performance profile, the status and exception mapping, the
// evidence helpers) compile with either feature or none.
#[cfg(feature = "dd")]
pub mod dd;
pub mod evidence;
#[cfg(feature = "ipi")]
pub mod ipi;
#[cfg(any(feature = "dd", feature = "ipi"))]
mod path;
pub mod profile;

pub use profile::PerformanceProfile;
pub use status::{NativeException, NativeStatus};

#[cfg(test)]
mod tests {
    use super::*;

    /// The performance profile parses the accepted spellings and round-trips
    /// through its canonical name.
    #[test]
    fn performance_profile_parsing() {
        assert_eq!(
            PerformanceProfile::parse("HighPerformance"),
            Some(PerformanceProfile::HighPerformance)
        );
        assert_eq!(
            PerformanceProfile::parse("high-performance"),
            Some(PerformanceProfile::HighPerformance)
        );
        assert_eq!(
            PerformanceProfile::parse("LOW MEMORY"),
            Some(PerformanceProfile::LowMemory)
        );
        assert_eq!(PerformanceProfile::parse("nonsense"), None);
        assert_eq!(PerformanceProfile::default(), PerformanceProfile::Default);
        assert_eq!(PerformanceProfile::Balanced.as_str(), "balanced");
    }

    /// `Manager` is `Send + Sync` and `Results` is `Send` but not `Sync`, the
    /// thread-safety contract documented for the native life cycle. These are
    /// compile-time assertions through trait bounds. The test compiles only when
    /// at least one product is enabled, because there is no engine type to assert
    /// against otherwise.
    #[cfg(any(feature = "dd", feature = "ipi"))]
    #[test]
    fn thread_safety_markers() {
        fn assert_send_sync<T: Send + Sync>() {}
        fn assert_send<T: Send>() {}

        #[cfg(feature = "dd")]
        {
            assert_send_sync::<dd::Manager>();
            assert_send::<dd::Results>();
        }
        #[cfg(feature = "ipi")]
        {
            assert_send_sync::<ipi::Manager>();
            assert_send::<ipi::Results>();
        }
    }
}
