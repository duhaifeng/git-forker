//! 差异里的语法着色。
//!
//! 做法对应 Fork 二进制里的分层：先划出注释和字符串这些范围，再在范围之外匹配关键字和类型。
//! 语言由文件扩展名决定；没有对应语法时整行保持普通文字。

use std::path::Path;

use eframe::egui::Color32;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Comment,
    String,
    Keyword,
    Type,
    Number,
    Attribute,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    pub start: usize,
    pub end: usize,
    pub kind: Kind,
}

/// 跨行的块注释、三引号字符串或模板字符串。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Carry {
    block_end: Option<&'static str>,
    /// Markdown 围栏代码块内部。
    fence: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Language {
    Rust,
    C,
    Cpp,
    ObjC,
    Swift,
    Go,
    Java,
    Kotlin,
    CSharp,
    JavaScript,
    TypeScript,
    Python,
    Ruby,
    Php,
    Shell,
    Sql,
    Yaml,
    Toml,
    Json,
    Html,
    Css,
    Markdown,
}

impl Language {
    #[cfg(test)]
    const ALL: [Language; 22] = [
        Language::Rust,
        Language::C,
        Language::Cpp,
        Language::ObjC,
        Language::Swift,
        Language::Go,
        Language::Java,
        Language::Kotlin,
        Language::CSharp,
        Language::JavaScript,
        Language::TypeScript,
        Language::Python,
        Language::Ruby,
        Language::Php,
        Language::Shell,
        Language::Sql,
        Language::Yaml,
        Language::Toml,
        Language::Json,
        Language::Html,
        Language::Css,
        Language::Markdown,
    ];
}

pub fn language_for(path: &str) -> Option<Language> {
    let path = Path::new(path);
    let name = path.file_name()?.to_str()?;
    if matches!(name, "Dockerfile" | "Makefile" | "makefile" | "GNUmakefile") {
        return Some(Language::Shell);
    }
    let ext = path.extension()?.to_str()?.to_ascii_lowercase();
    Some(match ext.as_str() {
        "rs" => Language::Rust,
        "c" | "h" => Language::C,
        "cc" | "cpp" | "cxx" | "hh" | "hpp" | "hxx" => Language::Cpp,
        "m" | "mm" => Language::ObjC,
        "swift" => Language::Swift,
        "go" => Language::Go,
        "java" => Language::Java,
        "kt" | "kts" => Language::Kotlin,
        "cs" => Language::CSharp,
        "js" | "jsx" | "mjs" | "cjs" => Language::JavaScript,
        "ts" | "tsx" => Language::TypeScript,
        "py" => Language::Python,
        "rb" => Language::Ruby,
        "php" => Language::Php,
        "sh" | "bash" | "zsh" => Language::Shell,
        "sql" => Language::Sql,
        "yml" | "yaml" => Language::Yaml,
        "toml" => Language::Toml,
        "json" => Language::Json,
        "html" | "htm" | "xml" | "svg" => Language::Html,
        "css" | "scss" => Language::Css,
        "md" | "markdown" => Language::Markdown,
        _ => return None,
    })
}

pub fn color(kind: Kind, dark: bool) -> Color32 {
    if dark {
        match kind {
            Kind::Comment => Color32::from_rgb(140, 148, 158),
            Kind::String => Color32::from_rgb(230, 160, 110),
            Kind::Keyword => Color32::from_rgb(210, 150, 230),
            Kind::Type => Color32::from_rgb(120, 210, 195),
            Kind::Number => Color32::from_rgb(170, 210, 140),
            Kind::Attribute => Color32::from_rgb(230, 200, 120),
        }
    } else {
        match kind {
            Kind::Comment => Color32::from_rgb(90, 99, 110),
            Kind::String => Color32::from_rgb(163, 58, 20),
            Kind::Keyword => Color32::from_rgb(150, 40, 170),
            Kind::Type => Color32::from_rgb(12, 110, 125),
            Kind::Number => Color32::from_rgb(16, 112, 48),
            Kind::Attribute => Color32::from_rgb(140, 90, 16),
        }
    }
}

/// 给一行上色，并更新跨行状态。返回的区间是 `line` 的字节区间，互不重叠。
pub fn highlight(line: &str, language: Language, carry: &mut Carry) -> Vec<Span> {
    match language {
        Language::Markdown => highlight_markdown(line, carry),
        Language::Html => highlight_html(line, carry),
        Language::Css => highlight_css(line, carry),
        _ => highlight_code(line, grammar(language), carry),
    }
}

