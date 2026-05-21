//! Deterministic git + syn engine backing the traceability parity pass
//! (CMP-TRACE-ENGINE, SEQ-TRACE-REVIEW) and the freshness check
//! (SEQ-MAP-STATUS). Pure: it extracts a Rust symbol index, asks git for
//! the lines changed since a base revision, and maps those lines back to
//! enclosing symbols. It authors no models and makes no policy decisions —
//! the verdict (forward/reverse buckets) lives in `Project`.

use std::collections::BTreeMap;
use std::path::Path;
use std::process::Command;

use anyhow::Result;
use syn::spanned::Spanned;

/// DOM-SYMBOL-SPAN: a code symbol located by syn, with its 1-based,
/// inclusive line span. `symbol` is a dotted-by-`::` path: a bare name
/// for a free item (`apply_theme`, `RenderTree`) or `Type::method` for an
/// impl method (`Project::review_traceability`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SymbolSpan {
    pub path: String,
    pub symbol: String,
    pub line_start: usize,
    pub line_end: usize,
    pub kind: String,
}

/// Behaviour-bearing kinds. A changed symbol of one of these kinds with no
/// source_link is a blocking reverse violation (bucket B); everything else
/// (const/static/glue/test) only warns (bucket A).
pub fn is_behaviour_kind(kind: &str) -> bool {
    matches!(kind, "fn" | "struct" | "enum" | "trait" | "type")
}

/// True when `link_symbol` (as written in a trace.yml source_link) refers to
/// the same thing as `span_symbol` (as produced by [`extract_symbols`]).
/// Tolerates the two forms source_links use in this repo — a bare leaf name
/// and a `Type::method` path — in either direction.
pub fn symbol_matches(span_symbol: &str, link_symbol: &str) -> bool {
    span_symbol == link_symbol
        || span_symbol.ends_with(&format!("::{link_symbol}"))
        || link_symbol.ends_with(&format!("::{span_symbol}"))
}

/// Parse Rust `source` and return its non-test symbol index. Test items
/// (`#[test]` fns and `#[cfg(test)]` modules) are skipped so editing tests
/// never trips the reverse pass. Unparseable input yields an empty index.
pub fn extract_symbols(path: &str, source: &str) -> Vec<SymbolSpan> {
    let mut out = Vec::new();
    if let Ok(file) = syn::parse_file(source) {
        collect_items(path, &file.items, false, &mut out);
    }
    out
}

fn collect_items(path: &str, items: &[syn::Item], in_test: bool, out: &mut Vec<SymbolSpan>) {
    for item in items {
        match item {
            syn::Item::Fn(f) => {
                if in_test || f.attrs.iter().any(is_test_attr) {
                    continue;
                }
                push(out, path, f.sig.ident.to_string(), "fn", f.span());
            }
            syn::Item::Struct(s) if !in_test => {
                push(out, path, s.ident.to_string(), "struct", s.span());
            }
            syn::Item::Enum(e) if !in_test => {
                push(out, path, e.ident.to_string(), "enum", e.span());
                // Index variants as `Enum::Variant` so a source_link may
                // point at a specific variant (e.g. `Commands::Render`).
                for variant in &e.variants {
                    push(
                        out,
                        path,
                        format!("{}::{}", e.ident, variant.ident),
                        "variant",
                        variant.span(),
                    );
                }
            }
            syn::Item::Trait(t) if !in_test => {
                push(out, path, t.ident.to_string(), "trait", t.span());
            }
            syn::Item::Type(t) if !in_test => {
                push(out, path, t.ident.to_string(), "type", t.span());
            }
            syn::Item::Const(c) if !in_test => {
                push(out, path, c.ident.to_string(), "const", c.span());
            }
            syn::Item::Static(s) if !in_test => {
                push(out, path, s.ident.to_string(), "static", s.span());
            }
            syn::Item::Mod(m) => {
                let test = in_test || m.attrs.iter().any(is_test_attr);
                if let Some((_, inner)) = &m.content {
                    collect_items(path, inner, test, out);
                }
            }
            syn::Item::Impl(i) => {
                if in_test || i.attrs.iter().any(is_test_attr) {
                    continue;
                }
                let ty = impl_type_name(&i.self_ty);
                for impl_item in &i.items {
                    if let syn::ImplItem::Fn(m) = impl_item {
                        if m.attrs.iter().any(is_test_attr) {
                            continue;
                        }
                        let symbol = match &ty {
                            Some(t) => format!("{t}::{}", m.sig.ident),
                            None => m.sig.ident.to_string(),
                        };
                        push(out, path, symbol, "fn", m.span());
                    }
                }
            }
            _ => {}
        }
    }
}

