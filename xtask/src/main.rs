//! `cargo xtask scan` — a `syn` AST scanner for the four review-enforced bans
//! from `CLAUDE.md` Iron Law 3 that NO clippy lint covers:
//!
//! 1. `.unwrap_or(..)` / `.unwrap_or_default()` on a lookup-shaped receiver
//!    (`.get`, `.first`, `.last`, `.find`), including through a chain of
//!    Option-preserving adaptors — `.get(&k).copied().unwrap_or(..)`,
//!    `.first().cloned().unwrap_or_default()`, `.find(..).map(..).unwrap_or(..)`
//!    — the fix is a total structure or an enum variant, never a default at the
//!    call site.
//! 2. Inline `#[expect(..)]` — silences a restriction lint without tripping
//!    `allow_attributes`; the fix belongs in the lint-config layer.
//! 3. `#[non_exhaustive]` on any `enum` — re-opens the wildcard hole the
//!    exhaustiveness proof closes.
//! 4. A fabricated `Default` — `Default::default()` / `T::default()` /
//!    `..Default::default()` or `#[derive(Default)]` on a domain type — absence
//!    is a variant, not a default.
//!
//! The scanner is a dev tool, exempt from mu-core's Iron-Law lints. It is a
//! NAME-BASED mechanical pre-flight, not a replacement for review: it works from
//! method/attribute names, and `#[cfg(test)]` modules are skipped (the bans
//! target production core; clippy already exempts tests). Ban #1 catches the std
//! lookup-name chains above (`get`/`first`/`last`/`find`, including the peeled
//! adaptors), but a name-based scanner cannot see a DOMAIN-ACCESSOR lookup
//! (e.g. `atlas.monster(n).unwrap_or(..)`) — those stay review-authoritative.
//! Bans #2–#4 are name/shape-exact, so the scanner is authoritative for them. It
//! exits non-zero with `file:line` on any hit so a pre-commit hook and CI can
//! gate on the std forms mechanically.

use std::io::Write;
use std::path::Path;
use std::process::ExitCode;

use proc_macro2::Span;
use syn::spanned::Spanned;
use syn::visit::{self, Visit};
use walkdir::WalkDir;

/// Writes a line to stdout. `writeln!` (not the banned `println!` macro) keeps
/// this dev tool inside the workspace `clippy.toml` `disallowed-macros` policy —
/// core bans logging macros crate-wide, and that config is global to the clippy
/// run even though xtask does not inherit core's Iron-Law *lints*.
fn out(line: &str) {
    let _ = writeln!(std::io::stdout(), "{line}");
}

/// Writes a line to stderr, same rationale as [`out`].
fn err(line: &str) {
    let _ = writeln!(std::io::stderr(), "{line}");
}

/// Receiver method names that make an expression lookup-shaped: a query that
/// returns `Option<T>` because the key/element may be absent.
const LOOKUP_METHODS: [&str; 4] = ["get", "first", "last", "find"];

/// The absence-swallowing combinators the scanner rejects on a lookup receiver.
const UNWRAP_OR: [&str; 2] = ["unwrap_or", "unwrap_or_default"];

/// Option-preserving pass-through combinators. Each carries an `Option<T>`
/// through unchanged — mapping, filtering, or defaulting the payload without
/// collapsing the absence — so a lookup hidden behind any chain of them still
/// feeds the terminal `unwrap_or`. In this Copy-newtype codebase
/// `.get(&k).copied()` and `.first().cloned()` are the idiomatic forms, so the
/// scanner peels these off the receiver chain to reach the real lookup beneath.
const OPTION_ADAPTORS: [&str; 10] = [
    "copied", "cloned", "map", "as_ref", "as_deref", "and_then", "filter", "or", "or_else",
    "inspect",
];

/// Curated exemptions for the fabricated-`Default` rule, keyed by repo-relative
/// source path. EMPTY today — the tree carries no legitimate `Default` use, so
/// nothing needs exempting. A genuinely-legitimate future use is added here (a
/// deliberate, reviewed decision) rather than silenced with an inline
/// suppressor, keeping the scanner zero-false-positive by construction.
const DEFAULT_ALLOWLIST: &[&str] = &[];

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    match args.next().as_deref() {
        Some("scan") => scan(),
        other => {
            err(&format!("usage: cargo xtask scan  (got {other:?})"));
            ExitCode::from(2)
        }
    }
}

/// One flagged occurrence of a banned pattern.
struct Violation {
    file: String,
    line: usize,
    rule: &'static str,
    detail: String,
}

