//! Tokens of the Python subset, with Python's rules for lines and
//! indentation: a logical line ends at a newline outside brackets, a
//! backslash joins lines, blank and comment-only lines are skipped, and a
//! change of indentation gives `Indent`/`Dedent` tokens.

use crate::Unsupported;

/// A token.
#[derive(Clone, Debug, PartialEq)]
pub enum Tok {
    /// an identifier or keyword
    Name(String),
    /// an integer literal
    Int(i64),
    /// a floating-point literal
    Float(f64),
    /// a string literal (adjacent literals already joined)
    Str(String),
    /// an operator or delimiter
    Op(&'static str),
    /// the end of a logical line
    Newline,
    /// one more level of indentation
    Indent,
    /// one level less
    Dedent,
    /// the end of the source
    End,
}

/// A token and the line it starts on (1-based).
#[derive(Clone, Debug, PartialEq)]
pub struct Token {
    /// the token
    pub tok: Tok,
    /// its line
    pub line: usize,
}

// longest first, so that `**=` is not read as `**` and `=`
const OPS: [&str; 47] = [
    "**=", "//=", ">>=", "<<=", "...", "->", "**", "//", "==", "!=", "<=", ">=", "+=", "-=", "*=",
    "/=", "%=", "&=", "|=", "^=", ":=", "<<", ">>", "+", "-", "*", "/", "%", "<", ">", "=", "(",
    ")", "[", "]", "{", "}", ",", ":", ".", ";", "@", "&", "|", "^", "~", "!",
];

fn unsupported(line: usize, what: impl Into<String>) -> Unsupported {
    Unsupported { line, what: what.into() }
}

/// The tokens of `src`.
pub fn tokens(src: &str) -> Result<Vec<Token>, Unsupported> {
    let chars: Vec<char> = src.chars().collect();
    let mut out: Vec<Token> = vec![];
    let mut indents: Vec<usize> = vec![0];
    let mut depth = 0usize; // open brackets
    let mut i = 0usize;
    let mut line = 1usize;
    let mut at_line_start = true;
    let n = chars.len();
    while i < n {
        if at_line_start && depth == 0 {
            // measure the indentation of a new logical line
            let mut col = 0usize;
            let mut j = i;
            while j < n && (chars[j] == ' ' || chars[j] == '\t' || chars[j] == '\x0c') {
                col += 1;
                j += 1;
            }
            // blank or comment-only lines do not count
            if j >= n || chars[j] == '\n' || chars[j] == '#' || chars[j] == '\r' {
                while j < n && chars[j] != '\n' {
                    j += 1;
                }
                i = j;
                if i < n {
                    i += 1;
                    line += 1;
                }
                continue;
            }
            // Python's rules for tabs (and form feeds) in indentation are
            // subtle (TabError): such a script is left to Python
            if chars[i..j].iter().any(|c| *c != ' ') {
                return Err(unsupported(line, "tabs in indentation"));
            }
            if chars[j] == '\\' && chars.get(j + 1) == Some(&'\n') {
                return Err(unsupported(line, "a line that starts with a backslash"));
            }
            let top = *indents.last().expect("the base level");
            if col > top {
                indents.push(col);
                out.push(Token { tok: Tok::Indent, line });
            } else {
                while col < *indents.last().expect("the base level") {
                    indents.pop();
                    out.push(Token { tok: Tok::Dedent, line });
                }
                if col != *indents.last().expect("the base level") {
                    return Err(unsupported(line, "an unindent that matches no outer level"));
                }
            }
            i = j;
            at_line_start = false;
            continue;
        }
        let c = chars[i];
        match c {
            ' ' | '\t' | '\x0c' | '\r' => i += 1,
            '#' => {
                while i < n && chars[i] != '\n' {
                    i += 1;
                }
            }
            '\\' => {
                // an explicit line join
                let mut j = i + 1;
                if j < n && chars[j] == '\r' {
                    j += 1;
                }
                if j < n && chars[j] == '\n' {
                    i = j + 1;
                    line += 1;
                } else {
                    return Err(unsupported(line, "a backslash outside a string"));
                }
            }
            '\n' => {
                if depth == 0 {
                    if !matches!(out.last().map(|t| &t.tok), Some(Tok::Newline) | None) {
                        out.push(Token { tok: Tok::Newline, line });
                    }
                    at_line_start = true;
                }
                i += 1;
                line += 1;
            }
            '0'..='9' => {
                let (tok, len) = number(&chars[i..], line)?;
                out.push(Token { tok, line });
                i += len;
            }
            '.' if chars.get(i + 1).is_some_and(|d| d.is_ascii_digit()) => {
                let (tok, len) = number(&chars[i..], line)?;
                out.push(Token { tok, line });
                i += len;
            }
            '\'' | '"' => {
                let start_line = line;
                let (s, len, lines) = string(&chars[i..], line, false)?;
                line += lines;
                push_str(&mut out, s, start_line);
                i += len;
            }
            c if c.is_alphabetic() || c == '_' => {
                let mut j = i;
                while j < n && (chars[j].is_alphanumeric() || chars[j] == '_') {
                    j += 1;
                }
                let word: String = chars[i..j].iter().collect();
                // string prefixes
                if j < n && (chars[j] == '\'' || chars[j] == '"') {
                    let lower = word.to_ascii_lowercase();
                    match lower.as_str() {
                        "r" | "u" => {
                            let start_line = line;
                            let (s, len, lines) = string(&chars[j..], line, lower == "r")?;
                            line += lines;
                            push_str(&mut out, s, start_line);
                            i = j + len;
                            continue;
                        }
                        "f" | "b" | "rb" | "br" | "fr" | "rf" => {
                            return Err(unsupported(
                                line,
                                format!("{lower}-strings (only plain strings are)"),
                            ));
                        }
                        _ => {}
                    }
                }
                if !word.is_ascii() {
                    return Err(unsupported(
                        line,
                        format!("the name '{word}' (only ASCII names are)"),
                    ));
                }
                out.push(Token { tok: Tok::Name(word), line });
                i = j;
            }
            _ => {
                let rest: String = chars[i..(i + 3).min(n)].iter().collect();
                let Some(op) = OPS.iter().find(|o| rest.starts_with(**o)) else {
                    return Err(unsupported(line, format!("the character '{c}'")));
                };
                match *op {
                    "(" | "[" | "{" => depth += 1,
                    ")" | "]" | "}" => {
                        if depth == 0 {
                            return Err(unsupported(line, format!("an unmatched '{op}'")));
                        }
                        depth -= 1;
                    }
                    _ => {}
                }
                out.push(Token { tok: Tok::Op(op), line });
                i += op.len();
            }
        }
    }
    if depth > 0 {
        return Err(unsupported(line, "an unclosed bracket"));
    }
    if !matches!(out.last().map(|t| &t.tok), Some(Tok::Newline) | None) {
        out.push(Token { tok: Tok::Newline, line });
    }
    while indents.len() > 1 {
        indents.pop();
        out.push(Token { tok: Tok::Dedent, line });
    }
    out.push(Token { tok: Tok::End, line });
    Ok(out)
}

/// Adjacent string literals are one (`'a' 'b'` is `'ab'`).
fn push_str(out: &mut Vec<Token>, s: String, line: usize) {
    if let Some(Token { tok: Tok::Str(prev), .. }) = out.last_mut() {
        prev.push_str(&s);
    } else {
        out.push(Token { tok: Tok::Str(s), line });
    }
}

/// A number literal at the start of `c`: the token and its length.
fn number(c: &[char], line: usize) -> Result<(Tok, usize), Unsupported> {
    let n = c.len();
    // 0x, 0o, 0b
    if c[0] == '0' && n > 1 && matches!(c[1], 'x' | 'X' | 'o' | 'O' | 'b' | 'B') {
        let radix = match c[1] {
            'x' | 'X' => 16,
            'o' | 'O' => 8,
            _ => 2,
        };
        let mut j = 2;
        let mut digits = String::new();
        while j < n && (c[j].is_digit(radix) || c[j] == '_') {
            if c[j] == '_' {
                // single underscores between digits (or after the prefix)
                let before = j == 2 || c[j - 1].is_digit(radix);
                let after = c.get(j + 1).is_some_and(|d| d.is_digit(radix));
                if !(before && after) {
                    return Err(unsupported(line, "a malformed number"));
                }
            } else {
                digits.push(c[j]);
            }
            j += 1;
        }
        if j < n && (c[j].is_alphanumeric() || c[j] == '.') {
            return Err(unsupported(line, "a malformed number"));
        }
        let v = i64::from_str_radix(&digits, radix)
            .map_err(|_| unsupported(line, "an integer too large for 64 bits"))?;
        return Ok((Tok::Int(v), j));
    }
    let mut j = 0;
    let mut text = String::new();
    let mut is_float = false;
    // digits with single underscores between them; false on a misplaced one
    let digits = |j: &mut usize, text: &mut String| -> bool {
        while *j < n && (c[*j].is_ascii_digit() || c[*j] == '_') {
            if c[*j] == '_' {
                let before = *j > 0 && c[*j - 1].is_ascii_digit();
                let after = c.get(*j + 1).is_some_and(|d| d.is_ascii_digit());
                if !(before && after) {
                    return false;
                }
            } else {
                text.push(c[*j]);
            }
            *j += 1;
        }
        true
    };
    let mut ok = digits(&mut j, &mut text);
    if j < n && c[j] == '.' {
        is_float = true;
        text.push('.');
        j += 1;
        if c.get(j) == Some(&'_') {
            ok = false;
        }
        ok &= digits(&mut j, &mut text);
    }
    if j < n && (c[j] == 'e' || c[j] == 'E') {
        let mut k = j + 1;
        let mut exp = String::from("e");
        if k < n && (c[k] == '+' || c[k] == '-') {
            exp.push(c[k]);
            k += 1;
        }
        if k < n && c[k].is_ascii_digit() {
            is_float = true;
            j = k;
            text.push_str(&exp);
            ok &= digits(&mut j, &mut text);
        }
    }
    if !ok {
        return Err(unsupported(line, "a malformed number"));
    }
    if j < n && (c[j] == 'j' || c[j] == 'J') {
        return Err(unsupported(line, "complex numbers"));
    }
    if j < n && (c[j].is_alphanumeric() || c[j] == '_') {
        return Err(unsupported(line, "a malformed number"));
    }
    if is_float {
        // Rust's and Python's parsing both round correctly
        let v: f64 = text.parse().map_err(|_| unsupported(line, "a malformed number"))?;
        Ok((Tok::Float(v), j))
    } else {
        if text.len() > 1 && text.starts_with('0') && text.chars().any(|d| d != '0') {
            return Err(unsupported(line, "an integer with leading zeros"));
        }
        let v: i64 =
            text.parse().map_err(|_| unsupported(line, "an integer too large for 64 bits"))?;
        Ok((Tok::Int(v), j))
    }
}

/// A string literal at the start of `c` (at its quote): its value, its
/// length and the newlines it spans.
fn string(c: &[char], line: usize, raw: bool) -> Result<(String, usize, usize), Unsupported> {
    let q = c[0];
    let triple = c.len() >= 3 && c[1] == q && c[2] == q;
    let mut j = if triple { 3 } else { 1 };
    let mut s = String::new();
    let mut lines = 0;
    loop {
        let Some(&ch) = c.get(j) else {
            return Err(unsupported(line, "an unterminated string"));
        };
        if ch == q {
            if !triple {
                return Ok((s, j + 1, lines));
            }
            if c.get(j + 1) == Some(&q) && c.get(j + 2) == Some(&q) {
                return Ok((s, j + 3, lines));
            }
        }
        if ch == '\n' {
            if !triple {
                return Err(unsupported(line, "an unterminated string"));
            }
            lines += 1;
        }
        if ch == '\\' {
            let Some(&e) = c.get(j + 1) else {
                return Err(unsupported(line, "an unterminated string"));
            };
            if raw {
                s.push('\\');
                s.push(e);
                if e == '\n' {
                    lines += 1;
                }
                j += 2;
                continue;
            }
            let v = match e {
                '\n' => {
                    lines += 1;
                    j += 2;
                    continue;
                }
                '\\' => '\\',
                '\'' => '\'',
                '"' => '"',
                'n' => '\n',
                't' => '\t',
                'r' => '\r',
                '0' if !c.get(j + 2).is_some_and(|d| d.is_digit(8)) => '\0',
                _ => {
                    return Err(unsupported(line, format!("the string escape '\\{e}'")));
                }
            };
            s.push(v);
            j += 2;
            continue;
        }
        s.push(ch);
        j += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn toks(s: &str) -> Vec<Tok> {
        tokens(s).unwrap().into_iter().map(|t| t.tok).collect()
    }

    #[test]
    fn lines_and_indentation() {
        let t = toks("if x:\n    y = 1  # c\n\n    z = (1,\n 2)\nw = 3\n");
        use Tok::*;
        assert_eq!(
            t,
            vec![
                Name("if".into()),
                Name("x".into()),
                Op(":"),
                Newline,
                Indent,
                Name("y".into()),
                Op("="),
                Int(1),
                Newline,
                Name("z".into()),
                Op("="),
                Op("("),
                Int(1),
                Op(","),
                Int(2),
                Op(")"),
                Newline,
                Dedent,
                Name("w".into()),
                Op("="),
                Int(3),
                Newline,
                End
            ]
        );
    }

    #[test]
    fn numbers_and_strings() {
        use Tok::*;
        assert_eq!(
            toks("1_000 0.5 .5 5. 1e3 2E-2 0x1F"),
            vec![
                Int(1000),
                Float(0.5),
                Float(0.5),
                Float(5.0),
                Float(1000.0),
                Float(0.02),
                Int(31),
                Newline,
                End
            ]
        );
        assert_eq!(toks("'a' \"b\" '''c\nd'''"), vec![Str("abc\nd".into()), Newline, End]);
        assert_eq!(toks(r"'\n\t\\'"), vec![Str("\n\t\\".into()), Newline, End]);
        assert!(tokens("f'x'").is_err());
        assert!(tokens("1j").is_err());
        assert!(tokens("x = 012").is_err());
        assert!(tokens("99999999999999999999").is_err());
    }
}
