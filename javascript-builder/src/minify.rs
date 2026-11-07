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

//! JavaScript minification with the oxc toolchain.
//!
//! With the default-on `minify` feature, the rendered template is parsed,
//! minified and printed by the pure-Rust oxc parser, minifier and code
//! generator. A failure at any stage (a parse error, or a panic from an edge
//! case in the minifier) falls back to the original, unminified content and
//! says why in [`MinifyOutcome::errors`], so valid JavaScript is always served.
//!
//! oxc runs on a thread of its own, and a panic in it ends that thread and
//! becomes the unminified fallback when the thread is joined.
//!
//! Without the `minify` feature, and on wasm targets (where the oxc dependency
//! is not compiled, see the crate's target-specific dependency), the rendered
//! template is returned unchanged. That output is correct, just not compacted.

/// The outcome of producing the JavaScript to store for a request.
pub struct MinifyOutcome {
    /// The content to store. This is the minified JavaScript on success, or the
    /// original rendered content when minification was not performed or failed.
    pub content: String,
    /// What went wrong when minification was attempted but failed and the
    /// original content was kept. Empty on success and when the `minify`
    /// feature is off.
    pub errors: Vec<String>,
}

/// Return the JavaScript to store for this request.
///
/// With the `minify` feature the content is minified by oxc, falling back to the
/// original content (with `errors` saying why) on any failure. Without the
/// feature the content is returned unchanged.
pub fn minify(content: String) -> MinifyOutcome {
    minify_impl(content)
}

#[cfg(all(feature = "minify", not(target_family = "wasm")))]
fn minify_impl(content: String) -> MinifyOutcome {
    match try_minify(&content) {
        Ok(minified) => MinifyOutcome {
            content: minified,
            errors: Vec::new(),
        },
        Err(errors) => MinifyOutcome { content, errors },
    }
}

#[cfg(not(all(feature = "minify", not(target_family = "wasm"))))]
fn minify_impl(content: String) -> MinifyOutcome {
    // No minifier is compiled in. Serving the rendered template unchanged is by
    // design, not a failure, so there are no errors.
    MinifyOutcome {
        content,
        errors: Vec::new(),
    }
}

/// Minify `source` with oxc on a dedicated thread, returning what went wrong on
/// any parse error or panic so the caller serves the unminified content.
///
/// oxc parses and minifies by recursing over the AST. On a thread with a small
/// stack a deep script could overflow it, which surfaces on Windows as a process
/// access violation (exit 0xc0000005) rather than a catchable panic. The Windows
/// main thread is only 1 MiB, which is where the rustdoc doctest harness runs, so
/// the work is moved to a thread with a generous stack. Joining that thread also
/// contains an ordinary panic from an edge case (the join returns an error),
/// which becomes the unminified fallback.
#[cfg(all(feature = "minify", not(target_family = "wasm")))]
fn try_minify(source: &str) -> Result<String, Vec<String>> {
    std::thread::scope(|scope| {
        match std::thread::Builder::new()
            .stack_size(16 * 1024 * 1024)
            .spawn_scoped(scope, || minify_with_oxc(source))
        {
            Ok(handle) => handle
                .join()
                .unwrap_or_else(|_| Err(vec!["The minifier stopped unexpectedly.".to_owned()])),
            Err(error) => Err(vec![format!("The minifier could not start: {error}.")]),
        }
    })
}

/// Parse, minify and print `source` with oxc. Returns the parser's messages if
/// parsing reported errors, so only valid JavaScript is compacted.
#[cfg(all(feature = "minify", not(target_family = "wasm")))]
fn minify_with_oxc(source: &str) -> Result<String, Vec<String>> {
    use oxc::allocator::Allocator;
    use oxc::codegen::{Codegen, CodegenOptions};
    use oxc::minifier::{Minifier, MinifierOptions};
    use oxc::parser::Parser;
    use oxc::span::SourceType;

    let allocator = Allocator::default();
    // The generated client script is a classic script, not a module. This must
    // be SourceType::script(): under cjs (CommonJS) semantics a top-level var
    // is file-scoped, so the minifier is entitled to drop the unused
    // `var fod = new fiftyoneDegreesManager();` binding, and the page's global
    // `fod` object never exists. In script mode a top-level var is an
    // observable global and is kept.
    let source_type = SourceType::script();
    let parsed = Parser::new(&allocator, source, source_type).parse();
    if parsed.panicked || !parsed.diagnostics.is_empty() {
        let mut errors: Vec<String> = parsed
            .diagnostics
            .iter()
            .map(|diagnostic| format!("{diagnostic}."))
            .collect();
        if errors.is_empty() {
            errors.push("The parser stopped unexpectedly.".to_owned());
        }
        return Err(errors);
    }

    let mut program = parsed.program;
    let minified = Minifier::new(MinifierOptions::default()).minify(&allocator, &mut program);

    let printed = Codegen::new()
        .with_options(CodegenOptions {
            minify: true,
            ..CodegenOptions::default()
        })
        .with_scoping(minified.scoping)
        .build(&program);
    Ok(printed.code)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(all(feature = "minify", not(target_family = "wasm")))]
    #[test]
    fn minify_compacts_valid_javascript() {
        let content = "function   add ( a , b )  {\n  return a   +   b ;\n}\n".to_owned();
        let outcome = minify(content.clone());
        assert!(
            outcome.errors.is_empty(),
            "valid input minifies without error"
        );
        assert!(
            outcome.content.len() < content.len(),
            "minified output is smaller: {:?}",
            outcome.content
        );
        assert!(
            !outcome.content.contains("  "),
            "insignificant whitespace is removed: {:?}",
            outcome.content
        );
    }

    #[cfg(all(feature = "minify", not(target_family = "wasm")))]
    #[test]
    fn minify_keeps_an_unreferenced_top_level_var() {
        // The template's final statement is
        // `var fod = new fiftyoneDegreesManager();` and nothing else in the
        // file references `fod`: the binding exists for the page's own
        // scripts. Under module (cjs) parsing the minifier may drop it as
        // unused, which silently breaks every client-side integration, so the
        // script-mode global must survive minification.
        let content =
            "function manager() { this.complete = function () {}; }\nvar fod = new manager();\n"
                .to_owned();
        let outcome = minify(content);
        assert!(
            outcome.errors.is_empty(),
            "valid input minifies without error"
        );
        assert!(
            outcome.content.contains("fod"),
            "the top-level fod binding must survive minification: {:?}",
            outcome.content
        );
    }

    #[cfg(all(feature = "minify", not(target_family = "wasm")))]
    #[test]
    fn minify_falls_back_on_invalid_javascript() {
        // A parse error must not propagate. The original content is served and
        // the parser's message is returned.
        let content = "function (  {{ this is not valid ".to_owned();
        let outcome = minify(content.clone());
        assert!(!outcome.errors.is_empty(), "invalid input is flagged");
        assert!(
            outcome.errors.iter().all(|error| !error.is_empty()),
            "each error has a message: {:?}",
            outcome.errors
        );
        assert_eq!(outcome.content, content, "the original content is kept");
    }

    #[cfg(not(all(feature = "minify", not(target_family = "wasm"))))]
    #[test]
    fn minify_returns_content_unchanged_without_the_feature() {
        let content = "var  x  =  1 ;".to_owned();
        let outcome = minify(content.clone());
        assert!(outcome.errors.is_empty());
        assert_eq!(outcome.content, content);
    }
}