fn push(out: &mut Vec<SymbolSpan>, path: &str, symbol: String, kind: &str, span: proc_macro2::Span) {
    out.push(SymbolSpan {
        path: path.to_string(),
        symbol,
        line_start: span.start().line,
        line_end: span.end().line,
        kind: kind.to_string(),
    });
}

fn impl_type_name(ty: &syn::Type) -> Option<String> {
    if let syn::Type::Path(tp) = ty {
        tp.path.segments.last().map(|s| s.ident.to_string())
    } else {
        None
    }
}

fn is_test_attr(attr: &syn::Attribute) -> bool {
    match &attr.meta {
        syn::Meta::Path(p) => p.is_ident("test"),
        syn::Meta::List(l) => l.path.is_ident("cfg") && l.tokens.to_string().contains("test"),
        _ => false,
    }
}

/// A changed line that did not land inside any recorded (non-test) symbol —
/// module-level glue: an import, attribute, comment, or test body. Always a
/// bucket-A warning, never blocking.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GlueChange {
    pub path: String,
    pub line: usize,
    pub label: String,
}

/// The code that changed between a base revision and the working tree,
/// resolved against the syn symbol index.
#[derive(Debug, Clone, Default)]
pub struct ChangedCode {
    /// Recorded (non-test) symbols with at least one changed line.
    pub symbols: Vec<SymbolSpan>,
    /// Changed lines outside any recorded symbol.
    pub glue: Vec<GlueChange>,
}

/// Lines changed (added/modified in the new version) per `.rs` file under a
/// `*/src/` directory, between `base` and the working tree, via
/// `git diff --unified=0`. Maps repo-relative path -> inclusive new-line
/// ranges. Deleted files and pure deletions contribute nothing (there is no
/// current symbol to attribute them to).
pub fn git_changed_lines(root: &Path, base: &str) -> Result<BTreeMap<String, Vec<(usize, usize)>>> {
    let output = match Command::new("git")
        .arg("-C")
        .arg(root)
        .arg("diff")
        .arg("--unified=0")
        .arg("--no-color")
        .arg(base)
        .output()
    {
        Ok(output) => output,
        // git not installed / not on PATH: degrade to "no changeset seen"
        // rather than crashing review in a non-git context.
        Err(_) => return Ok(BTreeMap::new()),
    };
    if !output.status.success() {
        // Not a git repo, or `base` is unknown (e.g. an exported tarball or a
        // test tempdir). Treat as no detectable change instead of an error,
        // so the traceability pass never hard-fails review() outside a cycle.
        return Ok(BTreeMap::new());
    }
    let diff = String::from_utf8_lossy(&output.stdout);
    Ok(parse_unified_diff(&diff))
}

/// Parse `git diff --unified=0` text into path -> changed new-line ranges,
/// keeping only `.rs` files under a `*/src/` directory.
pub fn parse_unified_diff(diff: &str) -> BTreeMap<String, Vec<(usize, usize)>> {
    let mut result: BTreeMap<String, Vec<(usize, usize)>> = BTreeMap::new();
    let mut current: Option<String> = None;
    for line in diff.lines() {
        if let Some(rest) = line.strip_prefix("+++ ") {
            current = match rest.strip_prefix("b/") {
                Some(path) if path.ends_with(".rs") && path.contains("/src/") => {
                    Some(path.to_string())
                }
                _ => None, // /dev/null (deletion) or out-of-scope file
            };
            continue;
        }
        if let (Some(path), Some(hunk)) = (current.as_ref(), line.strip_prefix("@@ ")) {
            if let Some((start, count)) = parse_hunk_new_range(hunk) {
                if count > 0 {
                    result
                        .entry(path.clone())
                        .or_default()
                        .push((start, start + count - 1));
                }
            }
        }
    }
    result
}

/// From a hunk header body like `-12,3 +40,2 @@ fn foo` extract the new-file
/// `(start, count)`; an omitted count means 1.
fn parse_hunk_new_range(hunk: &str) -> Option<(usize, usize)> {
    let plus = hunk.split_whitespace().find(|tok| tok.starts_with('+'))?;
    let spec = plus.trim_start_matches('+');
    let mut parts = spec.split(',');
    let start: usize = parts.next()?.parse().ok()?;
    let count: usize = match parts.next() {
        Some(c) => c.parse().ok()?,
        None => 1,
    };
    Some((start, count))
}

