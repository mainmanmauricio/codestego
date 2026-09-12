//! Language profiles and trivia lexer for C-family and markup languages.

pub mod lexer;
mod markup;

pub use lexer::{Span, SpanKind, lex, non_trivia_bytes};

use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    C,
    Cpp,
    Go,
    Java,
    JavaScript,
    TypeScript,
    Rust,
    Html,
    Xml,
    Sgml,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommentSyntax {
    CFamily,
    Markup,
}

impl Lang {
    pub fn from_ext(path: &Path) -> Option<Self> {
        let ext = path.extension()?.to_str()?.to_ascii_lowercase();
        match ext.as_str() {
            "c" | "h" => Some(Self::C),
            "cc" | "cpp" | "cxx" | "hpp" | "hh" | "hxx" => Some(Self::Cpp),
            "go" => Some(Self::Go),
            "java" => Some(Self::Java),
            "js" | "jsx" | "mjs" | "cjs" => Some(Self::JavaScript),
            "ts" | "tsx" | "mts" | "cts" => Some(Self::TypeScript),
            "rs" => Some(Self::Rust),
            "html" | "htm" => Some(Self::Html),
            "xml" | "svg" | "xhtml" | "xsl" | "xslt" | "xsd" | "wsdl" | "rss" | "atom"
            | "plist" | "csproj" | "fsproj" | "vbproj" | "xaml" | "resx" | "nuspec" | "ui"
            | "kml" | "gpx" | "rdf" | "dtd" => Some(Self::Xml),
            "sgml" | "sgm" => Some(Self::Sgml),
            _ => None,
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "c" => Some(Self::C),
            "cpp" | "c++" | "cxx" => Some(Self::Cpp),
            "go" => Some(Self::Go),
            "java" => Some(Self::Java),
            "js" | "javascript" => Some(Self::JavaScript),
            "ts" | "typescript" => Some(Self::TypeScript),
            "rust" | "rs" => Some(Self::Rust),
            "html" | "htm" => Some(Self::Html),
            "xml" | "svg" | "xhtml" => Some(Self::Xml),
            "sgml" | "sgm" => Some(Self::Sgml),
            "auto" => None,
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::C => "c",
            Self::Cpp => "cpp",
            Self::Go => "go",
            Self::Java => "java",
            Self::JavaScript => "js",
            Self::TypeScript => "ts",
            Self::Rust => "rust",
            Self::Html => "html",
            Self::Xml => "xml",
            Self::Sgml => "sgml",
        }
    }

