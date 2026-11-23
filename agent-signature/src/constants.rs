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

//! The constants the element uses and that a caller compares its answers
//! against: evidence names, the element data key, the property names, the
//! status values, the reason codes, the protocol values, the algorithm names
//! and the builder defaults.

use std::time::Duration;

/// The name of the `Signature` HTTP header, without a prefix. A request that
/// arrives directly carries it under the `header` prefix, and one forwarded by
/// a caller's own pipeline carries it under `query`.
pub const EVIDENCE_SIGNATURE_NAME: &str = "signature";

/// The name of the `Signature-Input` HTTP header, without a prefix.
pub const EVIDENCE_SIGNATURE_INPUT_NAME: &str = "signature-input";

/// The name of the `Signature-Agent` HTTP header, without a prefix.
pub const EVIDENCE_SIGNATURE_AGENT_NAME: &str = "signature-agent";

/// The evidence key of the `Signature` HTTP header.
pub const EVIDENCE_SIGNATURE_KEY: &str = "header.signature";

/// The evidence key of the `Signature-Input` HTTP header.
pub const EVIDENCE_SIGNATURE_INPUT_KEY: &str = "header.signature-input";

/// The evidence key of the `Signature-Agent` HTTP header.
pub const EVIDENCE_SIGNATURE_AGENT_KEY: &str = "header.signature-agent";

/// The evidence key of the `Host` HTTP header, which the `@authority`
/// component is built from.
pub const EVIDENCE_HOST_KEY: &str = "header.host";

/// The element data key.
pub const DEFAULT_ELEMENT_DATA_KEY: &str = "agent-signature";

/// The property holding the status, one of the `STATUS_` constants.
pub const PROPERTY_STATUS: &str = "agentsignature";

/// The property holding the reason code, one of the `REASON_` constants.
pub const PROPERTY_REASON: &str = "agentsignaturereason";

/// The property holding the `Signature-Agent` member value exactly as the
/// agent sent it.
pub const PROPERTY_AGENT: &str = "agentsignatureagent";

/// The property holding the `keyid` signature parameter.
pub const PROPERTY_KEY_ID: &str = "agentsignaturekeyid";

/// The property holding the signature algorithm.
pub const PROPERTY_ALGORITHM: &str = "agentsignaturealgorithm";

/// The property holding the `created` signature parameter as a point in time.
pub const PROPERTY_CREATED: &str = "agentsignaturecreated";

/// The property holding the `expires` signature parameter as a point in time.
pub const PROPERTY_EXPIRES: &str = "agentsignatureexpires";

/// The property holding the `nonce` signature parameter.
pub const PROPERTY_NONCE: &str = "agentsignaturenonce";

/// The property holding what the agent says it uses fetched pages for.
pub const PROPERTY_PURPOSE: &str = "agentsignaturepurpose";

/// The property holding the agent name from the agent card.
pub const PROPERTY_NAME: &str = "agentsignaturename";

/// The property holding the robots.txt product token from the agent card.
pub const PROPERTY_PRODUCT_TOKEN: &str = "agentsignatureproducttoken";

/// The property holding the address of the agent card.
pub const PROPERTY_CARD_URL: &str = "agentsignaturecardurl";

/// The request carried no signature headers. This is the normal value,
/// because only a handful of agents sign today.
pub const STATUS_ABSENT: &str = "Absent";

/// The request carried a signature and the signature, or one of the headers
/// carrying it, is wrong.
pub const STATUS_INVALID: &str = "Invalid";

/// The request carried a signature that could not be checked. This is not
/// evidence against the agent.
pub const STATUS_UNVERIFIED: &str = "Unverified";

/// The key directory was still being fetched when the wait budget ran out.
/// The fetch continues, so a later request from the same agent finds the
/// result.
pub const STATUS_TIMEOUT: &str = "Timeout";

/// The signature checked out against a key the agent publishes.
pub const STATUS_VERIFIED: &str = "Verified";

/// No `Signature` and no `Signature-Input` header was present.
pub const REASON_NO_SIGNATURE: &str = "NoSignature";

/// One of the two headers was present without the other, or one of the three
/// headers could not be read.
pub const REASON_MALFORMED: &str = "Malformed";

/// No signature carried the `web-bot-auth` tag.
pub const REASON_TAG_MISMATCH: &str = "TagMismatch";

/// One of the required `created`, `expires` and `keyid` parameters was
/// missing.
pub const REASON_MISSING_PARAMETER: &str = "MissingParameter";

/// The signature had expired, or it was valid for longer than the configured
/// maximum lifetime.
pub const REASON_EXPIRED: &str = "Expired";

/// The signature was created further in the future than the configured clock
/// skew allows.
pub const REASON_NOT_YET_VALID: &str = "NotYetValid";

/// The signature named no agent, so there was nowhere to fetch a key from.
pub const REASON_NO_AGENT: &str = "NoAgent";

/// The agent carried its key set inline in a `data:` URI and the element is
/// not set to accept that.
pub const REASON_INLINE_DIRECTORY: &str = "InlineDirectory";

/// The signature covered neither `@authority` nor `@target-uri`, so nothing
/// ties it to the request it arrived on.
pub const REASON_UNBOUND_SIGNATURE: &str = "UnboundSignature";

/// The signature covered a part of the request that cannot be rebuilt from
/// the evidence the pipeline holds.
pub const REASON_COMPONENT_UNAVAILABLE: &str = "ComponentUnavailable";

/// The key directory was still being fetched when the wait budget ran out.
pub const REASON_DIRECTORY_PENDING: &str = "DirectoryPending";

/// The key directory could not be fetched or could not be read.
pub const REASON_DIRECTORY_UNAVAILABLE: &str = "DirectoryUnavailable";