/// Resolve the lines changed since `base` into changed symbols + glue.
/// Reads each changed file from the working tree (current state) so the
/// symbol index reflects the code as it is now.
pub fn changed_code(root: &Path, base: &str) -> Result<ChangedCode> {
    let by_file = git_changed_lines(root, base)?;
    let mut changed = ChangedCode::default();

    for (path, ranges) in by_file {
        let abs = root.join(&path);
        let Ok(source) = std::fs::read_to_string(&abs) else {
            continue; // deleted or unreadable in the working tree
        };
        let symbols = extract_symbols(&path, &source);
        let lines: Vec<&str> = source.lines().collect();

        let mut hit: Vec<bool> = vec![false; symbols.len()];
        for (start, end) in ranges {
            for line in start..=end {
                match innermost_symbol(&symbols, line) {
                    Some(idx) => hit[idx] = true,
                    None => changed.glue.push(GlueChange {
                        path: path.clone(),
                        line,
                        label: glue_label(lines.get(line - 1).copied().unwrap_or("")),
                    }),
                }
            }
        }
        for (idx, was_hit) in hit.into_iter().enumerate() {
            if was_hit {
                changed.symbols.push(symbols[idx].clone());
            }
        }
    }
    Ok(changed)
}

/// Index of the smallest symbol span containing `line`, if any.
fn innermost_symbol(symbols: &[SymbolSpan], line: usize) -> Option<usize> {
    symbols
        .iter()
        .enumerate()
        .filter(|(_, s)| s.line_start <= line && line <= s.line_end)
        .min_by_key(|(_, s)| s.line_end - s.line_start)
        .map(|(idx, _)| idx)
}

/// Coarse classification of a glue line for the bucket-A report.
fn glue_label(text: &str) -> String {
    let t = text.trim_start();
    let label = if t.starts_with("use ") || t.starts_with("pub use ") || t.starts_with("extern crate") {
        "import"
    } else if t.starts_with("//") || t.starts_with("/*") || t.starts_with('*') {
        "comment"
    } else if t.starts_with("#[") || t.starts_with("#![") {
        "attribute"
    } else if t.starts_with("mod ") || t.starts_with("pub mod ") {
        "module"
    } else if t.is_empty() {
        "blank"
    } else {
        "glue"
    };
    label.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_free_fn_struct_and_impl_method() {
        let src = "struct Foo;\n\nfn bare() {}\n\nimpl Foo {\n    fn method(&self) {}\n}\n";
        let syms = extract_symbols("crates/x/src/lib.rs", src);
        assert!(syms.iter().any(|s| s.symbol == "Foo" && s.kind == "struct"));
        assert!(syms.iter().any(|s| s.symbol == "bare" && s.kind == "fn"));
        assert!(syms.iter().any(|s| s.symbol == "Foo::method" && s.kind == "fn"));
    }

    #[test]
    fn skips_test_items() {
        let src = "#[test]\nfn t() {}\n\n#[cfg(test)]\nmod tests {\n    fn helper() {}\n}\n";
        let syms = extract_symbols("crates/x/src/lib.rs", src);
        assert!(syms.is_empty(), "test items must not be indexed: {syms:?}");
    }

    #[test]
    fn symbol_matching_tolerates_both_forms() {
        assert!(symbol_matches("Project::review", "review"));
        assert!(symbol_matches("review", "Project::review"));
        assert!(symbol_matches("apply_theme", "apply_theme"));
        assert!(!symbol_matches("apply_theme", "register_fonts"));
    }

    #[test]
    fn parses_unified_diff_new_ranges() {
        let diff = "diff --git a/crates/x/src/lib.rs b/crates/x/src/lib.rs\n--- a/crates/x/src/lib.rs\n+++ b/crates/x/src/lib.rs\n@@ -10,0 +11,2 @@ fn foo\n+a\n+b\n@@ -20 +22 @@\n+c\n";
        let map = parse_unified_diff(diff);
        let ranges = map.get("crates/x/src/lib.rs").unwrap();
        assert_eq!(ranges, &vec![(11, 12), (22, 22)]);
    }

    #[test]
    fn unified_diff_ignores_non_src_and_non_rs() {
        let diff = "+++ b/crates/x/examples/e.rs\n@@ -0,0 +1 @@\n+x\n+++ b/README.md\n@@ -0,0 +1 @@\n+y\n";
        assert!(parse_unified_diff(diff).is_empty());
    }
}
