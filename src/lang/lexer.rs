//! Hand-written trivia lexer for C-family and markup source.

use super::{CommentSyntax, LangProfile, RawStringStyle};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpanKind {
    Code,
    String,
    LineComment,
    BlockComment,
    Whitespace,
    Eol,
    /// Region skipped due to ambiguity (e.g. JS regex).
    Skipped,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Span {
    pub kind: SpanKind,
    pub start: usize,
    pub end: usize,
}

impl Span {
    pub fn text<'a>(&self, src: &'a str) -> &'a str {
        &src[self.start..self.end]
    }

    pub fn is_comment(&self) -> bool {
        matches!(self.kind, SpanKind::LineComment | SpanKind::BlockComment)
    }

    pub fn is_trivia(&self) -> bool {
        matches!(
            self.kind,
            SpanKind::LineComment
                | SpanKind::BlockComment
                | SpanKind::Whitespace
                | SpanKind::Eol
                | SpanKind::Skipped
        )
    }
}

/// Lex source into classified spans. Always covers the full byte range [0, src.len()).
pub fn lex(src: &str, profile: &LangProfile) -> Vec<Span> {
    match profile.comment_syntax {
        CommentSyntax::Markup => super::markup::lex_markup(src, profile),
        CommentSyntax::CFamily => lex_c_family(src, profile),
    }
}

fn lex_c_family(src: &str, profile: &LangProfile) -> Vec<Span> {
    let bytes = src.as_bytes();
    let mut spans = Vec::new();
    let mut i = 0usize;
    let mut code_start: Option<usize> = None;

    let flush_code = |spans: &mut Vec<Span>, code_start: &mut Option<usize>, end: usize| {
        if let Some(s) = code_start.take() {
            if s < end {
                spans.push(Span {
                    kind: SpanKind::Code,
                    start: s,
                    end,
                });
            }
        }
    };

    while i < bytes.len() {
        // EOL
        if bytes[i] == b'\n' {
            flush_code(&mut spans, &mut code_start, i);
            spans.push(Span {
                kind: SpanKind::Eol,
                start: i,
                end: i + 1,
            });
            i += 1;
            continue;
        }
        if bytes[i] == b'\r' {
            flush_code(&mut spans, &mut code_start, i);
            let end = if i + 1 < bytes.len() && bytes[i + 1] == b'\n' {
                i + 2
            } else {
                i + 1
            };
            spans.push(Span {
                kind: SpanKind::Eol,
                start: i,
                end,
            });
            i = end;
            continue;
        }

        // Whitespace (spaces/tabs only — not newlines)
        if bytes[i] == b' ' || bytes[i] == b'\t' {
            flush_code(&mut spans, &mut code_start, i);
            let start = i;
            while i < bytes.len() && (bytes[i] == b' ' || bytes[i] == b'\t') {
                i += 1;
            }
            spans.push(Span {
                kind: SpanKind::Whitespace,
                start,
                end: i,
            });
            continue;
        }

        // Line comment //
        if profile.line_comment && i + 1 < bytes.len() && bytes[i] == b'/' && bytes[i + 1] == b'/' {
            flush_code(&mut spans, &mut code_start, i);
            let start = i;
            i += 2;
            // Rust doc comments /// and //! are still line comments
            while i < bytes.len() && bytes[i] != b'\n' && bytes[i] != b'\r' {
                // C/C++ backslash-newline continuation
                if profile.backslash_continue
                    && bytes[i] == b'\\'
                    && i + 1 < bytes.len()
                    && (bytes[i + 1] == b'\n'
                        || (bytes[i + 1] == b'\r'
                            && i + 2 < bytes.len()
                            && bytes[i + 2] == b'\n'))
                {
                    i += 1;
                    if bytes[i] == b'\r' {
                        i += 2;
                    } else {
                        i += 1;
                    }
                    continue;
                }
                i += 1;
            }
            spans.push(Span {
                kind: SpanKind::LineComment,
                start,
                end: i,
            });
            continue;
        }

        // Block comment /* */
        if profile.block_comment && i + 1 < bytes.len() && bytes[i] == b'/' && bytes[i + 1] == b'*'
        {
            flush_code(&mut spans, &mut code_start, i);
            let start = i;
            i += 2;
            let mut depth = 1i32;
            while i + 1 < bytes.len() {
                if profile.nested_block && bytes[i] == b'/' && bytes[i + 1] == b'*' {
                    depth += 1;
                    i += 2;
                    continue;
                }
                if bytes[i] == b'*' && bytes[i + 1] == b'/' {
                    depth -= 1;
                    i += 2;
                    if depth == 0 {
                        break;
                    }
                    continue;
                }
                i += 1;
            }
            // unterminated: consume rest
            if depth != 0 {
                i = bytes.len();
            }
            spans.push(Span {
                kind: SpanKind::BlockComment,
                start,
                end: i,
            });
            continue;
        }

        // Strings
        if let Some(end) = try_string(bytes, i, profile) {
            flush_code(&mut spans, &mut code_start, i);
            spans.push(Span {
                kind: SpanKind::String,
                start: i,
                end,
            });
            i = end;
            continue;
        }

        // JS regex ambiguity: if we see `/` that isn't a comment and we're in JS/TS,
        // skip conservatively until we can re-sync (newline or `;` or `{` etc.)
        if matches!(
            profile.lang,
            super::Lang::JavaScript | super::Lang::TypeScript
        ) && bytes[i] == b'/'
            && (i + 1 >= bytes.len() || (bytes[i + 1] != b'/' && bytes[i + 1] != b'*'))
        {
            // Could be division or regex. Skip the `/.../` region heuristically
            // by treating until end of line or next clear delimiter as Skipped
            // only if it looks like a regex (letter after / or [).
            if i + 1 < bytes.len()
                && (bytes[i + 1].is_ascii_alphabetic()
                    || bytes[i + 1] == b'['
                    || bytes[i + 1] == b'\\'
                    || bytes[i + 1] == b'^'
                    || bytes[i + 1] == b'$'
                    || bytes[i + 1] == b'.'
                    || bytes[i + 1] == b'(')
            {
                flush_code(&mut spans, &mut code_start, i);
                let start = i;
                i += 1;
                while i < bytes.len() && bytes[i] != b'\n' && bytes[i] != b'\r' {
                    if bytes[i] == b'\\' && i + 1 < bytes.len() {
                        i += 2;
                        continue;
                    }
                    if bytes[i] == b'/' {
                        i += 1;
                        // flags
                        while i < bytes.len() && bytes[i].is_ascii_alphabetic() {
                            i += 1;
                        }
                        break;
                    }
                    i += 1;
                }
                spans.push(Span {
                    kind: SpanKind::Skipped,
                    start,
                    end: i,
                });
                continue;
            }
        }

        // Ordinary code byte
        if code_start.is_none() {
            code_start = Some(i);
        }
        i += 1;
    }
    flush_code(&mut spans, &mut code_start, bytes.len());
    spans
}

