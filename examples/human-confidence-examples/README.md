# Human Confidence example

Human Confidence says how likely it is that a person is looking at a web
page. This crate is a small web server and one page that show it working,
the way a publisher's website would use it. It's a web demo rather than a
console program, because the browser tests only run in a browser.

| Example | Description |
| --- | --- |
| `human-confidence-web` | A demo web server that serves `assets/page.html`, asks the cloud for the passive level, and reads the two Properties the cloud request engine gives for the sealed active level. It uses axum and reqwest, as the other Rust web examples do. |

## The flow

The page shows three rows, one for each step.

1. **Passive.** The server asks the cloud itself, sending the visitor's
   request headers and IP address. It passes on only the headers the cloud
   lists in its `evidencekeys`, so headers such as `Authorization` stay on
   the server. The answer has `HumanConfidenceSource` set to `Passive` and
   `HumanConfidence` set to a plain level, `High`, `Medium`, or `Low`. The
   server asks for it rather than reading it from the page, because a level
   sent by a browser can be forged.
2. **Active.** The page loads the 51Degrees client script, which runs the
   browser tests. They take a few seconds. When the script completes,
   `HumanConfidenceSource` is `Active` and `HumanConfidence` holds a sealed
   value. The browser can't read it or change it.
3. **Read.** When the server serves the page, it gives the page an id and
   keeps a transaction for it. The page loads the client script from an
   address that carries the transaction, so the sealed value is bound to
   this page. The page hands the sealed value and its id to the server as
   `humanconfidence` and `humanconfidencepage`. The server processes them
   as evidence through a Pipeline with the cloud request engine, which
   redeems the sealed value with the License Key and the kept transaction
   in its call to the cloud, and reads the two Properties with
   `human_confidence`: `HumanConfidenceSource`, always `Active`, and
   `HumanConfidence`, the level, or `Medium` where the sealed value wasn't
   redeemed. A page id the server didn't give, or one already used, is not
   redeemed. The example never redeems a sealed value itself.

At startup the server checks the cloud's `accessibleproperties` for the
Resource Key. If the key doesn't include `HumanConfidence`, the console and
the page say so and link to
[contact us](https://51degrees.com/contact-us?utm_source=code&utm_medium=example&utm_campaign=51degrees-rust&utm_content=examples-human-confidence-examples-readme.md&utm_term=human-confidence).

## On your own page

Naming `HumanConfidence` makes the client script run the browser tests and
send their answers with one more request, so its complete handlers run later
than they otherwise would, by the time the tests take and that request. A
page that wants the first response's Properties sooner reads them through
the client script's change handlers.

Where a page sets a Content Security Policy, the browser tests run only if
it allows these:

| Directive | Must allow |
| --- | --- |
| `script-src` | The cloud's host, and `'unsafe-eval'` |
| `connect-src` | The cloud's host |
| `worker-src` | `blob:` |

A page whose policy blocks any of them runs no browser tests, and its
`HumanConfidenceSource` stays `Passive`.
## What you need

- **A Resource Key** for an account on the Pro or Bespoke plan, with the
  properties `HumanConfidence` and `HumanConfidenceSource`.
- **Leave the Resource Key's domain list empty,** or include `localhost`
  and the server's own address. The server calls the cloud with the same
  key, and a call from a server has no `Origin` header, so a key limited to
  domains is refused.
- **A License Key** of the same account. The cloud request engine redeems
  with it. It never goes in the page. Without it every sealed value reads as
  `Medium`.

## Environment variables

| Variable | Meaning |
| --- | --- |
| `_51DEGREES_RESOURCE_KEY` | The Resource Key. Required. |
| `_51DEGREES_LICENSE_KEY` | A License Key of the same account. Server side only. The cloud request engine redeems with it. |
| `_51DEGREES_CLOUD_ENDPOINT` | Optional. The API base including the `/api/v4/` segment. It defaults to `https://cloud.51degrees.com/api/v4/`. |
| `_51DEGREES_EXAMPLE_HTTP_PORTS` | Optional. The port to listen on. It defaults to `5151`. |

## How to run

From the `examples` directory, in a bash shell:

```sh
export _51DEGREES_RESOURCE_KEY="your Resource Key"
export _51DEGREES_LICENSE_KEY="your License Key"
cargo run -p human-confidence-examples --bin human-confidence-web
```

Or in PowerShell:

```
$env:_51DEGREES_RESOURCE_KEY = "your Resource Key"
$env:_51DEGREES_LICENSE_KEY = "your License Key"
cargo run -p human-confidence-examples --bin human-confidence-web
```

Then open `http://localhost:5151/` in a browser. The server listens on
every interface, so the page can also be opened from a second device on
the same network. To build against the local
source tree rather than the published crates, add `--config source.toml` as
for the other example crates.

The tests run against a fake cloud on a local port, with
`cargo test -p human-confidence-examples`. Four more run against the cloud
service, and are ignored otherwise:
`cargo test -p human-confidence-examples -- --ignored`. With a Resource Key
in `_51DEGREES_RESOURCE_KEY_BESPOKE`, one checks that the key includes
`HumanConfidence`. The production cloud does not serve
`HumanConfidenceSource` yet, so the other three ask the staging cloud that
`CLOUD_STAGING_ENDPOINT` names, with the Bespoke key in
`RESOURCE_KEY_BESPOKE_STAGING`, and skip when either is not set. Only CI sets
those two names. They check that the demo's passive call answers a level,
and that a passive call naming both properties answers
`HumanConfidenceSource` as `Passive`. With a key outside the Bespoke product
in `_51DEGREES_RESOURCE_KEY_BASIC`, the same call answers the source as
null, with the reason. The
active source comes back only to a browser that has run the tests, so these
tests don't check it.

## What you should see

1. The first row shows `Passive` and a level.
2. After a few seconds, the second row shows `Active` and a sealed value.
3. The third row shows `Active` and the level the server read.

If the second row stays `Passive`, the browser tests didn't run. The cloud
you're using may not have the tests yet, or the Resource Key may not
include `HumanConfidenceSource`.

## What a run costs

Every call to the cloud is one use against the subscription behind the
keys. A page load makes several, the server's passive call, the client
script's calls, and the call that redeems the sealed value.

## Find out more

- [Documentation](https://51degrees.com/documentation/index.html?utm_source=code&utm_medium=example&utm_campaign=51degrees-rust&utm_content=examples-human-confidence-examples-readme.md&utm_term=find-out-more)
- [Pricing](https://51degrees.com/pricing?utm_source=code&utm_medium=example&utm_campaign=51degrees-rust&utm_content=examples-human-confidence-examples-readme.md&utm_term=find-out-more)
- [Contact us](https://51degrees.com/contact-us?utm_source=code&utm_medium=example&utm_campaign=51degrees-rust&utm_content=examples-human-confidence-examples-readme.md&utm_term=find-out-more)