fn scan() -> ExitCode {
    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("xtask is a workspace member, so it has a parent directory")
        .to_path_buf();
    let core_src = repo_root.join("core").join("src");

    let mut violations = Vec::new();
    for entry in WalkDir::new(&core_src).sort_by_file_name() {
        let entry = entry.expect("walk core/src");
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("rs") {
            continue;
        }
        let display = path
            .strip_prefix(&repo_root)
            .unwrap_or(path)
            .to_string_lossy()
            .replace('\\', "/");
        let source = std::fs::read_to_string(path).expect("read a core source file");
        let ast = match syn::parse_file(&source) {
            Ok(ast) => ast,
            Err(parse_err) => {
                err(&format!("{display}: could not parse: {parse_err}"));
                return ExitCode::from(2);
            }
        };
        let mut scanner = Scanner {
            file: &display,
            violations: &mut violations,
        };
        scanner.visit_file(&ast);
    }

    if violations.is_empty() {
        out("cargo xtask scan: clean — no review-enforced ban violations in core/src");
        return ExitCode::SUCCESS;
    }

    for violation in &violations {
        out(&format!(
            "{}:{}: [{}] {}",
            violation.file, violation.line, violation.rule, violation.detail
        ));
    }
    err(&format!(
        "cargo xtask scan: {} review-enforced ban violation(s) — fix at the root, not the call site",
        violations.len()
    ));
    ExitCode::FAILURE
}

struct Scanner<'a> {
    file: &'a str,
    violations: &'a mut Vec<Violation>,
}

impl Scanner<'_> {
    fn record(&mut self, span: Span, rule: &'static str, detail: String) {
        self.violations.push(Violation {
            file: self.file.to_string(),
            line: span.start().line,
            rule,
            detail,
        });
    }

    fn default_allowed(&self) -> bool {
        DEFAULT_ALLOWLIST.contains(&self.file)
    }
}

/// Descends a receiver chain, skipping [`OPTION_ADAPTORS`], to the first method
/// call that is not a pass-through adaptor — the terminal lookup candidate whose
/// absence a trailing `unwrap_or` would swallow. `None` when the chain bottoms
/// out on a non-method-call (a variable, field, index, or free call).
fn terminal_lookup_call(expr: &syn::Expr) -> Option<&syn::ExprMethodCall> {
    let syn::Expr::MethodCall(call) = expr else {
        return None;
    };
    if OPTION_ADAPTORS.contains(&call.method.to_string().as_str()) {
        return terminal_lookup_call(call.receiver.as_ref());
    }
    Some(call)
}

/// Whether an attribute is `#[cfg(test)]`. Test modules are skipped: the
/// review-enforced bans target production core, and `clippy.toml` already
/// exempts tests, so scanning them would only diverge from that policy.
fn is_cfg_test(attr: &syn::Attribute) -> bool {
    if !attr.path().is_ident("cfg") {
        return false;
    }
    let mut names_test = false;
    let _ = attr.parse_nested_meta(|meta| {
        if meta.path.is_ident("test") {
            names_test = true;
        }
        Ok(())
    });
    names_test
}

/// Whether a call path names `default` as a qualified associated function —
/// `Default::default`, `T::default`, or `<T as Default>::default` — as opposed
/// to a bare free function coincidentally named `default`.
fn is_qualified_default(path: &syn::Path, has_qself: bool) -> bool {
    match path.segments.last() {
        Some(last) if last.ident == "default" => has_qself || path.segments.len() >= 2,
        Some(_) | None => false,
    }
}