struct Grammar {
    keywords: &'static [&'static str],
    types: &'static [&'static str],
    line_comments: &'static [&'static str],
    block: Option<(&'static str, &'static str)>,
    quotes: &'static [char],
    triples: &'static [&'static str],
    case_insensitive: bool,
    char_literal: bool,
    rust_attr: bool,
    preprocessor: bool,
    pascal_types: bool,
    template: bool,
}

fn grammar(language: Language) -> Grammar {
    let code = Grammar {
        keywords: &[],
        types: &[],
        line_comments: &[],
        block: None,
        quotes: &['"'],
        triples: &[],
        case_insensitive: false,
        char_literal: false,
        rust_attr: false,
        preprocessor: false,
        pascal_types: false,
        template: false,
    };
    match language {
        Language::Rust => Grammar {
            keywords: RUST_KEYWORDS,
            types: RUST_TYPES,
            line_comments: &["//"],
            block: Some(("/*", "*/")),
            char_literal: true,
            rust_attr: true,
            pascal_types: true,
            ..code
        },
        Language::C => Grammar {
            keywords: C_KEYWORDS,
            types: C_TYPES,
            line_comments: &["//"],
            block: Some(("/*", "*/")),
            char_literal: true,
            preprocessor: true,
            ..code
        },
        Language::Cpp => Grammar {
            keywords: CPP_KEYWORDS,
            types: C_TYPES,
            line_comments: &["//"],
            block: Some(("/*", "*/")),
            char_literal: true,
            preprocessor: true,
            pascal_types: true,
            ..code
        },
        Language::ObjC => Grammar {
            keywords: OBJC_KEYWORDS,
            types: OBJC_TYPES,
            line_comments: &["//"],
            block: Some(("/*", "*/")),
            char_literal: true,
            preprocessor: true,
            pascal_types: true,
            ..code
        },
        Language::Swift => Grammar {
            keywords: SWIFT_KEYWORDS,
            types: SWIFT_TYPES,
            line_comments: &["//"],
            block: Some(("/*", "*/")),
            pascal_types: true,
            ..code
        },
        Language::Go => Grammar {
            keywords: GO_KEYWORDS,
            types: GO_TYPES,
            line_comments: &["//"],
            block: Some(("/*", "*/")),
            char_literal: true,
            ..code
        },
        Language::Java | Language::Kotlin => Grammar {
            keywords: if language == Language::Kotlin {
                KOTLIN_KEYWORDS
            } else {
                JAVA_KEYWORDS
            },
            types: &[
                "boolean", "byte", "char", "double", "float", "int", "long", "short", "void",
            ],
            line_comments: &["//"],
            block: Some(("/*", "*/")),
            char_literal: true,
            pascal_types: true,
            ..code
        },
        Language::CSharp => Grammar {
            keywords: CSHARP_KEYWORDS,
            types: &[
                "bool", "byte", "char", "decimal", "double", "float", "int", "long", "object",
                "string", "void",
            ],
            line_comments: &["//"],
            block: Some(("/*", "*/")),
            char_literal: true,
            pascal_types: true,
            ..code
        },
        Language::JavaScript => Grammar {
            keywords: JS_KEYWORDS,
            line_comments: &["//"],
            block: Some(("/*", "*/")),
            quotes: &['"', '\''],
            template: true,
            ..code
        },
        Language::TypeScript => Grammar {
            keywords: JS_KEYWORDS,
            types: &[
                "any", "boolean", "never", "number", "string", "unknown", "void",
            ],
            line_comments: &["//"],
            block: Some(("/*", "*/")),
            quotes: &['"', '\''],
            template: true,
            pascal_types: true,
            ..code
        },
        Language::Python => Grammar {
            keywords: PYTHON_KEYWORDS,
            line_comments: &["#"],
            quotes: &['"', '\''],
            triples: &["\"\"\"", "'''"],
            ..code
        },
        Language::Ruby => Grammar {
            keywords: RUBY_KEYWORDS,
            line_comments: &["#"],
            quotes: &['"', '\''],
            ..code
        },
        Language::Php => Grammar {
            keywords: PHP_KEYWORDS,
            line_comments: &["//", "#"],
            block: Some(("/*", "*/")),
            quotes: &['"', '\''],
            case_insensitive: true,
            ..code
        },
        Language::Shell => Grammar {
            keywords: SHELL_KEYWORDS,
            line_comments: &["#"],
            quotes: &['"', '\''],
            ..code
        },
        Language::Sql => Grammar {
            keywords: SQL_KEYWORDS,
            types: SQL_TYPES,
            line_comments: &["--"],
            block: Some(("/*", "*/")),
            quotes: &['\''],
            case_insensitive: true,
            ..code
        },
        Language::Yaml => Grammar {
            keywords: &["false", "no", "null", "true", "yes"],
            line_comments: &["#"],
            quotes: &['"', '\''],
            ..code
        },
        Language::Toml => Grammar {
            keywords: &["false", "true"],
            line_comments: &["#"],
            quotes: &['"', '\''],
            ..code
        },
        Language::Json => Grammar {
            keywords: &["false", "null", "true"],
            quotes: &['"'],
            ..code
        },
        Language::Html | Language::Css | Language::Markdown => code,
    }
}

