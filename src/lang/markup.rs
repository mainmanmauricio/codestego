//! Markup trivia lexer (HTML / XML / SGML instance documents).

use super::lexer::{Span, SpanKind};
use super::{Lang, LangProfile};

pub(super) fn lex_markup(src: &str, profile: &LangProfile) -> Vec<Span> {
    let bytes = src.as_bytes();
    let html = profile.lang == Lang::Html;
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

        if bytes[i] == b'<' {
            if bytes[i..].starts_with(b"<!--") {
                flush_code(&mut spans, &mut code_start, i);
                let start = i;
                i += 4;
                i = scan_comment_end(bytes, i, html);
                spans.push(Span {
                    kind: SpanKind::BlockComment,
                    start,
                    end: i,
                });
                continue;
            }
            if bytes[i..].starts_with(b"<![CDATA[") {
                flush_code(&mut spans, &mut code_start, i);
                let start = i;
                i += 9;
                while i < bytes.len() {
                    if bytes[i..].starts_with(b"]]>") {
                        i += 3;
                        break;
                    }
                    i += 1;
                }
                spans.push(Span {
                    kind: SpanKind::String,
                    start,
                    end: i,
                });
                continue;
            }
            if i + 1 < bytes.len() && bytes[i + 1] == b'!' {
                // DOCTYPE and other <! declarations: code bytewise so inner <!-- is found
                if code_start.is_none() {
                    code_start = Some(i);
                }
                i += 1;
                continue;
            }
            if bytes[i..].starts_with(b"<?") {
                flush_code(&mut spans, &mut code_start, i);
                let start = i;
                i += 2;
                i = scan_pi_end(bytes, i, html);
                spans.push(Span {
                    kind: SpanKind::Skipped,
                    start,
                    end: i,
                });
                continue;
            }

            // Start or end tag
            flush_code(&mut spans, &mut code_start, i);
            let tag_start = i;
            i += 1;
            let is_end = i < bytes.len() && bytes[i] == b'/';
            if is_end {
                i += 1;
            }
            let name_start = i;
            while i < bytes.len() && is_name_char(bytes[i]) {
                i += 1;
            }
            if name_start == i {
                // `<` not a tag
                if code_start.is_none() {
                    code_start = Some(tag_start);
                }
                continue;
            }
            let name = &bytes[name_start..i];
            code_start = Some(tag_start);
            loop {
                if i >= bytes.len() {
                    break;
                }
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
                if bytes[i] == b'"' || bytes[i] == b'\'' {
                    flush_code(&mut spans, &mut code_start, i);
                    let q = bytes[i];
                    let start = i;
                    i += 1;
                    while i < bytes.len() && bytes[i] != q {
                        i += 1;
                    }
                    if i < bytes.len() {
                        i += 1;
                    }
                    spans.push(Span {
                        kind: SpanKind::String,
                        start,
                        end: i,
                    });
                    continue;
                }
                if bytes[i] == b'>' {
                    if code_start.is_none() {
                        code_start = Some(i);
                    }
                    i += 1;
                    flush_code(&mut spans, &mut code_start, i);
                    break;
                }
                if code_start.is_none() {
                    code_start = Some(i);
                }
                i += 1;
            }

            if html && !is_end && is_html_skip_name(name) {
                let body_start = i;
                i = skip_until_end_tag(bytes, i, name, true);
                if body_start < i {
                    spans.push(Span {
                        kind: SpanKind::Skipped,
                        start: body_start,
                        end: i,
                    });
                }
            }
            continue;
        }

        if code_start.is_none() {
            code_start = Some(i);
        }
        i += 1;
    }
    flush_code(&mut spans, &mut code_start, bytes.len());
    spans
}

fn scan_comment_end(bytes: &[u8], mut i: usize, html: bool) -> usize {
    while i < bytes.len() {
        if bytes[i..].starts_with(b"-->") {
            return i + 3;
        }
        if html && bytes[i..].starts_with(b"--!>") {
            return i + 4;
        }
        i += 1;
    }
    bytes.len()
}

fn scan_pi_end(bytes: &[u8], mut i: usize, html: bool) -> usize {
    if html {
        while i < bytes.len() {
            if bytes[i] == b'>' {
                return i + 1;
            }
            i += 1;
        }
        return bytes.len();
    }
    while i + 1 < bytes.len() {
        if bytes[i] == b'?' && bytes[i + 1] == b'>' {
            return i + 2;
        }
        i += 1;
    }
    bytes.len()
}

fn is_name_char(b: u8) -> bool {
    b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b':' | b'.')
}

fn is_html_skip_name(name: &[u8]) -> bool {
    eq_ignore_ascii(name, b"script")
        || eq_ignore_ascii(name, b"style")
        || eq_ignore_ascii(name, b"iframe")
        || eq_ignore_ascii(name, b"textarea")
        || eq_ignore_ascii(name, b"title")
}

fn eq_ignore_ascii(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len()
        && a.iter()
            .zip(b)
            .all(|(x, y)| x.to_ascii_lowercase() == y.to_ascii_lowercase())
}

fn names_eq(a: &[u8], b: &[u8], html: bool) -> bool {
    if html {
        eq_ignore_ascii(a, b)
    } else {
        a == b
    }
}

fn skip_until_end_tag(bytes: &[u8], mut i: usize, name: &[u8], html: bool) -> usize {
    while i < bytes.len() {
        if bytes[i] == b'<' && i + 1 < bytes.len() && bytes[i + 1] == b'/' {
            let nstart = i + 2;
            let mut nend = nstart;
            while nend < bytes.len() && is_name_char(bytes[nend]) {
                nend += 1;
            }
            if names_eq(&bytes[nstart..nend], name, html) {
                let mut j = nend;
                while j < bytes.len() && bytes[j] != b'>' {
                    j += 1;
                }
                if j < bytes.len() {
                    return j + 1;
                }
            }
        }
        i += 1;
    }
    bytes.len()
}