/// The key directory was read and holds no key with the key id the signature
/// names, which is evidence that the key was withdrawn.
pub const REASON_UNKNOWN_KEY: &str = "UnknownKey";

/// The key itself was not valid at the time the signature was created.
pub const REASON_KEY_EXPIRED: &str = "KeyExpired";

/// The signature uses an algorithm this element does not verify.
pub const REASON_UNSUPPORTED_ALGORITHM: &str = "UnsupportedAlgorithm";

/// The signature did not check out against the key the agent publishes.
pub const REASON_SIGNATURE_MISMATCH: &str = "SignatureMismatch";

/// The signature checked out against a key the agent publishes.
pub const REASON_VERIFIED: &str = "Verified";

/// The tag a Web Bot Auth request signature carries.
pub const TAG_WEB_BOT_AUTH: &str = "web-bot-auth";

/// The tag the signature over a key directory response carries.
pub const TAG_DIRECTORY: &str = "http-message-signatures-directory";

/// The path a key directory is served from, relative to the origin a
/// `Signature-Agent` member of type `directory` names.
pub const DIRECTORY_PATH: &str = "/.well-known/http-message-signatures-directory";

/// The media type a key directory is served with.
pub const DIRECTORY_MEDIA_TYPE: &str = "application/http-message-signatures-directory+json";

/// The media type a set of keys named directly, or an agent card, is served
/// with.
pub const JSON_MEDIA_TYPE: &str = "application/json";

/// The media type asked for when a registry of agent cards is fetched.
pub const REGISTRY_MEDIA_TYPE: &str = "text/plain";

/// The `Signature-Agent` member type naming an origin that serves a key
/// directory. This is the default when no type is given.
pub const AGENT_TYPE_DIRECTORY: &str = "directory";

/// The `Signature-Agent` member type naming the address of the keys directly.
pub const AGENT_TYPE_JWKS_URI: &str = "jwks_uri";

/// The `Signature-Agent` member type naming an agent card.
pub const AGENT_TYPE_CIMD: &str = "cimd";

/// The RFC 9421 registry name for Ed25519.
pub const ALGORITHM_ED25519: &str = "ed25519";

/// The RFC 9421 registry name for RSASSA-PSS using SHA-512.
pub const ALGORITHM_RSA_PSS_SHA512: &str = "rsa-pss-sha512";

/// The RFC 9421 registry name for ECDSA over the P-256 curve using SHA-256.
pub const ALGORITHM_ECDSA_P256_SHA256: &str = "ecdsa-p256-sha256";

/// The RFC 9421 registry name for HMAC using SHA-256. The Web Bot Auth
/// protocol forbids shared secrets, so this element never verifies a
/// signature that uses it.
pub const ALGORITHM_HMAC_SHA256: &str = "hmac-sha256";

/// The default number of key directories held in the cache.
pub const DEFAULT_CACHE_SIZE: usize = 1000;

/// The default period a fetched key directory is reused for, which is the
/// `max-age` the drafts recommend a directory is served with.
pub const DEFAULT_CACHE_LIFETIME: Duration = Duration::from_secs(24 * 60 * 60);

/// The default period a failed fetch is remembered for, so an outage at one
/// agent does not cause a fetch for every request.
pub const DEFAULT_NEGATIVE_CACHE_LIFETIME: Duration = Duration::from_secs(5 * 60);

/// The default period a request waits for a key directory fetch before the
/// status becomes Timeout.
pub const DEFAULT_WAIT_BUDGET: Duration = Duration::from_millis(350);

/// The default time limit on a single key directory fetch.
pub const DEFAULT_FETCH_TIMEOUT: Duration = Duration::from_secs(5);

/// The default tolerance on the `created` and `expires` signature parameters,
/// which allows for clocks that differ a little.
pub const DEFAULT_CLOCK_SKEW: Duration = Duration::from_secs(60);

/// The default maximum lifetime of a signature. Zero means no limit, because
/// the protocol draft only recommends one.
pub const DEFAULT_MAX_LIFETIME: Duration = Duration::ZERO;

/// The default for whether the bare quoted string form of the
/// `Signature-Agent` header is accepted.
pub const DEFAULT_ALLOW_LEGACY_SIGNATURE_AGENT: bool = true;

/// The default for whether a key set carried inline in a `data:` URI is
/// accepted. Off, because such a key set is chosen by whoever sent the
/// request, so a signature that checks out against it says nothing about
/// which agent sent the request.
pub const DEFAULT_ALLOW_INLINE_DIRECTORY: bool = false;

/// The default for whether evidence under the `query` prefix is trusted to
/// describe the request that was signed. Off, because a web integration turns
/// query string parameters into evidence under that prefix, so a visitor could
/// otherwise put a signature, a host and a path in the address bar and have
/// those checked instead of the request that actually arrived.
pub const DEFAULT_TRUST_FORWARDED_EVIDENCE: bool = false;

/// The default number of bytes read from a key directory, an agent card or a
/// registry before the fetch is abandoned. The address is chosen by whoever
/// sent the request, so the size has to be held down.
pub const DEFAULT_MAX_RESPONSE_BYTES: usize = 256 * 1024;

/// The most key directory fetches one element runs at once. Each fetch runs
/// on a thread of its own and the sender of a request chooses the address,
/// so without a bound a stream of requests naming new addresses that never
/// answer would hold a thread each for the whole fetch timeout. A fetch that
/// cannot start because the bound is reached reports DirectoryUnavailable,
/// and a later request for that directory tries again.
pub const MAXIMUM_FETCHES_RUNNING: usize = 64;

/// The longest period any of the builder's period settings may be given.
pub const MAXIMUM_PERIOD: Duration = Duration::from_secs(365 * 24 * 60 * 60);