fn highlight_code(line: &str, grammar: Grammar, carry: &mut Carry) -> Vec<Span> {
    let mut spans = Vec::new();
    let mut i = continue_block(line, carry, &mut spans);
    let bytes = line.as_bytes();
    let mut seen_code = i > 0;
    while i < bytes.len() {
        if bytes[i].is_ascii_whitespace() {
            i += 1;
            continue;
        }
        if let Some(marker) = starts_with_any(line, i, grammar.line_comments) {
            // 行首的 #include 是预处理指令，不是注释。
            let preprocessor = grammar.preprocessor && !seen_code && marker == "#";
            if !preprocessor {
                push(&mut spans, i, line.len(), Kind::Comment);
                break;
            }
        }
        if let Some((_, end)) = grammar.block
            && line[i..].starts_with(grammar.block.unwrap().0)
        {
            let start_len = grammar.block.unwrap().0.len();
            i = finish_block(line, i, start_len, end, Kind::Comment, carry, &mut spans);
            seen_code = true;
            continue;
        }
        if let Some(marker) = starts_with_any(line, i, grammar.triples) {
            i = finish_block(
                line,
                i,
                marker.len(),
                marker,
                Kind::String,
                carry,
                &mut spans,
            );
            seen_code = true;
            continue;
        }
        if grammar.template && bytes[i] == b'`' {
            i = finish_block(line, i, 1, "`", Kind::String, carry, &mut spans);
            seen_code = true;
            continue;
        }
        if let Some(quote) = grammar
            .quotes
            .iter()
            .copied()
            .find(|quote| bytes[i] == *quote as u8)
        {
            let end = scan_quoted(line, i, quote as u8);
            push(&mut spans, i, end, Kind::String);
            i = end;
            seen_code = true;
            continue;
        }
        if grammar.rust_attr && (line[i..].starts_with("#[") || line[i..].starts_with("#![")) {
            let end = line[i..]
                .find(']')
                .map(|offset| i + offset + 1)
                .unwrap_or(line.len());
            push(&mut spans, i, end, Kind::Attribute);
            i = end;
            seen_code = true;
            continue;
        }
        if grammar.preprocessor && !seen_code && bytes[i] == b'#' {
            let end = ident_end(line, i + 1);
            push(&mut spans, i, end, Kind::Attribute);
            i = end;
            seen_code = true;
            continue;
        }
        if grammar.char_literal && bytes[i] == b'\'' {
            if let Some(end) = char_literal_end(line, i) {
                push(&mut spans, i, end, Kind::String);
                i = end;
                seen_code = true;
                continue;
            }
            if grammar.rust_attr && ident_end(line, i + 1) > i + 1 {
                let end = ident_end(line, i + 1);
                push(&mut spans, i, end, Kind::Attribute);
                i = end;
                seen_code = true;
                continue;
            }
        }
        if bytes[i].is_ascii_digit() {
            let end = number_end(line, i);
            push(&mut spans, i, end, Kind::Number);
            i = end;
            seen_code = true;
            continue;
        }
        if is_ident_byte(bytes[i]) {
            let end = ident_end(line, i);
            let word = &line[i..end];
            let kind = if contains(grammar.keywords, word, grammar.case_insensitive) {
                Some(Kind::Keyword)
            } else if contains(grammar.types, word, grammar.case_insensitive)
                || (grammar.pascal_types && is_pascal(word))
            {
                Some(Kind::Type)
            } else {
                None
            };
            if let Some(kind) = kind {
                push(&mut spans, i, end, kind);
            }
            i = end;
            seen_code = true;
            continue;
        }
        i += char_len(bytes, i);
        seen_code = true;
    }
    spans
}

