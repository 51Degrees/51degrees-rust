[![51Degrees](https://raw.githubusercontent.com/51Degrees/51degrees-rust/main/.github/images/logo-160.png "Data rewards the curious")](https://51degrees.com/?utm_source=github&utm_medium=readme&utm_campaign=51degrees-rust&utm_content=fiftyone-agent-signature-readme.md&utm_term=logo)

# 51Degrees Agent Signature

`AgentSignatureElement` for the 51Degrees pipeline: checks the HTTP Message
Signature an automated agent sends under the IETF Web Bot Auth protocol, and
reports what the signature proves.

This crate is part of the [51Degrees](https://51degrees.com/?utm_source=github&utm_medium=readme&utm_campaign=51degrees-rust&utm_content=fiftyone-agent-signature-readme.md&utm_term=introduction) Rust solution for high-performance
device detection and IP intelligence, available both on-premise from a local
data file and from the 51Degrees cloud.

## What it does

An agent is a program that fetches pages on someone's behalf, such as a search
crawler or a tool that reads pages for a person. An agent that signs its
requests sends three headers, `Signature`, `Signature-Input` and
`Signature-Agent`, as
[RFC 9421](https://www.rfc-editor.org/rfc/rfc9421.html) and the Web Bot Auth
drafts describe. The element rebuilds the text the agent signed from the
request, fetches the public key the agent publishes, and checks the signature
with Ed25519, RSA-PSS with SHA-512 or ECDSA over P-256.

It reports twelve properties under the element data key `agent-signature`.

| Property | What it holds |
| --- | --- |
| `AgentSignature` | Absent, Invalid, Unverified, Timeout or Verified. Always has a value. |
| `AgentSignatureReason` | Why, as one of seventeen reason codes. Always has a value. |
| `AgentSignatureAgent` | The `Signature-Agent` member value as the agent sent it. |
| `AgentSignatureKeyId` | The `keyid` parameter, the thumbprint of the key. |
| `AgentSignatureAlgorithm` | The algorithm the signature was checked with. |
| `AgentSignatureCreated` | When the signature was made. |
| `AgentSignatureExpires` | When the signature stops being valid. |
| `AgentSignatureNonce` | The `nonce` parameter, for the caller to check is never reused. |
| `AgentSignaturePurpose` | What the agent says it uses fetched pages for. |
| `AgentSignatureName` | The name from the agent card. |
| `AgentSignatureProductToken` | The name the agent answers to in a robots.txt file. |
| `AgentSignatureCardUrl` | The address of the agent card. |

Each property with no value says why. Almost no traffic is signed today, so a
request with no signature reports Absent, which is never evidence against the
request, and the element never fails a request whatever the headers hold.

## Using it

Turn on the `reqwest-client` feature for the built-in HTTP client, which
follows no redirects, or supply a client of your own with `set_http_client`.

```toml
[dependencies]
fiftyone-agent-signature = { version = "4.5", features = ["reqwest-client"] }
```

```rust
use std::sync::Arc;

use fiftyone_agent_signature::AgentSignatureElement;
use fiftyone_pipeline_core::Pipeline;

let element = AgentSignatureElement::builder().build()?;
let pipeline = Pipeline::builder()
    .add_element(Arc::new(element))
    .build()?;

let mut data = pipeline.create_flow_data_with(evidence);
data.process()?;

let result = data.get(AgentSignatureElement::KEY).unwrap();
println!("{:?}", result.agent_signature());
```

The element asks for every `header.` evidence key, because a signature may
cover any header. A signature that covers the method, the path or the query
also needs `server.request-method`, `server.request-path` and
`server.request-query`, exactly as the request line carried them. The web
integration in `fiftyone-pipeline-web` fills them where its adapter supplies
the request line.

## Fetching keys

The first request from an agent starts one fetch of the agent's key directory
on a thread of its own and waits for at most the wait budget, 350 milliseconds
by default. Where the fetch has not finished by then, the request reports
Timeout and the fetch carries on, so a later request from that agent finds the
keys. This is deliberate, and not a fault. The directories are held in a
bounded cache for a day, a failed fetch is remembered for five minutes, and no
more than 64 fetches run at once. Every address is checked before it is
fetched, so an address written as an IP address in a private or loopback range
is never fetched, and a document that arrives from any address other than the
one asked for is refused. A name such as `localhost`, or one that resolves to a
private address, cannot be refused this way, because the name is resolved when
the connection is made, so a deployment facing the public internet should
route these fetches through an outbound proxy that enforces an allow list.

Where registries of agent cards are configured, each must be HTTPS with no user
information, and they are read once for the life of the element. A registry or
card that cannot be fetched at that read is not tried again until the element
is built again. Only the card properties are affected.

## Settings

| Builder method | Default |
| --- | --- |
| `set_http_client` | The built-in client, with the `reqwest-client` feature |
| `set_cache_size` | 1000 directories |
| `set_cache_lifetime` | 24 hours |
| `set_negative_cache_lifetime` | 5 minutes |
| `set_wait_budget` | 350 milliseconds |
| `set_fetch_timeout` | 5 seconds |
| `set_clock_skew` | 60 seconds |
| `set_max_lifetime` | No limit |
| `set_registry` | None |
| `set_trust_forwarded_evidence` | Off |
| `set_allow_legacy_signature_agent` | On |
| `set_allow_inline_directory` | Off |
| `set_max_response_bytes` | 262,144 bytes |
| `set_reachability_check_url` | None |

## License

Licensed under the European Union Public Licence v1.2 (EUPL-1.2). See the
[repository](https://github.com/51Degrees/51degrees-rust) for the full text.

## Find out more

- Source and issues: [github.com/51Degrees/51degrees-rust](https://github.com/51Degrees/51degrees-rust)
- API documentation: [docs.rs/fiftyone-agent-signature](https://docs.rs/fiftyone-agent-signature)
- The element specification: [51Degrees/specifications](https://github.com/51Degrees/specifications/blob/main/pipeline-specification/pipeline-elements/agent-signature-element.md)
- Device detection, which gives `IsCrawler`, `CrawlerName` and `IsArtificialIntelligence`: [51degrees.com/device-detection](https://51degrees.com/device-detection?utm_source=docs.rs&utm_medium=package&utm_campaign=51degrees-rust&utm_content=fiftyone-agent-signature-readme.md&utm_term=device-detection)
- Build custom data services with the Pipeline API: [51degrees.com/build-your-own](https://51degrees.com/build-your-own?utm_source=docs.rs&utm_medium=package&utm_campaign=51degrees-rust&utm_content=fiftyone-agent-signature-readme.md&utm_term=build-your-own)
- Pipeline API documentation: [51degrees.com/documentation](https://51degrees.com/documentation/_pipeline_api__index.html?utm_source=docs.rs&utm_medium=package&utm_campaign=51degrees-rust&utm_content=fiftyone-agent-signature-readme.md&utm_term=documentation)
- About 51Degrees: [51degrees.com](https://51degrees.com/?utm_source=docs.rs&utm_medium=package&utm_campaign=51degrees-rust&utm_content=fiftyone-agent-signature-readme.md&utm_term=about)
- Data files and pricing: [51degrees.com/pricing](https://51degrees.com/pricing?utm_source=docs.rs&utm_medium=package&utm_campaign=51degrees-rust&utm_content=fiftyone-agent-signature-readme.md&utm_term=pricing)