fn try_string(bytes: &[u8], i: usize, profile: &LangProfile) -> Option<usize> {
    // Rust raw string: r#"..."# or r##"..."## etc, also br"...", cr"..."
    if profile.raw_strings == RawStringStyle::Rust {
        if let Some(end) = try_rust_raw(bytes, i) {
            return Some(end);
        }
    }
    // C++ raw string: R"delim(...)delim"
    if profile.raw_strings == RawStringStyle::Cpp {
        if let Some(end) = try_cpp_raw(bytes, i) {
            return Some(end);
        }
    }
    // Go backtick string
    if profile.backtick_strings && bytes[i] == b'`' {
        let mut j = i + 1;
        while j < bytes.len() {
            if bytes[j] == b'`' {
                return Some(j + 1);
            }
            j += 1;
        }
        return Some(bytes.len());
    }
    // JS/TS template literal
    if profile.template_literals && bytes[i] == b'`' {
        return Some(scan_template(bytes, i));
    }
    // Normal quoted strings
    let quote = bytes[i];
    if quote != b'"' && quote != b'\'' {
        return None;
    }
    // Rust byte/c strings: b"..." c"..."
    // already handled if we start at quote; prefixes are code then quote.

    let mut j = i + 1;
    while j < bytes.len() {
        if bytes[j] == b'\\' {
            j += 2;
            continue;
        }
        if bytes[j] == quote {
            return Some(j + 1);
        }
        // C/C++ may allow escaped newlines in strings
        if bytes[j] == b'\n' && !profile.backslash_continue {
            // unclosed string ending at newline — still treat as string for safety
            return Some(j);
        }
        j += 1;
    }
    Some(bytes.len())
}