fn highlight_markdown(line: &str, carry: &mut Carry) -> Vec<Span> {
    let trimmed = line.trim_start();
    if carry.fence {
        carry.fence = !trimmed.starts_with("```");
        return vec![Span {
            start: 0,
            end: line.len(),
            kind: Kind::String,
        }];
    }
    if trimmed.starts_with("```") {
        carry.fence = true;
        return vec![Span {
            start: 0,
            end: line.len(),
            kind: Kind::String,
        }];
    }
    if trimmed.starts_with('#') {
        return vec![Span {
            start: 0,
            end: line.len(),
            kind: Kind::Keyword,
        }];
    }
    let mut spans = Vec::new();
    let bytes = line.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'`'
            && let Some(offset) = line[i + 1..].find('`')
        {
            push(&mut spans, i, i + 1 + offset + 1, Kind::String);
            i = i + 1 + offset + 1;
            continue;
        }
        i += char_len(bytes, i);
    }
    spans
}

fn highlight_html(line: &str, carry: &mut Carry) -> Vec<Span> {
    let mut spans = Vec::new();
    let mut i = continue_block(line, carry, &mut spans);
    let bytes = line.as_bytes();
    while i < bytes.len() {
        if line[i..].starts_with("<!--") {
            i = finish_block(line, i, 4, "-->", Kind::Comment, carry, &mut spans);
            continue;
        }
        if bytes[i] == b'<' {
            let name = if line[i..].starts_with("</") {
                i + 2
            } else {
                i + 1
            };
            let end = ident_end(line, name);
            if end > name {
                push(&mut spans, i, end, Kind::Keyword);
                i = end;
                continue;
            }
        }
        if is_ident_byte(bytes[i]) {
            let end = ident_end(line, i);
            if line[end..].trim_start().starts_with('=') {
                push(&mut spans, i, end, Kind::Attribute);
            }
            i = end;
            continue;
        }
        if bytes[i] == b'"' || bytes[i] == b'\'' {
            let end = scan_quoted(line, i, bytes[i]);
            push(&mut spans, i, end, Kind::String);
            i = end;
            continue;
        }
        i += char_len(bytes, i);
    }
    spans
}

fn highlight_css(line: &str, carry: &mut Carry) -> Vec<Span> {
    let mut spans = Vec::new();
    let mut i = continue_block(line, carry, &mut spans);
    let bytes = line.as_bytes();
    while i < bytes.len() {
        if bytes[i].is_ascii_whitespace() {
            i += 1;
            continue;
        }
        if line[i..].starts_with("/*") {
            i = finish_block(line, i, 2, "*/", Kind::Comment, carry, &mut spans);
            continue;
        }
        if bytes[i] == b'"' || bytes[i] == b'\'' {
            let end = scan_quoted(line, i, bytes[i]);
            push(&mut spans, i, end, Kind::String);
            i = end;
            continue;
        }
        if bytes[i].is_ascii_digit() {
            let end = number_end(line, i);
            push(&mut spans, i, end, Kind::Number);
            i = end;
            continue;
        }
        if bytes[i] == b'@' || is_ident_byte(bytes[i]) {
            let end = ident_end(line, if bytes[i] == b'@' { i + 1 } else { i });
            let kind = if bytes[i] == b'@' {
                Kind::Keyword
            } else if line[end..].trim_start().starts_with(':') {
                Kind::Attribute
            } else if contains(&["from", "important", "to"], &line[i..end], true) {
                Kind::Keyword
            } else {
                Kind::Type
            };
            // 选择器里的类名保持普通颜色，只给属性和少量关键字上色。
            if !matches!(kind, Kind::Type) || bytes[i] == b'@' {
                push(&mut spans, i, end.max(i + 1), kind);
            }
            i = end.max(i + 1);
            continue;
        }
        i += char_len(bytes, i);
    }
    spans
}

