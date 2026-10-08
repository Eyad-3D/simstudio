//! Tokens of the text format and of Base Modelica (one lexer for both).

use crate::{LangError, Span};

/// A token's kind and content.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Tok {
    /// a plain identifier or keyword (`model`, `R`, `der`)
    Ident(String),
    /// a quoted identifier, without its quotes (`'R1.v'` → `R1.v`)
    QIdent(String),
    /// a number literal: its value
    Num(f64),
    /// a string literal, escapes resolved
    Str(String),
    /// punctuation or an operator
    Sym(&'static str),
    /// the end of the text
    Eof,
}

/// A token with its place in the text (byte offsets).
#[derive(Clone, Debug)]
pub(crate) struct Token {
    pub tok: Tok,
    pub start: usize,
    pub end: usize,
}

/// Line starts of a text, to turn byte offsets into lines and columns.
pub(crate) struct SourceMap<'a> {
    text: &'a str,
    line_starts: Vec<usize>,
}

impl<'a> SourceMap<'a> {
    pub fn new(text: &'a str) -> Self {
        let mut line_starts = vec![0];
        for (i, b) in text.bytes().enumerate() {
            if b == b'\n' {
                line_starts.push(i + 1);
            }
        }
        SourceMap { text, line_starts }
    }

    fn line_col(&self, offset: usize) -> (u32, u32) {
        let offset = offset.min(self.text.len());
        let line = match self.line_starts.binary_search(&offset) {
            Ok(i) => i,
            Err(i) => i - 1,
        };
        let start = self.line_starts[line];
        let col = self.text[start..offset].chars().count() + 1;
        (line as u32 + 1, col as u32)
    }

    /// The span of the bytes `start..end`.
    pub fn span(&self, start: usize, end: usize) -> Span {
        let (line, col) = self.line_col(start);
        let (end_line, end_col) = self.line_col(end.max(start));
        Span { line, col, end_line, end_col, start, end: end.max(start) }
    }

    /// The text of a span, its runs of white space (and comments' line
    /// breaks) folded to single spaces, for quoting in messages.
    pub fn quote(&self, span: Span) -> String {
        let raw = &self.text[span.start.min(self.text.len())..span.end.min(self.text.len())];
        raw.split_whitespace().collect::<Vec<_>>().join(" ")
    }
}

const SYMBOLS: [&str; 28] = [
    ".+", ".-", ".*", "./", ".^", ":=", "<=", ">=", "==", "<>", "(", ")", "{", "}", "[", "]", ",",
    ";", ":", ".", "=", "+", "-", "*", "/", "^", "<", ">",
];

fn is_ident_start(c: char) -> bool {
    c.is_ascii_alphabetic() || c == '_'
}

fn is_ident_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

