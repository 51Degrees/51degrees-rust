# 51Did examples

Runnable 51Did examples for the 51Degrees Rust SDK. The crate is a member of
the `examples` workspace, beside the device detection, IP intelligence and
pipeline example crates, which is where CI builds and tests every runnable
example. The single-file offline reader example lives with the `fodid` crate
at `fodid/examples/parse_and_verify.rs`.

## Creator context web demo

`src/bin/fodid-web-creator-context.rs` serves `assets/page.html` from a small
axum web server. Every 51Did the 51Degrees cloud issues carries a creator
context, which binds the identifier to the browser and connection it was
created on. The page runs the full flow the way production does:

1. **Create** a 51Did by calling the cloud `json` endpoint from the browser,
   which issues an identifier for the browser's own connection. The request
   asks for every kind of identifier at once and the page shows all six.
2. **Verify** it with `verify-full`, again from the browser, so the cloud
   observes the browser's live connection. The answer is only an encrypted
   `result` that the browser can neither read nor forge, with the signature
   outcome and the creator context verdict sealed inside it.
3. **Redeem** the encrypted result on the demo's own server, which parses
   the 51Did, checks its signature offline against the cloud's public key
   for its date, then calls `redeem` with the 51Did, the encrypted result
   and the account's licence key, and receives the signature outcome, the
   true creator context verdict, when the verification happened
   (`verifiedAt`) and how long ago that was (`secondsSinceVerified`). The
   server does this through the `fodid` crate's cloud client, `DidClient`,
   so it writes no HTTP or key handling of its own.

The licence key lives on the server and only there. A fresh single-use
challenge is issued per page load and bound through both verification steps
by the cloud. A production server would also remember the value it issued and
reject a redemption carrying any other, which this demo keeps out of scope.

The page carries the licensed probabilistic identifier through verification
where the account holds licence keys, otherwise the global one. Both carry the
creator context where the issuing cloud emits it. An account holding no
licence keys returns no licensed identifier at all.

The context verdict is whatever the cloud decided, and `nocontext` is a
normal one rather than an error. A self-hosted container may be configured
not to emit the creator context, so an identifier it issued redeems as
`nocontext`, and the page shows that verdict the way it shows any other.
Only a transport failure or an answer other than a 2xx status is an error,
which the page reports as a failure with the status and body the cloud
sent. The demo server answers the page with the cloud's status and a JSON
body in the cloud's own shape (`signature`, `context`, `factors` when
present, `verifiedAt`, `secondsSinceVerified`), built from the client's
typed result, plus one extra field, `serverSignature`, with `verified` or
`invalid` from the server's own offline signature check. The page ignores
fields it does not know, so the page is the same in every language. The
server answers 502 with a JSON error of its own only when the cloud cannot
be reached.

A 404 from `verify-full` or `redeem` means the host answering does not
offer the creator context at all, which is a service without the feature
rather than a failed check, and the page says so, naming the service and
asking to be pointed at one that does. The demo server answers that case
with a 404 of its own.

### Environment variables

| Variable | Meaning |
| --- | --- |
| `_51DEGREES_RESOURCE_KEY` | Required. The page's resource key, public by nature. The same name without the leading underscore and the CI names `_51DEGREES_RESOURCE_KEY_BESPOKE` and `_51DEGREES_RESOURCE_KEY_BASIC` are read after it, as every cloud example in this repository does. Without one the demo prints which variables it looked at and exits. |
| `_51DEGREES_LICENSE_KEY` | Optional. A licence key of the same account, used server side only. Only an account that holds licence keys needs one to redeem, so an account holding none runs without it. |
| `51DEGREES_CLOUD_ENDPOINT` | Optional. The cloud API base including the `/api/v4/` segment, defaulting to `https://cloud.51degrees.com/api/v4/`. A host other than cloud.51degrees.com would be used to (a) use an on premise web server, or (b) use a privately hosted version of the 51Degrees cloud for performance reasons, which is the private hosting option of the 51Degrees cloud service. Both run the same service, so the demo works unchanged. This is the same variable the cloud request engine honours, so a developer who has set it once points every 51Degrees example at the same place. |
| `PORT` | Optional. The port to listen on, defaulting to `5100`. |

### Running

From the `examples` directory:

```sh
export _51DEGREES_RESOURCE_KEY="<your resource key>"
cargo run -p fodid-examples --bin fodid-web-creator-context
```

Then open <http://localhost:5100/>. The server listens on every interface, so
to demonstrate across two devices open the page by this machine's network
address and copy the link from there.

To build against the local source tree rather than the published crates, add
`--config source.toml` as for the other example crates.

### The server-side step, for your own server

The one part of this demo a developer copies into their own server is the
redeem step, which is the `redeem_with` function in
`src/bin/fodid-web-creator-context.rs`. The page sends it the 51Did, the
encrypted result from `verify-full` and the challenge the page was served
with, and the server's `DidClient` adds the licence key, which is the only
thing the browser must never see. The client is built once at start-up
from the same environment variables the demo reads, and its essential lines
are these.

```rust
use fodid_client::DidClient;
use fodid::FodId;

// At start-up. The endpoint defaults to the public cloud or the
// 51DEGREES_CLOUD_ENDPOINT environment variable.
let client = DidClient::builder(resource_key)
    .licence_key(licence_key)
    .build()?;

// Per request. The page sends the URL-safe alphabet, which is accepted.
let fod_id = FodId::from_base64(&query.fodid)?;
// Offline, against the cloud's public key for the identifier's date. The
// keys are fetched once and cached.
let server_signature = client.verify_signature(&fod_id).await?;
// With the licence key, one use against the resource key.
let redeemed = client
    .redeem(&fod_id, &query.result, Some(&query.challenge))
    .await?;
```

`redeemed` is a typed `RedeemResult` with the signature outcome, the
creator context verdict, the factors where the verdict is a mismatch,
`verified_at` and `seconds_since_verified`, and the handler answers the
page with those in the cloud's own shape plus `serverSignature`. A host
without the creator context raises `ClientError::NotSupported`, which the
handler turns into a 404, and a cloud that cannot be reached raises
`ClientError::Transport`, which becomes a 502. A production server would
also check that the challenge is one it issued and has not been redeemed
before.

### What a run costs

Every call the page or the server makes to the cloud is one use against the
subscription behind the resource key. A browser checking a 51Did makes two,
`verify-full` from the page and `redeem` from the server, so a browser-based
context check is two uses every time. Checking only the signature with
`verify` is one use. The server's own offline signature check costs nothing
per identifier, because the public keys are fetched once and cached.

### The copy-and-paste proof

Once the 51Did has fully validated, the page shows a copy-and-paste section
with a link carrying the same 51Did, and an explanation of what will happen
next. Open that link in a **different browser** and the same page loads with
the same identifier. The signature still verifies and the identifier unpacks,
because it is genuine, but the creator context does **not** validate, because
the context binds the identifier to the browser and connection it was created
on. That visible failure is the demonstration that matters, a copied or stolen
identifier caught at presentation with nothing stored server side. Opening
the link in the same browser is not the demonstration, since the same browser
presents the same context and may still verify.

### The stylesheet

The page is styled with the shared 51Degrees example design system and holds
no styles of its own. The page stays byte for byte identical to the other
language versions and loads `examples-main.min.css` through a relative URL.
The server answers that URL with the copy embedded by the
`examples-web-shared` crate, which every Rust web example shares. That copy is
refreshed by the weekly example assets update workflow, so the demo does not
drift from the design system.