fn continue_block(line: &str, carry: &mut Carry, spans: &mut Vec<Span>) -> usize {
    let Some(end) = carry.block_end else {
        return 0;
    };
    match line.find(end) {
        Some(offset) => {
            let stop = offset + end.len();
            push(
                spans,
                0,
                stop,
                if end == "*/" || end == "-->" {
                    Kind::Comment
                } else {
                    Kind::String
                },
            );
            carry.block_end = None;
            stop
        }
        None => {
            push(
                spans,
                0,
                line.len(),
                if end == "*/" || end == "-->" {
                    Kind::Comment
                } else {
                    Kind::String
                },
            );
            line.len()
        }
    }
}

fn finish_block(
    line: &str,
    start: usize,
    open_len: usize,
    end: &'static str,
    kind: Kind,
    carry: &mut Carry,
    spans: &mut Vec<Span>,
) -> usize {
    if let Some(offset) = line[start + open_len..].find(end) {
        let stop = start + open_len + offset + end.len();
        push(spans, start, stop, kind);
        stop
    } else {
        push(spans, start, line.len(), kind);
        carry.block_end = Some(end);
        line.len()
    }
}

fn push(spans: &mut Vec<Span>, start: usize, end: usize, kind: Kind) {
    if end > start {
        spans.push(Span { start, end, kind });
    }
}

fn starts_with_any<'a>(line: &str, i: usize, markers: &'a [&str]) -> Option<&'a str> {
    markers
        .iter()
        .copied()
        .find(|marker| !marker.is_empty() && line[i..].starts_with(marker))
}

fn contains(words: &[&str], word: &str, insensitive: bool) -> bool {
    if insensitive {
        let lower = word.to_ascii_lowercase();
        words.binary_search(&lower.as_str()).is_ok()
    } else {
        words.binary_search(&word).is_ok()
    }
}

fn is_pascal(word: &str) -> bool {
    let mut chars = word.chars();
    chars.next().is_some_and(|first| first.is_ascii_uppercase())
        && chars.any(|c| c.is_ascii_lowercase())
}

fn is_ident_byte(byte: u8) -> bool {
    byte.is_ascii_alphabetic() || byte == b'_' || byte == b'$'
}

fn ident_end(line: &str, start: usize) -> usize {
    let bytes = line.as_bytes();
    let mut i = start;
    while i < bytes.len() && (is_ident_byte(bytes[i]) || bytes[i].is_ascii_digit()) {
        i += 1;
    }
    i
}

fn number_end(line: &str, start: usize) -> usize {
    let bytes = line.as_bytes();
    let mut i = start + 1;
    let hex = bytes.get(start) == Some(&b'0')
        && matches!(
            bytes.get(start + 1),
            Some(b'x' | b'X' | b'b' | b'B' | b'o' | b'O')
        );
    if hex {
        i = start + 2;
    }
    while i < bytes.len()
        && (bytes[i].is_ascii_hexdigit() && (hex || bytes[i].is_ascii_digit())
            || bytes[i] == b'_'
            || bytes[i] == b'.')
    {
        if bytes[i] == b'.' && (hex || bytes.get(i + 1).is_none_or(|next| !next.is_ascii_digit())) {
            break;
        }
        if !hex
            && bytes[i].is_ascii_hexdigit()
            && !bytes[i].is_ascii_digit()
            && bytes[i] != b'_'
            && bytes[i] != b'.'
        {
            break;
        }
        i += 1;
    }
    i
}

fn scan_quoted(line: &str, start: usize, quote: u8) -> usize {
    let bytes = line.as_bytes();
    let mut i = start + 1;
    while i < bytes.len() {
        if bytes[i] == b'\\' {
            i += char_len(bytes, i);
            if i < bytes.len() {
                i += char_len(bytes, i);
            }
            continue;
        }
        if bytes[i] == quote {
            return i + 1;
        }
        i += char_len(bytes, i);
    }
    i
}

fn char_literal_end(line: &str, start: usize) -> Option<usize> {
    let bytes = line.as_bytes();
    let mut i = start + 1;
    if i >= bytes.len() {
        return None;
    }
    if bytes[i] == b'\\' {
        i += char_len(bytes, i);
        if i < bytes.len() {
            i += char_len(bytes, i);
        }
    } else {
        i += char_len(bytes, i);
    }
    (i < bytes.len() && bytes[i] == b'\'').then_some(i + 1)
}