fn try_rust_raw(bytes: &[u8], i: usize) -> Option<usize> {
    // Optional b or c prefix
    let mut start = i;
    if start < bytes.len() && (bytes[start] == b'b' || bytes[start] == b'c') {
        start += 1;
    }
    if start >= bytes.len() || bytes[start] != b'r' {
        return None;
    }
    let mut j = start + 1;
    let mut hashes = 0usize;
    while j < bytes.len() && bytes[j] == b'#' {
        hashes += 1;
        j += 1;
    }
    if j >= bytes.len() || bytes[j] != b'"' {
        return None;
    }
    j += 1; // after opening quote
    let closing_hashes = hashes;
    'outer: while j < bytes.len() {
        if bytes[j] == b'"' {
            let mut k = 0;
            while k < closing_hashes && j + 1 + k < bytes.len() && bytes[j + 1 + k] == b'#' {
                k += 1;
            }
            if k == closing_hashes {
                return Some(j + 1 + closing_hashes);
            }
        }
        j += 1;
        if j >= bytes.len() {
            break 'outer;
        }
    }
    Some(bytes.len())
}

fn try_cpp_raw(bytes: &[u8], i: usize) -> Option<usize> {
    // R"delim(...)delim"  also LR"..." UR"..." u8R"..."
    let mut start = i;
    // optional encoding prefix
    if start + 1 < bytes.len() && bytes[start] == b'u' && bytes[start + 1] == b'8' {
        start += 2;
    } else if start < bytes.len() && matches!(bytes[start], b'L' | b'U' | b'u') {
        start += 1;
    }
    if start >= bytes.len() || bytes[start] != b'R' {
        return None;
    }
    if start + 1 >= bytes.len() || bytes[start + 1] != b'"' {
        return None;
    }
    let mut j = start + 2;
    let delim_start = j;
    while j < bytes.len() && bytes[j] != b'(' {
        if bytes[j] == b' ' || bytes[j] == b'\\' || bytes[j] == b')' {
            return None;
        }
        j += 1;
        if j - delim_start > 16 {
            return None;
        }
    }
    if j >= bytes.len() {
        return None;
    }
    let delim = &bytes[delim_start..j];
    j += 1; // skip '('
    while j < bytes.len() {
        if bytes[j] == b')' {
            let after = j + 1;
            if after + delim.len() < bytes.len()
                && &bytes[after..after + delim.len()] == delim
                && bytes[after + delim.len()] == b'"'
            {
                return Some(after + delim.len() + 1);
            }
        }
        j += 1;
    }
    Some(bytes.len())
}

fn scan_template(bytes: &[u8], i: usize) -> usize {
    // Handle nested ${ ... } with recursive template awareness
    let mut j = i + 1;
    let mut depth = 0i32; // brace depth inside ${}
    while j < bytes.len() {
        if depth == 0 {
            if bytes[j] == b'`' {
                return j + 1;
            }
            if bytes[j] == b'\\' {
                j += 2;
                continue;
            }
            if bytes[j] == b'$' && j + 1 < bytes.len() && bytes[j + 1] == b'{' {
                depth = 1;
                j += 2;
                continue;
            }
            j += 1;
        } else {
            // inside ${}
            if bytes[j] == b'`' {
                // nested template
                j = scan_template(bytes, j);
                continue;
            }
            if bytes[j] == b'\'' || bytes[j] == b'"' {
                let q = bytes[j];
                j += 1;
                while j < bytes.len() {
                    if bytes[j] == b'\\' {
                        j += 2;
                        continue;
                    }
                    if bytes[j] == q {
                        j += 1;
                        break;
                    }
                    j += 1;
                }
                continue;
            }
            if bytes[j] == b'{' {
                depth += 1;
            } else if bytes[j] == b'}' {
                depth -= 1;
            }
            j += 1;
        }
    }
    bytes.len()
}