    pub fn profile(self) -> LangProfile {
        match self {
            Self::C => LangProfile {
                lang: self,
                line_comment: true,
                block_comment: true,
                nested_block: false,
                backslash_continue: true,
                doc_comments: false,
                raw_strings: RawStringStyle::None,
                template_literals: false,
                backtick_strings: false,
                comment_syntax: CommentSyntax::CFamily,
            },
            Self::Cpp => LangProfile {
                lang: self,
                line_comment: true,
                block_comment: true,
                nested_block: false,
                backslash_continue: true,
                doc_comments: false,
                raw_strings: RawStringStyle::Cpp,
                template_literals: false,
                backtick_strings: false,
                comment_syntax: CommentSyntax::CFamily,
            },
            Self::Go => LangProfile {
                lang: self,
                line_comment: true,
                block_comment: true,
                nested_block: false,
                backslash_continue: false,
                doc_comments: false,
                raw_strings: RawStringStyle::None,
                template_literals: false,
                backtick_strings: true,
                comment_syntax: CommentSyntax::CFamily,
            },
            Self::Java => LangProfile {
                lang: self,
                line_comment: true,
                block_comment: true,
                nested_block: false,
                backslash_continue: false,
                doc_comments: false,
                raw_strings: RawStringStyle::None,
                template_literals: false,
                backtick_strings: false,
                comment_syntax: CommentSyntax::CFamily,
            },
            Self::JavaScript | Self::TypeScript => LangProfile {
                lang: self,
                line_comment: true,
                block_comment: true,
                nested_block: false,
                backslash_continue: false,
                doc_comments: false,
                raw_strings: RawStringStyle::None,
                template_literals: true,
                backtick_strings: false, // handled as template
                comment_syntax: CommentSyntax::CFamily,
            },
            Self::Rust => LangProfile {
                lang: self,
                line_comment: true,
                block_comment: true,
                nested_block: true,
                backslash_continue: false,
                doc_comments: true,
                raw_strings: RawStringStyle::Rust,
                template_literals: false,
                backtick_strings: false,
                comment_syntax: CommentSyntax::CFamily,
            },
            Self::Html | Self::Xml | Self::Sgml => LangProfile {
                lang: self,
                line_comment: false,
                block_comment: false,
                nested_block: false,
                backslash_continue: false,
                doc_comments: false,
                raw_strings: RawStringStyle::None,
                template_literals: false,
                backtick_strings: false,
                comment_syntax: CommentSyntax::Markup,
            },
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RawStringStyle {
    None,
    Rust,
    Cpp,
}

#[derive(Debug, Clone, Copy)]
pub struct LangProfile {
    pub lang: Lang,
    pub line_comment: bool,
    pub block_comment: bool,
    pub nested_block: bool,
    pub backslash_continue: bool,
    pub doc_comments: bool,
    pub raw_strings: RawStringStyle,
    pub template_literals: bool,
    pub backtick_strings: bool,
    pub comment_syntax: CommentSyntax,
}

impl LangProfile {
    pub fn eof_comment_delimiters(self) -> (&'static str, &'static str) {
        match self.comment_syntax {
            CommentSyntax::Markup => ("<!--", "-->"),
            CommentSyntax::CFamily => ("/*", "*/"),
        }
    }

    pub fn is_html(self) -> bool {
        self.lang == Lang::Html
    }
}

/// True when `path` is a `.plist` whose contents start with the binary plist magic.
pub fn is_binary_plist(path: &Path, src: &str) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("plist"))
        && src.as_bytes().starts_with(b"bplist")
}

/// Resolve language from optional override and path.
pub fn detect_lang(override_lang: Option<&str>, path: &Path) -> Option<Lang> {
    if let Some(s) = override_lang {
        if s != "auto" {
            return Lang::parse(s);
        }
    }
    Lang::from_ext(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn detect_by_extension() {
        assert_eq!(detect_lang(None, Path::new("a.rs")), Some(Lang::Rust));
        assert_eq!(detect_lang(None, Path::new("a.go")), Some(Lang::Go));
        assert_eq!(detect_lang(Some("cpp"), Path::new("a.rs")), Some(Lang::Cpp));
        assert_eq!(
            detect_lang(Some("auto"), Path::new("x.ts")),
            Some(Lang::TypeScript)
        );
        assert_eq!(detect_lang(None, Path::new("a.html")), Some(Lang::Html));
        assert_eq!(detect_lang(None, Path::new("a.htm")), Some(Lang::Html));
        assert_eq!(detect_lang(None, Path::new("a.xml")), Some(Lang::Xml));
        assert_eq!(detect_lang(None, Path::new("a.svg")), Some(Lang::Xml));
        assert_eq!(detect_lang(None, Path::new("a.xhtml")), Some(Lang::Xml));
        assert_eq!(detect_lang(None, Path::new("a.dtd")), Some(Lang::Xml));
        assert_eq!(detect_lang(None, Path::new("a.csproj")), Some(Lang::Xml));
        assert_eq!(detect_lang(None, Path::new("a.plist")), Some(Lang::Xml));
        assert_eq!(detect_lang(None, Path::new("a.sgml")), Some(Lang::Sgml));
        assert_eq!(detect_lang(None, Path::new("a.sgm")), Some(Lang::Sgml));
        assert_eq!(detect_lang(None, Path::new("a.vue")), None);
        assert_eq!(detect_lang(None, Path::new("a.php")), None);
        assert_eq!(detect_lang(None, Path::new("a.md")), None);
        assert_eq!(detect_lang(None, Path::new("a.config")), None);
        assert_eq!(
            detect_lang(None, Path::new("a.jsx")),
            Some(Lang::JavaScript)
        );
        assert_eq!(
            detect_lang(None, Path::new("a.tsx")),
            Some(Lang::TypeScript)
        );
        assert_eq!(Lang::parse("html"), Some(Lang::Html));
        assert_eq!(Lang::parse("htm"), Some(Lang::Html));
        assert_eq!(Lang::parse("xml"), Some(Lang::Xml));
        assert_eq!(Lang::parse("svg"), Some(Lang::Xml));
        assert_eq!(Lang::parse("xhtml"), Some(Lang::Xml));
        assert_eq!(Lang::parse("sgml"), Some(Lang::Sgml));
        assert_eq!(Lang::parse("sgm"), Some(Lang::Sgml));
        assert_eq!(Lang::parse("xsd"), None);
        assert_eq!(detect_lang(Some("html"), Path::new("a.rs")), Some(Lang::Html));
        assert_eq!(Lang::Sgml.name(), "sgml");
        assert_eq!(
            Lang::Sgml.profile().comment_syntax,
            CommentSyntax::Markup
        );
        assert_eq!(
            Lang::Xml.profile().comment_syntax,
            CommentSyntax::Markup
        );
        assert!(!Lang::Html.profile().line_comment);
        assert!(!Lang::Html.profile().block_comment);
    }

    #[test]
    fn binary_plist_sniff() {
        assert!(is_binary_plist(
            Path::new("x.plist"),
            "bplist00\0garbage"
        ));
        assert!(!is_binary_plist(
            Path::new("x.plist"),
            "<plist><!-- c --></plist>"
        ));
        assert!(!is_binary_plist(Path::new("x.bin"), "bplist00"));
        assert!(!is_binary_plist(Path::new("x.xml"), "bplist00"));
    }
}