/// Splits `text` into tokens; the last one is [`Tok::Eof`].
pub(crate) fn lex(text: &str, map: &SourceMap) -> Result<Vec<Token>, LangError> {
    let bytes = text.as_bytes();
    let mut out = vec![];
    let mut i = 0;
    let err = |start: usize, end: usize, code: &'static str, msg: String| {
        LangError::new(code, map.span(start, end), msg)
    };
    while i < text.len() {
        let c = text[i..].chars().next().expect("in bounds");
        if c.is_whitespace() {
            i += c.len_utf8();
            continue;
        }
        // comments
        if text[i..].starts_with("//") {
            while i < text.len() && bytes[i] != b'\n' {
                i += 1;
            }
            continue;
        }
        if text[i..].starts_with("/*") {
            match text[i + 2..].find("*/") {
                Some(k) => i += 2 + k + 2,
                None => {
                    return Err(err(
                        i,
                        i + 2,
                        "COMMENT-OPEN",
                        "this comment is never closed: a '*/' is missing".into(),
                    ));
                }
            }
            continue;
        }
        let start = i;
        // identifiers and keywords
        if is_ident_start(c) {
            while i < text.len() && is_ident_char(bytes[i] as char) {
                i += 1;
            }
            out.push(Token { tok: Tok::Ident(text[start..i].to_string()), start, end: i });
            continue;
        }
        // quoted identifiers
        if c == '\'' {
            i += 1;
            let mut name = String::new();
            loop {
                let Some(ch) = text[i..].chars().next() else {
                    return Err(err(
                        start,
                        start + 1,
                        "QUOTE-OPEN",
                        "this quoted name is never closed: a closing ' is missing".into(),
                    ));
                };
                if ch == '\n' {
                    return Err(err(
                        start,
                        i,
                        "QUOTE-OPEN",
                        "this quoted name runs to the end of the line: a closing ' is missing"
                            .into(),
                    ));
                }
                i += ch.len_utf8();
                match ch {
                    '\'' => break,
                    '\\' => {
                        let Some(e) = text[i..].chars().next() else { continue };
                        i += e.len_utf8();
                        name.push(e);
                    }
                    other => name.push(other),
                }
            }
            if name.is_empty() {
                return Err(err(start, i, "QUOTE-EMPTY", "a quoted name cannot be empty".into()));
            }
            out.push(Token { tok: Tok::QIdent(name), start, end: i });
            continue;
        }
        // strings
        if c == '"' {
            i += 1;
            let mut s = String::new();
            loop {
                let Some(ch) = text[i..].chars().next() else {
                    return Err(err(
                        start,
                        start + 1,
                        "STRING-OPEN",
                        "this text in quotes is never closed: a closing \" is missing".into(),
                    ));
                };
                i += ch.len_utf8();
                match ch {
                    '"' => break,
                    '\\' => {
                        let Some(e) = text[i..].chars().next() else { continue };
                        i += e.len_utf8();
                        s.push(match e {
                            'n' => '\n',
                            't' => '\t',
                            'r' => '\r',
                            other => other,
                        });
                    }
                    other => s.push(other),
                }
            }
            out.push(Token { tok: Tok::Str(s), start, end: i });
            continue;
        }
        // numbers: digits [. digits] [e [+-] digits]
        if c.is_ascii_digit() || (c == '.' && bytes.get(i + 1).is_some_and(u8::is_ascii_digit)) {
            while i < text.len() && bytes[i].is_ascii_digit() {
                i += 1;
            }
            if i < text.len()
                && bytes[i] == b'.'
                && !bytes.get(i + 1).is_some_and(|b| b"+-*/^".contains(b))
            {
                i += 1;
                while i < text.len() && bytes[i].is_ascii_digit() {
                    i += 1;
                }
            }
            if i < text.len() && (bytes[i] == b'e' || bytes[i] == b'E') {
                let save = i;
                i += 1;
                if i < text.len() && (bytes[i] == b'+' || bytes[i] == b'-') {
                    i += 1;
                }
                let digits = i;
                while i < text.len() && bytes[i].is_ascii_digit() {
                    i += 1;
                }
                if i == digits {
                    i = save;
                    return Err(err(
                        start,
                        i + 1,
                        "NUMBER",
                        format!(
                            "'{}' is not a number: the exponent after 'e' has no digits",
                            &text[start..(i + 1).min(text.len())]
                        ),
                    ));
                }
            }
            if i < text.len() && is_ident_char(bytes[i] as char) {
                let mut j = i;
                while j < text.len() && is_ident_char(bytes[j] as char) {
                    j += 1;
                }
                return Err(err(
                    start,
                    j,
                    "NUMBER",
                    format!(
                        "'{}' is not a number: a number cannot run into letters (units go in \
                         the declaration, as unit = \"...\")",
                        &text[start..j]
                    ),
                ));
            }
            let v: f64 = text[start..i].parse().map_err(|_| {
                err(start, i, "NUMBER", format!("'{}' is not a number", &text[start..i]))
            })?;
            out.push(Token { tok: Tok::Num(v), start, end: i });
            continue;
        }
        // punctuation and operators, longest first
        if let Some(sym) = SYMBOLS.iter().find(|s| text[i..].starts_with(**s)) {
            i += sym.len();
            out.push(Token { tok: Tok::Sym(sym), start, end: i });
            continue;
        }
        for (bad, good) in [("!=", "'<>'"), ("&&", "'and'"), ("||", "'or'"), ("!", "'not'")] {
            if text[i..].starts_with(bad) {
                return Err(err(
                    start,
                    start + bad.len(),
                    "OPERATOR",
                    format!("'{bad}' is not an operator here: write {good}"),
                ));
            }
        }
        let what = match c {
            '#' | '$' | '&' | '|' | '!' | '?' | '~' | '%' | '`' => format!("'{c}'"),
            other => format!("the character '{other}'"),
        };
        return Err(err(
            start,
            start + c.len_utf8(),
            "CHARACTER",
            format!("{what} cannot appear here"),
        ));
    }
    out.push(Token { tok: Tok::Eof, start: text.len(), end: text.len() });
    Ok(out)
}

/// Words that cannot be used as names unless quoted.
pub(crate) const KEYWORDS: [&str; 61] = [
    "algorithm",
    "and",
    "annotation",
    "block",
    "break",
    "class",
    "connect",
    "connector",
    "constant",
    "constrainedby",
    "der",
    "discrete",
    "each",
    "else",
    "elseif",
    "elsewhen",
    "encapsulated",
    "end",
    "enumeration",
    "equation",
    "expandable",
    "extends",
    "external",
    "false",
    "final",
    "flow",
    "for",
    "function",
    "if",
    "import",
    "impure",
    "in",
    "initial",
    "inner",
    "input",
    "loop",
    "model",
    "not",
    "operator",
    "or",
    "outer",
    "output",
    "package",
    "parameter",
    "partial",
    "protected",
    "public",
    "pure",
    "record",
    "redeclare",
    "replaceable",
    "return",
    "stream",
    "then",
    "true",
    "type",
    "when",
    "while",
    "within",
    "structural",
    "time",
];

/// Whether `name` may be written without quotes.
pub(crate) fn plain_ident(name: &str) -> bool {
    let mut chars = name.chars();
    matches!(chars.next(), Some(c) if is_ident_start(c))
        && chars.all(is_ident_char)
        && !KEYWORDS.contains(&name)
}