/// Extract non-trivia token bytes for invariance checking.
pub fn non_trivia_bytes(src: &str, spans: &[Span]) -> Vec<u8> {
    let mut out = Vec::new();
    for sp in spans {
        if matches!(sp.kind, SpanKind::Code | SpanKind::String | SpanKind::Skipped) {
            out.extend_from_slice(src[sp.start..sp.end].as_bytes());
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lang::Lang;

    #[test]
    fn rust_nested_comments() {
        let src = "fn main() { /* outer /* inner */ still */ let x = 1; }";
        let spans = lex(src, &Lang::Rust.profile());
        let comments: Vec<_> = spans.iter().filter(|s| s.is_comment()).collect();
        assert_eq!(comments.len(), 1);
        assert!(comments[0].text(src).contains("inner"));
        assert!(comments[0].text(src).contains("still"));
    }

    #[test]
    fn string_with_slashes() {
        let src = r#"let u = "https://example.com"; // comment"#;
        let spans = lex(src, &Lang::Rust.profile());
        let kinds: Vec<_> = spans.iter().map(|s| s.kind).collect();
        assert!(kinds.contains(&SpanKind::String));
        assert!(kinds.contains(&SpanKind::LineComment));
    }

    #[test]
    fn go_backtick() {
        let src = "s := `line // not comment\n/* still string */`\n// real";
        let spans = lex(src, &Lang::Go.profile());
        assert!(spans.iter().any(|s| s.kind == SpanKind::String));
        assert!(spans.iter().any(|s| s.kind == SpanKind::LineComment));
    }

    #[test]
    fn js_template() {
        let src = "const s = `hello ${foo(`/re/`)} world`; // end";
        let spans = lex(src, &Lang::JavaScript.profile());
        assert!(spans.iter().any(|s| s.kind == SpanKind::LineComment));
    }

    #[test]
    fn rust_raw_string_not_comment() {
        let src = "let s = r#\" line // not comment \"#; // real";
        let spans = lex(src, &Lang::Rust.profile());
        assert!(spans.iter().any(|s| s.kind == SpanKind::String));
        assert!(spans.iter().any(|s| s.kind == SpanKind::LineComment));
    }

    fn kinds_at_comment<'a>(src: &'a str, lang: Lang) -> Vec<(SpanKind, &'a str)> {
        lex(src, &lang.profile())
            .into_iter()
            .filter(|s| s.is_comment() || s.kind == SpanKind::String || s.kind == SpanKind::Skipped)
            .map(|s| (s.kind, s.text(src)))
            .collect()
    }

    #[test]
    fn html_quotes_in_text_are_not_strings() {
        let src = r#"<p>He said "ok"</p><!-- real comment -->"#;
        let spans = lex(src, &Lang::Html.profile());
        let comments: Vec<_> = spans.iter().filter(|s| s.is_comment()).collect();
        assert_eq!(comments.len(), 1);
        assert!(comments[0].text(src).contains("real comment"));
        assert!(!spans.iter().any(|s| s.kind == SpanKind::String));
    }

    #[test]
    fn html_attr_comment_lookalike_is_string() {
        let src = r#"<div title="<!-- not a comment // -->"></div><!-- real -->"#;
        let spans = lex(src, &Lang::Html.profile());
        assert!(spans.iter().any(|s| s.kind == SpanKind::String
            && s.text(src).contains("<!-- not a comment")));
        assert_eq!(spans.iter().filter(|s| s.is_comment()).count(), 1);
    }

    #[test]
    fn html_attr_newline_stays_string() {
        let src = "<div title=\"a\nb\"></div><!-- real -->";
        let spans = lex(src, &Lang::Html.profile());
        let strings: Vec<_> = spans.iter().filter(|s| s.kind == SpanKind::String).collect();
        assert_eq!(strings.len(), 1);
        assert!(strings[0].text(src).contains('\n'));
        assert_eq!(spans.iter().filter(|s| s.is_comment()).count(), 1);
    }

    #[test]
    fn html_script_body_skipped() {
        let src = "<script><!-- hide\nvar x = 1;\n//--></script><!-- real -->";
        let spans = lex(src, &Lang::Html.profile());
        assert!(spans.iter().any(|s| s.kind == SpanKind::Skipped
            && s.text(src).contains("var x")));
        let comments: Vec<_> = spans.iter().filter(|s| s.is_comment()).collect();
        assert_eq!(comments.len(), 1);
        assert!(comments[0].text(src).contains("real"));
    }

    #[test]
    fn html_script_case_insensitive() {
        let src = "<SCRIPT>alert(1)</SCRIPT><!-- real -->";
        let spans = lex(src, &Lang::Html.profile());
        assert!(spans.iter().any(|s| s.kind == SpanKind::Skipped));
        assert_eq!(spans.iter().filter(|s| s.is_comment()).count(), 1);
    }

    #[test]
    fn html_script_gt_does_not_close() {
        let src = "<script src=\"x.js\"/><!-- not a comment -->";
        let spans = lex(src, &Lang::Html.profile());
        assert!(
            !spans.iter().any(|s| s.is_comment()),
            "{:?}",
            kinds_at_comment(src, Lang::Html)
        );
        assert!(spans.iter().any(|s| s.kind == SpanKind::Skipped
            && s.text(src).contains("<!-- not a comment")));
    }

    #[test]
    fn xml_script_gt_is_empty() {
        let src = r#"<script src="x.js"/><!-- after empty script -->"#;
        let spans = lex(src, &Lang::Xml.profile());
        let comments: Vec<_> = spans.iter().filter(|s| s.is_comment()).collect();
        assert_eq!(comments.len(), 1);
        assert!(comments[0].text(src).contains("after empty script"));
    }

    #[test]
    fn html_bang_comment_closer() {
        let src = "<!-- bang closed --!><p>x</p>";
        let spans = lex(src, &Lang::Html.profile());
        let comments: Vec<_> = spans.iter().filter(|s| s.is_comment()).collect();
        assert_eq!(comments.len(), 1);
        assert!(comments[0].text(src).ends_with("--!>"));
        assert!(spans.iter().any(|s| s.kind == SpanKind::Code && s.text(src).contains("<p>")));
    }

    #[test]
    fn xml_cdata_hides_comment() {
        let src = "<![CDATA[ <!-- not a comment --> ]]><!-- real -->";
        let spans = lex(src, &Lang::Xml.profile());
        assert!(spans.iter().any(|s| s.kind == SpanKind::String
            && s.text(src).contains("<!-- not a comment")));
        assert_eq!(spans.iter().filter(|s| s.is_comment()).count(), 1);
    }

    #[test]
    fn html_template_and_noscript_are_comments() {
        let src = "<template><!-- in template --></template><noscript><!-- in noscript --></noscript>";
        let spans = lex(src, &Lang::Html.profile());
        let comments: Vec<_> = spans.iter().filter(|s| s.is_comment()).collect();
        assert_eq!(comments.len(), 2);
    }

    #[test]
    fn html_unclosed_title_skips_rest() {
        let src = "<title><!-- swallowed";
        let spans = lex(src, &Lang::Html.profile());
        assert!(!spans.iter().any(|s| s.is_comment()));
        assert!(spans.iter().any(|s| s.kind == SpanKind::Skipped));
    }

    #[test]
    fn unterminated_markup_comment_to_eof() {
        let src = "<p><!-- no close";
        let spans = lex(src, &Lang::Html.profile());
        let comments: Vec<_> = spans.iter().filter(|s| s.is_comment()).collect();
        assert_eq!(comments.len(), 1);
        assert_eq!(comments[0].end, src.len());
    }

    #[test]
    fn jsx_still_c_family() {
        let src = "const x = <div><!-- c --></div>;\n// real";
        let spans = lex(src, &Lang::JavaScript.profile());
        assert!(!spans.iter().any(|s| s.kind == SpanKind::BlockComment));
        assert!(spans.iter().any(|s| s.kind == SpanKind::LineComment
            && s.text(src).contains("real")));
    }

    #[test]
    fn xml_comment_inside_script() {
        let src = "<script><!-- real xml comment --></script>";
        let spans = lex(src, &Lang::Xml.profile());
        assert_eq!(spans.iter().filter(|s| s.is_comment()).count(), 1);
        assert!(!spans.iter().any(|s| s.kind == SpanKind::Skipped));
    }

    #[test]
    fn xml_dtd_internal_subset_comment() {
        let src = r#"<!DOCTYPE foo [ <!-- dtd comment --> ]><root/>"#;
        let spans = lex(src, &Lang::Xml.profile());
        let comments: Vec<_> = spans.iter().filter(|s| s.is_comment()).collect();
        assert_eq!(comments.len(), 1);
        assert!(comments[0].text(src).contains("dtd comment"));
    }
}
