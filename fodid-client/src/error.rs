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

//! What can go wrong, and whose problem each thing is.
//!
//! Every field of free text an error carries goes through
//! [`crate::redact`] before it is shown, in both the `Display` and the `Debug`
//! form, because an address carries the resource key in its route and the
//! service repeats the key back inside its own message. `Debug` matters as
//! much as `Display`, since `unwrap` and `expect` print that one.

/// The result type used throughout this crate.
pub type Result<T> = std::result::Result<T, Error>;

/// Why a call did not produce an answer.
///
/// Kept separate from the answers themselves on purpose. A redemption that
/// comes back `mismatch`, or a signature that does not verify, is an answer
/// and is returned as one. These are the cases where there was no answer to
/// return.
#[derive(thiserror::Error)]
pub enum Error {
    /// The value given is not something this client will send, being empty,
    /// too long to be an identifier, or not a 51Did at all. Named for what it
    /// is here rather than sent to the service to be refused there.
    #[error("invalid argument: {}", crate::redact::redact(.0))]
    InvalidArgument(String),

    /// The request did not complete. The service could not be reached, the
    /// connection failed or timed out, or the answer could not be read.
    #[error("transport: {}", crate::redact::redact(.0))]
    Transport(String),

    /// The service answered with a status this client did not expect for that
    /// endpoint, carrying the status and the start of the body.
    #[error("the 51Did {endpoint} endpoint answered {status}: {}", crate::redact::redact(.body))]
    UnexpectedStatus {
        /// Which endpoint answered.
        endpoint: &'static str,
        /// The HTTP status.
        status: u16,
        /// The start of the body, truncated.
        body: String,
    },

    /// The service answered in a shape this client could not read, for
    /// example a key list that is not a JSON array.
    #[error("protocol: {}", crate::redact::redact(.0))]
    Protocol(String),

    /// The service at this endpoint does not support the 51Did creator
    /// context, answering the redeem endpoint with 404.
    #[error("the service at {} does not support the 51Did creator context", crate::redact::redact(.0))]
    NotSupported(String),

    /// The signing key published for the identifier's date could not be used
    /// to verify, for example because it is not a key this build can read.
    #[error("the published signing key could not be used: {}", crate::redact::redact(.0))]
    KeyUnusable(String),
}

/// Debug is written by hand rather than derived so the free-text fields go
/// through [`crate::redact`] as well.
///
/// Display alone would not be enough, because `Result::unwrap` and
/// `Result::expect` print the `Debug` form, and that is how a resource key
/// reaches a test log. The shape matches what the derive would print, so
/// nothing a reader relies on changes apart from the masked values.
impl core::fmt::Debug for Error {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Error::InvalidArgument(message) => f
                .debug_tuple("InvalidArgument")
                .field(&crate::redact::redact(message))
                .finish(),
            Error::Transport(message) => f
                .debug_tuple("Transport")
                .field(&crate::redact::redact(message))
                .finish(),
            Error::UnexpectedStatus {
                endpoint,
                status,
                body,
            } => f
                .debug_struct("UnexpectedStatus")
                .field("endpoint", endpoint)
                .field("status", status)
                .field("body", &crate::redact::redact(body))
                .finish(),
            Error::Protocol(message) => f
                .debug_tuple("Protocol")
                .field(&crate::redact::redact(message))
                .finish(),
            Error::NotSupported(endpoint) => f
                .debug_tuple("NotSupported")
                .field(&crate::redact::redact(endpoint))
                .finish(),
            Error::KeyUnusable(message) => f
                .debug_tuple("KeyUnusable")
                .field(&crate::redact::redact(message))
                .finish(),
        }
    }
}

impl Error {
    /// Cuts a body down to something that fits in an error message.
    ///
    /// The body is cleaned before it is cut, so that cutting it can never
    /// leave the front half of a credential behind.
    pub(crate) fn truncate(body: &str) -> String {
        const LIMIT: usize = 200;
        let body = crate::redact::redact(body);
        if body.chars().count() <= LIMIT {
            body.into_owned()
        } else {
            let mut out: String = body.chars().take(LIMIT).collect();
            out.push_str("...");
            out
        }
    }
}