impl<'ast> Visit<'ast> for Scanner<'_> {
    fn visit_expr_method_call(&mut self, node: &'ast syn::ExprMethodCall) {
        if UNWRAP_OR.contains(&node.method.to_string().as_str()) {
            if let Some(terminal) = terminal_lookup_call(node.receiver.as_ref()) {
                let inner = terminal.method.to_string();
                if LOOKUP_METHODS.contains(&inner.as_str()) {
                    self.record(
                        node.method.span(),
                        "lookup-unwrap-or",
                        format!(
                            ".{inner}(..).{}(..) swallows a lookup's absence — return a total \
                             structure (lookup yields T, not Option<T>) or fold absence into an enum",
                            node.method
                        ),
                    );
                }
            }
        }
        visit::visit_expr_method_call(self, node);
    }

    fn visit_item_mod(&mut self, node: &'ast syn::ItemMod) {
        if node.attrs.iter().any(is_cfg_test) {
            return;
        }
        visit::visit_item_mod(self, node);
    }

    fn visit_expr_call(&mut self, node: &'ast syn::ExprCall) {
        if let syn::Expr::Path(path) = node.func.as_ref() {
            if is_qualified_default(&path.path, path.qself.is_some()) && !self.default_allowed() {
                self.record(
                    node.func.span(),
                    "fabricated-default",
                    "fabricated Default::default()/T::default() to satisfy a signature — absence \
                     is a variant, not a default"
                        .to_string(),
                );
            }
        }
        visit::visit_expr_call(self, node);
    }

    fn visit_attribute(&mut self, node: &'ast syn::Attribute) {
        if node.path().is_ident("expect") {
            self.record(
                node.span(),
                "inline-expect",
                "inline #[expect(..)] silences a restriction lint at the call site — change the \
                 lint-config layer (workspace lints / clippy.toml) instead"
                    .to_string(),
            );
        }
        if node.path().is_ident("derive") {
            let mut derives_default = false;
            let _ = node.parse_nested_meta(|meta| {
                if meta.path.is_ident("Default") {
                    derives_default = true;
                }
                Ok(())
            });
            if derives_default && !self.default_allowed() {
                self.record(
                    node.span(),
                    "derive-default",
                    "#[derive(Default)] on a domain type fabricates a zeroed value — model absence \
                     as an enum variant instead"
                        .to_string(),
                );
            }
        }
        visit::visit_attribute(self, node);
    }

    fn visit_item_enum(&mut self, node: &'ast syn::ItemEnum) {
        for attr in &node.attrs {
            if attr.path().is_ident("non_exhaustive") {
                self.record(
                    attr.span(),
                    "non-exhaustive-enum",
                    format!(
                        "#[non_exhaustive] on enum {} re-opens the wildcard hole for every \
                         consumer — keep the enum exhaustive",
                        node.ident
                    ),
                );
            }
        }
        visit::visit_item_enum(self, node);
    }
}

/// Runs the scanner over an in-memory source string — the seam the self-check
/// tests drive so they never touch the filesystem.
#[cfg(test)]
fn scan_source(source: &str) -> Vec<Violation> {
    let ast = syn::parse_file(source).expect("test fixture parses");
    let mut violations = Vec::new();
    let mut scanner = Scanner {
        file: "<fixture>",
        violations: &mut violations,
    };
    scanner.visit_file(&ast);
    violations
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The rule tags flagged in `source`, in discovery order.
    fn rules(source: &str) -> Vec<&'static str> {
        scan_source(source).into_iter().map(|v| v.rule).collect()
    }

    #[test]
    fn flags_get_copied_unwrap_or() {
        let src = "fn f(m: &std::collections::BTreeMap<u8, u8>) { let _ = m.get(&1).copied().unwrap_or(0); }";
        assert_eq!(rules(src), vec!["lookup-unwrap-or"]);
    }

    #[test]
    fn flags_first_cloned_unwrap_or_default() {
        let src = "fn f(v: &[u8]) { let _ = v.first().cloned().unwrap_or_default(); }";
        assert_eq!(rules(src), vec!["lookup-unwrap-or"]);
    }

    #[test]
    fn flags_find_map_unwrap_or() {
        let src =
            "fn f(v: &[u8]) { let _ = v.iter().find(|&&x| x == 1).map(|&x| x).unwrap_or(0); }";
        assert_eq!(rules(src), vec!["lookup-unwrap-or"]);
    }

    #[test]
    fn still_flags_a_direct_lookup_unwrap_or() {
        let src =
            "fn f(m: &std::collections::BTreeMap<u8, u8>) { let _ = m.get(&1).unwrap_or(&0); }";
        assert_eq!(rules(src), vec!["lookup-unwrap-or"]);
    }

    #[test]
    fn ignores_unwrap_or_on_a_non_lookup_terminal() {
        let src = "fn f(v: &[u8]) { let _ = v.iter().map(|&x| x).max().unwrap_or(0); }";
        assert!(rules(src).is_empty());
    }

    #[test]
    fn skips_cfg_test_modules() {
        let src = "#[cfg(test)] mod tests { fn g(m: &std::collections::BTreeMap<u8, u8>) { let _ = m.get(&1).copied().unwrap_or(0); } }";
        assert!(rules(src).is_empty());
    }

    #[test]
    fn scans_non_test_modules() {
        let src = "mod inner { pub fn g(m: &std::collections::BTreeMap<u8, u8>) { let _ = m.get(&1).copied().unwrap_or(0); } }";
        assert_eq!(rules(src), vec!["lookup-unwrap-or"]);
    }
}