fn char_len(bytes: &[u8], i: usize) -> usize {
    let width = match bytes.get(i).copied().unwrap_or(0) {
        0..=0x7F => 1,
        0xC0..=0xDF => 2,
        0xE0..=0xEF => 3,
        _ => 4,
    };
    width.min(bytes.len().saturating_sub(i).max(1))
}

const RUST_KEYWORDS: &[&str] = &[
    "Self", "as", "async", "await", "break", "const", "continue", "crate", "dyn", "else", "enum",
    "extern", "false", "fn", "for", "if", "impl", "in", "let", "loop", "match", "mod", "move",
    "mut", "pub", "ref", "return", "self", "static", "struct", "super", "trait", "true", "type",
    "unsafe", "use", "where", "while",
];
const RUST_TYPES: &[&str] = &[
    "bool", "char", "f32", "f64", "i128", "i16", "i32", "i64", "i8", "isize", "str", "u128", "u16",
    "u32", "u64", "u8", "usize",
];
const C_KEYWORDS: &[&str] = &[
    "break", "case", "const", "continue", "default", "do", "else", "enum", "extern", "for", "goto",
    "if", "register", "return", "sizeof", "static", "struct", "switch", "typedef", "union",
    "volatile", "while",
];
const C_TYPES: &[&str] = &[
    "bool", "char", "double", "float", "int", "long", "short", "signed", "unsigned", "void",
];
const CPP_KEYWORDS: &[&str] = &[
    "break",
    "case",
    "catch",
    "class",
    "const",
    "constexpr",
    "continue",
    "default",
    "delete",
    "do",
    "else",
    "enum",
    "explicit",
    "extern",
    "false",
    "for",
    "friend",
    "goto",
    "if",
    "inline",
    "namespace",
    "new",
    "nullptr",
    "operator",
    "private",
    "protected",
    "public",
    "return",
    "sizeof",
    "static",
    "struct",
    "switch",
    "template",
    "this",
    "throw",
    "true",
    "try",
    "typedef",
    "typename",
    "union",
    "using",
    "virtual",
    "volatile",
    "while",
];
const OBJC_KEYWORDS: &[&str] = &[
    "NO",
    "YES",
    "break",
    "case",
    "const",
    "continue",
    "default",
    "do",
    "else",
    "end",
    "enum",
    "extern",
    "for",
    "goto",
    "if",
    "implementation",
    "in",
    "interface",
    "nil",
    "protocol",
    "return",
    "self",
    "sizeof",
    "static",
    "struct",
    "super",
    "switch",
    "typedef",
    "union",
    "while",
];
const OBJC_TYPES: &[&str] = &[
    "BOOL",
    "Class",
    "IMP",
    "NSInteger",
    "NSUInteger",
    "SEL",
    "char",
    "double",
    "float",
    "id",
    "instancetype",
    "int",
    "long",
    "short",
    "void",
];
const SWIFT_KEYWORDS: &[&str] = &[
    "Any",
    "Self",
    "as",
    "associatedtype",
    "break",
    "case",
    "catch",
    "class",
    "continue",
    "default",
    "defer",
    "else",
    "enum",
    "extension",
    "fallthrough",
    "false",
    "for",
    "func",
    "guard",
    "if",
    "import",
    "in",
    "init",
    "inout",
    "internal",
    "let",
    "nil",
    "operator",
    "private",
    "protocol",
    "public",
    "return",
    "self",
    "static",
    "struct",
    "switch",
    "throw",
    "throws",
    "true",
    "try",
    "typealias",
    "var",
    "where",
    "while",
];
const SWIFT_TYPES: &[&str] = &[
    "Bool",
    "Character",
    "Double",
    "Float",
    "Int",
    "String",
    "UInt",
];
const GO_KEYWORDS: &[&str] = &[
    "break",
    "case",
    "chan",
    "const",
    "continue",
    "default",
    "defer",
    "else",
    "fallthrough",
    "for",
    "func",
    "go",
    "goto",
    "if",
    "import",
    "interface",
    "map",
    "package",
    "range",
    "return",
    "select",
    "struct",
    "switch",
    "type",
    "var",
];
const GO_TYPES: &[&str] = &[
    "bool",
    "byte",
    "complex128",
    "complex64",
    "error",
    "float32",
    "float64",
    "int",
    "int16",
    "int32",
    "int64",
    "int8",
    "rune",
    "string",
    "uint",
    "uint16",
    "uint32",
    "uint64",
    "uint8",
    "uintptr",
];
const JAVA_KEYWORDS: &[&str] = &[
    "abstract",
    "assert",
    "boolean",
    "break",
    "byte",
    "case",
    "catch",
    "char",
    "class",
    "const",
    "continue",
    "default",
    "do",
    "double",
    "else",
    "enum",
    "extends",
    "final",
    "finally",
    "float",
    "for",
    "goto",
    "if",
    "implements",
    "import",
    "instanceof",
    "int",
    "interface",
    "long",
    "native",
    "new",
    "null",
    "package",
    "private",
    "protected",
    "public",
    "return",
    "short",
    "static",
    "strictfp",
    "super",
    "switch",
    "synchronized",
    "this",
    "throw",
    "throws",
    "transient",
    "try",
    "void",
    "volatile",
    "while",
];
const KOTLIN_KEYWORDS: &[&str] = &[
    "as",
    "break",
    "class",
    "continue",
    "do",
    "else",
    "false",
    "for",
    "fun",
    "if",
    "import",
    "in",
    "interface",
    "is",
    "null",
    "object",
    "package",
    "return",
    "super",
    "this",
    "throw",
    "true",
    "try",
    "typealias",
    "typeof",
    "val",
    "var",
    "when",
    "while",
];
const CSHARP_KEYWORDS: &[&str] = &[
    "abstract",
    "as",
    "base",
    "bool",
    "break",
    "byte",
    "case",
    "catch",
    "char",
    "checked",
    "class",
    "const",
    "continue",
    "decimal",
    "default",
    "delegate",
    "do",
    "double",
    "else",
    "enum",
    "event",
    "explicit",
    "extern",
    "false",
    "finally",
    "fixed",
    "float",
    "for",
    "foreach",
    "goto",
    "if",
    "implicit",
    "in",
    "int",
    "interface",
    "internal",
    "is",
    "lock",
    "long",
    "namespace",
    "new",
    "null",
    "object",
    "operator",
    "out",
    "override",
    "params",
    "private",
    "protected",
    "public",
    "readonly",
    "ref",
    "return",
    "sealed",
    "short",
    "sizeof",
    "stackalloc",
    "static",
    "string",
    "struct",
    "switch",
    "this",
    "throw",
    "true",
    "try",
    "typeof",
    "uint",
    "ulong",
    "unchecked",
    "unsafe",
    "ushort",
    "using",
    "virtual",
    "void",
    "volatile",
    "while",
];
const JS_KEYWORDS: &[&str] = &[
    "async",
    "await",
    "break",
    "case",
    "catch",
    "class",
    "const",
    "continue",
    "debugger",
    "default",
    "delete",
    "do",
    "else",
    "export",
    "extends",
    "false",
    "finally",
    "for",
    "function",
    "if",
    "import",
    "in",
    "instanceof",
    "let",
    "new",
    "null",
    "return",
    "static",
    "super",
    "switch",
    "this",
    "throw",
    "true",
    "try",
    "typeof",
    "undefined",
    "var",
    "void",
    "while",
    "yield",
];
const PYTHON_KEYWORDS: &[&str] = &[
    "False", "None", "True", "and", "as", "assert", "async", "await", "break", "class", "continue",
    "def", "del", "elif", "else", "except", "finally", "for", "from", "global", "if", "import",
    "in", "is", "lambda", "nonlocal", "not", "or", "pass", "raise", "return", "try", "while",
    "with", "yield",
];
const RUBY_KEYWORDS: &[&str] = &[
    "BEGIN", "END", "alias", "and", "begin", "break", "case", "class", "def", "defined?", "do",
    "else", "elsif", "end", "ensure", "false", "for", "if", "in", "module", "next", "nil", "not",
    "or", "redo", "rescue", "retry", "return", "self", "super", "then", "true", "undef", "unless",
    "until", "when", "while", "yield",
];
const PHP_KEYWORDS: &[&str] = &[
    "array",
    "as",
    "break",
    "case",
    "catch",
    "class",
    "clone",
    "const",
    "continue",
    "declare",
    "default",
    "do",
    "echo",
    "else",
    "elseif",
    "empty",
    "endforeach",
    "endif",
    "endswitch",
    "endwhile",
    "extends",
    "false",
    "foreach",
    "function",
    "global",
    "if",
    "implements",
    "include",
    "instanceof",
    "interface",
    "isset",
    "list",
    "namespace",
    "new",
    "null",
    "private",
    "protected",
    "public",
    "require",
    "return",
    "static",
    "switch",
    "throw",
    "trait",
    "true",
    "try",
    "unset",
    "use",
    "var",
    "while",
    "yield",
];
const SHELL_KEYWORDS: &[&str] = &[
    "case", "do", "done", "elif", "else", "esac", "fi", "for", "function", "if", "in", "select",
    "then", "until", "while",
];
const SQL_KEYWORDS: &[&str] = &[
    "add", "all", "alter", "and", "as", "asc", "between", "by", "case", "create", "delete", "desc",
    "distinct", "drop", "else", "end", "exists", "from", "full", "group", "having", "in", "inner",
    "insert", "into", "is", "join", "left", "like", "limit", "not", "null", "on", "or", "order",
    "outer", "right", "select", "set", "table", "then", "union", "update", "values", "when",
    "where",
];
const SQL_TYPES: &[&str] = &[
    "bigint",
    "bool",
    "boolean",
    "char",
    "date",
    "decimal",
    "double",
    "float",
    "int",
    "integer",
    "numeric",
    "text",
    "timestamp",
    "varchar",
];

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(line: &str, language: Language) -> Vec<(usize, usize, Kind)> {
        highlight(line, language, &mut Carry::default())
            .into_iter()
            .map(|span| (span.start, span.end, span.kind))
            .collect()
    }

    #[test]
    fn rust_colors_keywords_strings_and_numbers_but_not_words_inside_them() {
        let spans = kinds("fn main() { let n = 1; s(\"fn\") }", Language::Rust);
        assert!(spans.contains(&(0, 2, Kind::Keyword)));
        assert!(!spans.contains(&(3, 7, Kind::Keyword)));
        assert!(spans.iter().any(|span| span.2 == Kind::Number
            && &"fn main() { let n = 1; s(\"fn\") }"[span.0..span.1] == "1"));
        let string = spans.iter().find(|span| span.2 == Kind::String).unwrap();
        assert_eq!(
            &"fn main() { let n = 1; s(\"fn\") }"[string.0..string.1],
            "\"fn\""
        );
    }

    #[test]
    fn comments_hide_keywords_and_block_comments_cross_lines() {
        assert!(
            kinds("// fn let", Language::Rust)
                .iter()
                .all(|span| span.2 == Kind::Comment)
        );
        let mut carry = Carry::default();
        let first = highlight("/* fn", Language::Rust, &mut carry);
        assert_eq!(
            first,
            vec![Span {
                start: 0,
                end: 5,
                kind: Kind::Comment
            }]
        );
        assert!(carry.block_end.is_some());
        let second = highlight("still */ let x", Language::Rust, &mut carry);
        assert_eq!(second[0].kind, Kind::Comment);
        assert!(second.iter().any(|span| span.kind == Kind::Keyword));
        assert!(carry.block_end.is_none());
    }

    #[test]
    fn sql_keywords_ignore_case() {
        let spans = kinds("SELECT name FROM t", Language::Sql);
        assert!(spans.iter().any(
            |span| span.2 == Kind::Keyword && &"SELECT name FROM t"[span.0..span.1] == "SELECT"
        ));
        assert!(
            spans
                .iter()
                .any(|span| span.2 == Kind::Keyword
                    && &"SELECT name FROM t"[span.0..span.1] == "FROM")
        );
    }

    #[test]
    fn language_follows_the_file_extension() {
        assert_eq!(language_for("src/main.rs"), Some(Language::Rust));
        assert_eq!(language_for("Dockerfile"), Some(Language::Shell));
        assert_eq!(language_for("notes.txt"), None);
    }

    #[test]
    fn keyword_lists_are_sorted_and_unique() {
        for language in Language::ALL {
            let grammar = grammar(language);
            for words in [grammar.keywords, grammar.types] {
                let mut sorted = words.to_vec();
                sorted.sort_unstable();
                sorted.dedup();
                assert_eq!(words, sorted.as_slice(), "{language:?}");
            }
        }
    }
}
