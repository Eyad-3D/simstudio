//! A recursive-descent parser for the text format and Base Modelica: the
//! Modelica grammar without arrays, inheritance or connectors' internals,
//! plus the text format's `connector p: Pin` and `structural parameter`.
//! Every node keeps its span; a syntax error stops parsing with one
//! [`LangError`] that says, in plain words, what is missing or out of
//! place.

use crate::ast::*;
use crate::lexer::{KEYWORDS, SourceMap, Tok, Token};
use crate::{LangError, Span};

pub(crate) type PResult<T> = Result<T, LangError>;

pub(crate) struct Parser<'a> {
    toks: Vec<Token>,
    pos: usize,
    map: &'a SourceMap<'a>,
}

const CLASS_WORDS: [&str; 8] =
    ["model", "block", "connector", "record", "function", "type", "package", "class"];

fn describe(t: &Tok) -> String {
    match t {
        Tok::Ident(s) => format!("'{s}'"),
        Tok::QIdent(s) => format!("the name '{s}'"),
        Tok::Num(v) => format!("the number {v}"),
        Tok::Str(s) => {
            let short: String = s.chars().take(30).collect();
            let more = if s.chars().count() > 30 { "…" } else { "" };
            format!("the text \"{short}{more}\"")
        }
        Tok::Sym(s) => format!("'{s}'"),
        Tok::Eof => "the end of the text".into(),
    }
}

impl<'a> Parser<'a> {
    pub fn new(toks: Vec<Token>, map: &'a SourceMap<'a>) -> Self {
        Parser { toks, pos: 0, map }
    }

    // ---- token helpers ----

    fn peek(&self) -> &Tok {
        &self.toks[self.pos].tok
    }

    fn peek_at(&self, k: usize) -> &Tok {
        let i = (self.pos + k).min(self.toks.len() - 1);
        &self.toks[i].tok
    }

    fn cur(&self) -> &Token {
        &self.toks[self.pos]
    }

    fn cur_span(&self) -> Span {
        let t = self.cur();
        self.map.span(t.start, t.end)
    }

    fn start(&self) -> usize {
        self.cur().start
    }

    fn prev_end(&self) -> usize {
        if self.pos == 0 { 0 } else { self.toks[self.pos - 1].end }
    }

    fn span_from(&self, start: usize) -> Span {
        self.map.span(start, self.prev_end().max(start))
    }

    fn bump(&mut self) -> Token {
        let t = self.toks[self.pos].clone();
        if self.pos + 1 < self.toks.len() {
            self.pos += 1;
        }
        t
    }

    fn is_kw(&self, kw: &str) -> bool {
        matches!(self.peek(), Tok::Ident(s) if s == kw)
    }

    fn is_kw_at(&self, k: usize, kw: &str) -> bool {
        matches!(self.peek_at(k), Tok::Ident(s) if s == kw)
    }

    fn is_sym(&self, s: &str) -> bool {
        matches!(self.peek(), Tok::Sym(x) if *x == s)
    }

    fn eat_kw(&mut self, kw: &str) -> bool {
        if self.is_kw(kw) {
            self.bump();
            true
        } else {
            false
        }
    }

    fn eat_sym(&mut self, s: &str) -> bool {
        if self.is_sym(s) {
            self.bump();
            true
        } else {
            false
        }
    }

    fn found(&self) -> String {
        describe(self.peek())
    }

    /// An error at the current token.
    pub fn err_here(&self, code: &'static str, msg: String) -> LangError {
        LangError::new(code, self.cur_span(), msg)
    }

    /// An error just after the previous token (where something is missing).
    fn err_missing(&self, code: &'static str, msg: String) -> LangError {
        let at = self.prev_end();
        if matches!(self.peek(), Tok::Eof) || self.pos == 0 {
            return self.err_here(code, msg);
        }
        LangError::new(code, self.map.span(at, at), msg)
    }

    fn expect_sym(&mut self, s: &str, context: &str) -> PResult<()> {
        if self.eat_sym(s) {
            return Ok(());
        }
        Err(self.err_missing(
            "SYNTAX",
            format!("a '{s}' is missing {context} (found {})", self.found()),
        ))
    }

    fn expect_kw(&mut self, kw: &str, context: &str) -> PResult<()> {
        if self.eat_kw(kw) {
            return Ok(());
        }
        Err(self
            .err_here("SYNTAX", format!("'{kw}' is missing {context} (found {})", self.found())))
    }

    /// An identifier (plain, not a keyword, or quoted).
    fn ident(&mut self, what: &str) -> PResult<(String, Span)> {
        let span = self.cur_span();
        match self.peek().clone() {
            Tok::Ident(s) if !KEYWORDS.contains(&s.as_str()) => {
                self.bump();
                Ok((s, span))
            }
            Tok::QIdent(s) => {
                self.bump();
                Ok((s, span))
            }
            Tok::Ident(s) => Err(self.err_here(
                "KEYWORD-NAME",
                format!(
                    "'{s}' is a reserved word and cannot be {what}; choose another name, or put \
                     it in single quotes ('{s}')"
                ),
            )),
            other => Err(self.err_here(
                "SYNTAX",
                format!("{what} is missing here (found {})", describe(&other)),
            )),
        }
    }

    /// A dotted name `A.B.c` (each part plain or quoted).
    fn dotted(&mut self, what: &str) -> PResult<(Vec<String>, Span)> {
        let start = self.start();
        let mut parts = vec![self.ident(what)?.0];
        while self.is_sym(".") && matches!(self.peek_at(1), Tok::Ident(_) | Tok::QIdent(_)) {
            self.bump();
            parts.push(self.ident(what)?.0);
        }
        Ok((parts, self.span_from(start)))
    }

    fn no_subscripts(&self, what: &str) -> PResult<()> {
        if self.is_sym("[") {
            return Err(self.err_here(
                "ARRAY",
                format!(
                    "{what} has an array subscript '[': arrays are not supported (the text \
                     format and the Base Modelica subset hold scalars only)"
                ),
            ));
        }
        Ok(())
    }

    /// A description: `"text"` or `"a" + "b"`, if present.
    fn doc(&mut self) -> PResult<Option<String>> {
        let Tok::Str(s) = self.peek().clone() else { return Ok(None) };
        self.bump();
        let mut s = s;
        while self.is_sym("+") && matches!(self.peek_at(1), Tok::Str(_)) {
            self.bump();
            if let Tok::Str(more) = self.bump().tok {
                s.push_str(&more);
            }
        }
        Ok(Some(s))
    }

    /// `annotation(…)`, if present.
    fn annotation(&mut self) -> PResult<Vec<ModArg>> {
        if !self.eat_kw("annotation") {
            return Ok(vec![]);
        }
        self.expect_sym("(", "after 'annotation'")?;
        self.mod_args("the annotation")
    }

    // ---- classes ----

    /// The whole text: a list of definitions.
    pub fn file(&mut self) -> PResult<Vec<ClassDef>> {
        let mut out = vec![];
        if self.eat_kw("within") {
            if !self.is_sym(";") {
                self.dotted("a package name after 'within'")?;
            }
            self.expect_sym(";", "after the 'within' clause")?;
        }
        while !matches!(self.peek(), Tok::Eof) {
            out.push(self.class_def()?);
        }
        Ok(out)
    }

    fn class_kind(&mut self) -> PResult<ClassKind> {
        for (w, k) in [
            ("model", ClassKind::Model),
            ("block", ClassKind::Block),
            ("connector", ClassKind::Connector),
            ("record", ClassKind::Record),
            ("function", ClassKind::Function),
            ("type", ClassKind::Type),
            ("package", ClassKind::Package),
            ("class", ClassKind::Class),
        ] {
            if self.eat_kw(w) {
                return Ok(k);
            }
        }
        for w in ["operator", "expandable", "optimization"] {
            if self.is_kw(w) {
                return Err(
                    self.err_here("UNSUPPORTED", format!("'{w}' definitions are not supported"))
                );
            }
        }
        Err(self.err_here(
            "SYNTAX",
            format!(
                "a definition starts with 'model', 'connector', 'type', 'record', 'function' or \
                 'package' (found {})",
                self.found()
            ),
        ))
    }

    fn class_def(&mut self) -> PResult<ClassDef> {
        let start = self.start();
        if self.is_kw("encapsulated") {
            return Err(self.err_here("UNSUPPORTED", "'encapsulated' is not supported".into()));
        }
        self.eat_kw("partial");
        self.eat_kw("final");
        if self.is_kw("pure") || self.is_kw("impure") {
            self.bump();
        }
        let kind = self.class_kind()?;
        let (parts, name_span) = self.dotted(&format!("the {}'s name", kind.word()))?;
        let name = parts.join(".");
        let mut def = ClassDef {
            kind,
            name: name.clone(),
            name_span,
            span: name_span,
            doc: None,
            elements: vec![],
            classes: vec![],
            equations: vec![],
            initial_equations: vec![],
            algorithm: vec![],
            annotation: vec![],
            short: None,
        };
        if self.eat_sym("=") {
            def.short = Some(self.short_class(&name)?);
            def.doc = self.doc()?;
            def.annotation = self.annotation()?;
            self.expect_sym(";", &format!("after the definition of '{name}'"))?;
            def.span = self.span_from(start);
            return Ok(def);
        }
        def.doc = self.doc()?;
        self.composition(&mut def)?;
        // end NAME ;
        let end_span = self.cur_span();
        self.bump(); // 'end'
        let (end_parts, end_name_span) = match self.peek() {
            Tok::Ident(_) | Tok::QIdent(_) => self.dotted("the name after 'end'")?,
            _ => {
                return Err(self.err_here(
                    "END-NAME",
                    format!(
                        "'end' must be followed by the name of the {} it closes: 'end {name};' \
                         (found {})",
                        kind.word(),
                        self.found()
                    ),
                ));
            }
        };
        let end_name = end_parts.join(".");
        if end_name != name {
            return Err(LangError::new(
                "END-NAME",
                end_name_span,
                format!(
                    "the {} '{name}' is closed by 'end {end_name};': the two names must be the \
                     same",
                    kind.word()
                ),
            ));
        }
        let _ = end_span;
        self.expect_sym(";", &format!("after 'end {name}'"))?;
        def.span = self.span_from(start);
        Ok(def)
    }

    fn short_class(&mut self, name: &str) -> PResult<Short> {
        if self.eat_kw("enumeration") {
            self.expect_sym("(", "after 'enumeration'")?;
            let mut lits = vec![];
            if self.is_sym(":") {
                return Err(self.err_here(
                    "UNSUPPORTED",
                    format!("the enumeration '{name}' must list its options"),
                ));
            }
            loop {
                let (lit, span) = self.ident("an option of the enumeration")?;
                let doc = self.doc()?;
                self.annotation()?;
                lits.push((lit, doc, span));
                if self.eat_sym(",") {
                    continue;
                }
                self.expect_sym(")", &format!("after the options of '{name}'"))?;
                break;
            }
            return Ok(Short::Enumeration(lits));
        }
        let (base, base_span) = self.dotted(&format!("the type '{name}' stands for"))?;
        self.no_subscripts(&format!("the type '{name}'"))?;
        let mods =
            if self.eat_sym("(") { self.mod_args(&format!("the type '{name}'"))? } else { vec![] };
        Ok(Short::Alias { base, base_span, mods })
    }

    fn at_class_def(&self) -> bool {
        let mut k = 0;
        while ["partial", "encapsulated", "final", "pure", "impure"]
            .iter()
            .any(|w| self.is_kw_at(k, w))
        {
            k += 1;
        }
        let Tok::Ident(w) = self.peek_at(k) else { return false };
        if !CLASS_WORDS.contains(&w.as_str()) {
            return false;
        }
        // `connector p: Pin` declares a port, not a connector type
        !(w == "connector" && matches!(self.peek_at(k + 2), Tok::Sym(":")))
    }

    fn composition(&mut self, def: &mut ClassDef) -> PResult<()> {
        let mut protected = false;
        loop {
            match self.peek() {
                Tok::Eof => {
                    return Err(self.err_here(
                        "END-MISSING",
                        format!(
                            "the text ends inside the {} '{}': 'end {};' is missing",
                            def.kind.word(),
                            def.name,
                            def.name
                        ),
                    ));
                }
                Tok::Ident(w) if w == "end" => return Ok(()),
                _ => {}
            }
            if self.eat_kw("public") {
                protected = false;
                continue;
            }
            if self.eat_kw("protected") {
                protected = true;
                continue;
            }
            if self.is_kw("equation") || (self.is_kw("initial") && self.is_kw_at(1, "equation")) {
                let initial = self.eat_kw("initial");
                self.bump();
                let mut eqs = vec![];
                while !self.at_section_end() {
                    let e = self.equation()?;
                    self.expect_sym(";", "after the equation")?;
                    eqs.push(e);
                }
                if initial {
                    def.initial_equations.extend(eqs);
                } else {
                    def.equations.extend(eqs);
                }
                continue;
            }
            if self.is_kw("algorithm") || (self.is_kw("initial") && self.is_kw_at(1, "algorithm")) {
                if self.is_kw("initial") || def.kind != ClassKind::Function {
                    return Err(self.err_here(
                        "UNSUPPORTED",
                        format!(
                            "algorithm sections are supported in functions only; write the {} \
                             '{}' with equations",
                            def.kind.word(),
                            def.name
                        ),
                    ));
                }
                self.bump();
                while !self.at_section_end() {
                    let s = self.statement()?;
                    self.expect_sym(";", "after the statement")?;
                    def.algorithm.push(s);
                }
                continue;
            }
            if self.is_kw("annotation") {
                def.annotation.extend(self.annotation()?);
                self.expect_sym(";", "after the annotation")?;
                continue;
            }
            for (w, why) in [
                (
                    "extends",
                    "'extends' is not supported: a definition here lists everything it has; \
                     compose parts as sub-components instead",
                ),
                ("import", "'import' is not supported: write names in full"),
                (
                    "external",
                    "external functions are not supported; write the function's algorithm",
                ),
            ] {
                if self.is_kw(w) {
                    return Err(self.err_here("UNSUPPORTED", why.into()));
                }
            }
            if self.at_class_def() {
                def.classes.push(self.class_def()?);
                continue;
            }
            let els = self.element(protected)?;
            def.elements.extend(els);
        }
    }

    fn at_section_end(&self) -> bool {
        match self.peek() {
            Tok::Eof => true,
            Tok::Ident(w) => {
                matches!(
                    w.as_str(),
                    "end" | "equation" | "algorithm" | "annotation" | "public" | "protected"
                ) || (w == "initial"
                    && (self.is_kw_at(1, "equation") || self.is_kw_at(1, "algorithm")))
            }
            _ => false,
        }
    }

    // ---- declarations ----

    fn element(&mut self, protected: bool) -> PResult<Vec<Element>> {
        let start = self.start();
        // `connector p: Pin "doc";`
        if self.is_kw("connector") {
            self.bump();
            let (name, name_span) = self.ident("the port's name")?;
            self.expect_sym(":", &format!("after the port name '{name}'"))?;
            let (ty, type_span) = self.dotted("the port's connector type")?;
            self.no_subscripts(&format!("the port '{name}'"))?;
            let doc = self.doc()?;
            let annotation = self.annotation()?;
            self.expect_sym(";", &format!("after the declaration of '{name}'"))?;
            return Ok(vec![Element {
                span: self.span_from(start),
                prefixes: Prefixes::default(),
                port: true,
                protected,
                type_name: ty,
                type_span,
                name,
                name_span,
                mods: vec![],
                binding: None,
                doc,
                annotation,
            }]);
        }
        let mut p = Prefixes::default();
        while let Tok::Ident(w) = self.peek().clone() {
            match w.as_str() {
                "final" | "each" => {}
                "structural" => p.structural = true,
                "parameter" => p.parameter = true,
                "constant" => p.constant = true,
                "discrete" => p.discrete = true,
                "input" => p.input = true,
                "output" => p.output = true,
                "flow" => p.flow = true,
                "stream" | "inner" | "outer" | "replaceable" | "redeclare" => {
                    return Err(self
                        .err_here("UNSUPPORTED", format!("'{w}' declarations are not supported")));
                }
                _ => break,
            }
            self.bump();
        }
        if p.structural && !p.parameter {
            return Err(self.err_missing(
                "SYNTAX",
                format!("'structural' must be followed by 'parameter' (found {})", self.found()),
            ));
        }
        let (ty, type_span) = match self.peek() {
            Tok::Ident(w) if KEYWORDS.contains(&w.as_str()) => {
                return Err(self.err_here(
                    "SYNTAX",
                    format!(
                        "a declaration, an 'equation' section or 'end' is expected here (found {})",
                        self.found()
                    ),
                ));
            }
            Tok::Ident(_) | Tok::QIdent(_) => self.dotted("the type")?,
            _ => {
                return Err(self.err_here(
                    "SYNTAX",
                    format!(
                        "a declaration, an 'equation' section or 'end' is expected here (found {})",
                        self.found()
                    ),
                ));
            }
        };
        self.no_subscripts("the type")?;
        let mut out = vec![];
        loop {
            let decl_start = self.start();
            let (name, name_span) = self.ident("the name being declared")?;
            self.no_subscripts(&format!("'{name}'"))?;
            let mods =
                if self.eat_sym("(") { self.mod_args(&format!("'{name}'"))? } else { vec![] };
            let binding =
                if self.eat_sym("=") || self.eat_sym(":=") { Some(self.expr()?) } else { None };
            let doc = self.doc()?;
            let annotation = self.annotation()?;
            let span =
                self.map.span(if out.is_empty() { start } else { decl_start }, self.prev_end());
            out.push(Element {
                span,
                prefixes: p.clone(),
                port: false,
                protected,
                type_name: ty.clone(),
                type_span,
                name,
                name_span,
                mods,
                binding,
                doc,
                annotation,
            });
            if self.eat_sym(",") {
                continue;
            }
            let last = &out.last().expect("one at least").name;
            let context = format!("after the declaration of '{last}'");
            if matches!(self.peek(), Tok::Ident(_) | Tok::QIdent(_)) && !self.at_section_end() {
                return Err(self.err_missing(
                    "SYNTAX",
                    format!("a ';' is missing {context} (found {})", self.found()),
                ));
            }
            self.expect_sym(";", &context)?;
            return Ok(out);
        }
    }

    /// Modification arguments after '(' up to and including ')'.
    fn mod_args(&mut self, what: &str) -> PResult<Vec<ModArg>> {
        let mut out = vec![];
        if self.eat_sym(")") {
            return Ok(out);
        }
        loop {
            let start = self.start();
            while self.eat_kw("each") || self.eat_kw("final") {}
            if self.is_kw("redeclare") || self.is_kw("replaceable") {
                return Err(self.err_here("UNSUPPORTED", "redeclarations are not supported".into()));
            }
            let (name, _) = self.dotted(&format!("a modifier name in {what}"))?;
            self.no_subscripts(&format!("the modifier '{}'", name.join(".")))?;
            let mods = if self.eat_sym("(") { self.mod_args(what)? } else { vec![] };
            let value =
                if self.eat_sym("=") || self.eat_sym(":=") { Some(self.expr()?) } else { None };
            self.doc()?;
            out.push(ModArg { name, span: self.span_from(start), mods, value });
            if self.eat_sym(",") {
                continue;
            }
            if self.eat_sym(")") {
                return Ok(out);
            }
            return Err(self.err_missing(
                "SYNTAX",
                format!("a ',' or ')' is missing in {what} (found {})", self.found()),
            ));
        }
    }

    // ---- equations and statements ----

    fn equation(&mut self) -> PResult<Equation> {
        let start = self.start();
        if self.eat_kw("if") {
            let mut branches = vec![];
            let mut cond = self.expr()?;
            loop {
                self.expect_kw("then", "after the condition of 'if'")?;
                let body = self.equations_until(&["elseif", "else", "end"])?;
                branches.push((cond, body));
                if self.eat_kw("elseif") {
                    cond = self.expr()?;
                    continue;
                }
                break;
            }
            let otherwise =
                if self.eat_kw("else") { self.equations_until(&["end"])? } else { vec![] };
            self.close("if", start)?;
            let span = self.span_from(start);
            let doc = self.doc()?;
            self.annotation()?;
            return Ok(Equation { kind: EqKind::If(branches, otherwise), span, doc });
        }
        if self.eat_kw("when") {
            let mut branches = vec![];
            let mut cond = self.expr()?;
            let mut early_doc = None;
            loop {
                self.expect_kw("then", "after the condition of 'when'")?;
                if branches.is_empty() {
                    early_doc = self.doc()?;
                }
                let body = self.equations_until(&["elsewhen", "end"])?;
                branches.push((cond, body));
                if self.eat_kw("elsewhen") {
                    cond = self.expr()?;
                    continue;
                }
                break;
            }
            self.close("when", start)?;
            let span = self.span_from(start);
            let doc = self.doc()?.or(early_doc);
            self.annotation()?;
            return Ok(Equation { kind: EqKind::When(branches), span, doc });
        }
        if self.is_kw("for") {
            return Err(self.err_here(
                "UNSUPPORTED",
                "'for' loops are not supported (scalars only): write each equation".into(),
            ));
        }
        if self.eat_kw("connect") {
            self.expect_sym("(", "after 'connect'")?;
            let a = self.cref_expr("the first port of connect")?;
            self.expect_sym(",", "between the two ports of connect")?;
            let b = self.cref_expr("the second port of connect")?;
            self.expect_sym(")", "after the two ports of connect")?;
            let span = self.span_from(start);
            let doc = self.doc()?;
            self.annotation()?;
            return Ok(Equation { kind: EqKind::Connect(a, b), span, doc });
        }
        let lhs = self.simple_expr()?;
        if let ExprKind::Bin(BinOp::Eq, ..) = lhs.kind
            && !self.is_sym("=")
        {
            return Err(LangError::new(
                "SYNTAX",
                lhs.span,
                "an equation is written with a single '=' ('==' compares two values)".into(),
            ));
        }
        let kind = if self.eat_sym("=") {
            let rhs = self.expr()?;
            EqKind::Simple(lhs, rhs)
        } else if self.is_sym(":=") {
            return Err(self.err_here(
                "SYNTAX",
                "':=' assigns in algorithms; an equation is written with '='".into(),
            ));
        } else if matches!(lhs.kind, ExprKind::Call(..)) {
            EqKind::Call(lhs)
        } else {
            return Err(self.err_missing(
                "SYNTAX",
                format!("an equation needs '=' between its two sides (found {})", self.found()),
            ));
        };
        let span = self.span_from(start);
        let doc = self.doc()?;
        self.annotation()?;
        Ok(Equation { kind, span, doc })
    }

    /// `end if` / `end when`, with a plain message when it is missing.
    fn close(&mut self, kw: &str, start: usize) -> PResult<()> {
        let line = self.map.span(start, start).line;
        if self.is_kw("end") && self.is_kw_at(1, kw) {
            self.bump();
            self.bump();
            return Ok(());
        }
        let found = if self.is_kw("end") {
            format!(
                "'end {}'",
                match self.peek_at(1) {
                    Tok::Ident(s) | Tok::QIdent(s) => s.clone(),
                    other => describe(other),
                }
            )
        } else {
            self.found()
        };
        Err(self.err_here(
            "SYNTAX",
            format!("the '{kw}' that starts on line {line} is not closed: 'end {kw};' is missing (found {found})"),
        ))
    }

    fn equations_until(&mut self, stops: &[&str]) -> PResult<Vec<Equation>> {
        let mut out = vec![];
        while !stops.iter().any(|w| self.is_kw(w)) {
            if matches!(self.peek(), Tok::Eof) {
                return Err(self.err_here(
                    "END-MISSING",
                    format!("the text ends before '{}'", stops.last().copied().unwrap_or("end")),
                ));
            }
            let e = self.equation()?;
            self.expect_sym(";", "after the equation")?;
            out.push(e);
        }
        Ok(out)
    }

    fn cref_expr(&mut self, what: &str) -> PResult<Expr> {
        let start = self.start();
        let (parts, _) = self.dotted(what)?;
        self.no_subscripts(what)?;
        Ok(Expr { kind: ExprKind::Ref(parts), span: self.span_from(start) })
    }

    fn statement(&mut self) -> PResult<Stmt> {
        let start = self.start();
        if self.eat_kw("if") {
            let mut branches = vec![];
            let mut cond = self.expr()?;
            loop {
                self.expect_kw("then", "after the condition of 'if'")?;
                let body = self.statements_until(&["elseif", "else", "end"])?;
                branches.push((cond, body));
                if self.eat_kw("elseif") {
                    cond = self.expr()?;
                    continue;
                }
                break;
            }
            let otherwise =
                if self.eat_kw("else") { self.statements_until(&["end"])? } else { vec![] };
            self.expect_kw("end", "to close the 'if' statement")?;
            self.expect_kw("if", "after 'end' (an 'if' statement closes with 'end if')")?;
            return Ok(Stmt {
                kind: StmtKind::If(branches, otherwise),
                span: self.span_from(start),
            });
        }
        if self.eat_kw("return") {
            return Ok(Stmt { kind: StmtKind::Return, span: self.span_from(start) });
        }
        for w in ["for", "while", "when", "break"] {
            if self.is_kw(w) {
                return Err(self.err_here(
                    "UNSUPPORTED",
                    format!("'{w}' is not supported in functions here: use assignments and 'if'"),
                ));
            }
        }
        let target = self.simple_expr()?;
        if self.eat_sym(":=") {
            let value = self.expr()?;
            self.doc()?;
            return Ok(Stmt { kind: StmtKind::Assign(target, value), span: self.span_from(start) });
        }
        if self.is_sym("=") {
            return Err(self.err_here(
                "SYNTAX",
                "in an algorithm a value is assigned with ':=' (an '=' is for equations)".into(),
            ));
        }
        if matches!(target.kind, ExprKind::Call(..)) {
            return Ok(Stmt { kind: StmtKind::Call(target), span: self.span_from(start) });
        }
        Err(self.err_missing(
            "SYNTAX",
            format!("':=' is missing in the statement (found {})", self.found()),
        ))
    }

    fn statements_until(&mut self, stops: &[&str]) -> PResult<Vec<Stmt>> {
        let mut out = vec![];
        while !stops.iter().any(|w| self.is_kw(w)) {
            if matches!(self.peek(), Tok::Eof) {
                return Err(
                    self.err_here("END-MISSING", "the text ends inside an 'if' statement".into())
                );
            }
            let s = self.statement()?;
            self.expect_sym(";", "after the statement")?;
            out.push(s);
        }
        Ok(out)
    }

    // ---- expressions ----

    pub fn expr(&mut self) -> PResult<Expr> {
        if self.is_kw("if") {
            return self.if_expr();
        }
        self.simple_expr()
    }

    fn if_expr(&mut self) -> PResult<Expr> {
        let start = self.start();
        self.bump(); // if
        let mut branches = vec![];
        let mut cond = self.expr()?;
        loop {
            self.expect_kw("then", "after the condition of 'if'")?;
            let value = self.expr()?;
            branches.push((cond, value));
            if self.eat_kw("elseif") {
                cond = self.expr()?;
                continue;
            }
            break;
        }
        if !self.eat_kw("else") {
            return Err(self.err_here(
                "SYNTAX",
                format!(
                    "an if-expression needs an 'else' part: 'if c then a else b' (found {})",
                    self.found()
                ),
            ));
        }
        let otherwise = self.expr()?;
        Ok(Expr { kind: ExprKind::If(branches, Box::new(otherwise)), span: self.span_from(start) })
    }

    fn simple_expr(&mut self) -> PResult<Expr> {
        let e = self.logical_expr()?;
        if self.is_sym(":") {
            return Err(
                self.err_here("ARRAY", "ranges 'a:b' are not supported (scalars only)".into())
            );
        }
        Ok(e)
    }

    fn bin(&self, op: BinOp, a: Expr, b: Expr, start: usize) -> Expr {
        Expr { kind: ExprKind::Bin(op, Box::new(a), Box::new(b)), span: self.span_from(start) }
    }

    fn logical_expr(&mut self) -> PResult<Expr> {
        let start = self.start();
        let mut e = self.logical_term()?;
        while self.eat_kw("or") {
            let r = self.logical_term()?;
            e = self.bin(BinOp::Or, e, r, start);
        }
        Ok(e)
    }

    fn logical_term(&mut self) -> PResult<Expr> {
        let start = self.start();
        let mut e = self.logical_factor()?;
        while self.eat_kw("and") {
            let r = self.logical_factor()?;
            e = self.bin(BinOp::And, e, r, start);
        }
        Ok(e)
    }

    fn logical_factor(&mut self) -> PResult<Expr> {
        let start = self.start();
        if self.eat_kw("not") {
            let r = self.relation()?;
            return Ok(Expr { kind: ExprKind::Not(Box::new(r)), span: self.span_from(start) });
        }
        self.relation()
    }

    fn relation(&mut self) -> PResult<Expr> {
        let start = self.start();
        let a = self.arith()?;
        let op = match self.peek() {
            Tok::Sym("<") => BinOp::Lt,
            Tok::Sym("<=") => BinOp::Le,
            Tok::Sym(">") => BinOp::Gt,
            Tok::Sym(">=") => BinOp::Ge,
            Tok::Sym("==") => BinOp::Eq,
            Tok::Sym("<>") => BinOp::Ne,
            _ => return Ok(a),
        };
        self.bump();
        let b = self.arith()?;
        if matches!(self.peek(), Tok::Sym("<" | "<=" | ">" | ">=" | "==" | "<>")) {
            return Err(
                self.err_here("SYNTAX", "two comparisons in a row: write 'a < b and b < c'".into())
            );
        }
        Ok(self.bin(op, a, b, start))
    }

    fn arith(&mut self) -> PResult<Expr> {
        let start = self.start();
        let mut e = if self.is_sym("-") || self.is_sym("+") {
            let neg = self.is_sym("-");
            self.bump();
            let literal = matches!(self.peek(), Tok::Num(_));
            let t = self.term()?;
            match (&t.kind, neg) {
                (_, false) => t,
                (ExprKind::Num(v), true) if literal => {
                    Expr { kind: ExprKind::Num(-v), span: self.span_from(start) }
                }
                _ => Expr { kind: ExprKind::Neg(Box::new(t)), span: self.span_from(start) },
            }
        } else {
            self.term()?
        };
        loop {
            let op = match self.peek() {
                Tok::Sym("+" | ".+") => BinOp::Add,
                Tok::Sym("-" | ".-") => BinOp::Sub,
                _ => return Ok(e),
            };
            self.bump();
            if self.is_sym("-") || self.is_sym("+") {
                return Err(self.err_here(
                    "SYNTAX",
                    "two signs in a row: put the second in parentheses, as 'a - (-b)'".into(),
                ));
            }
            let r = self.term()?;
            e = self.bin(op, e, r, start);
        }
    }

    fn term(&mut self) -> PResult<Expr> {
        let start = self.start();
        let mut e = self.factor()?;
        loop {
            let op = match self.peek() {
                Tok::Sym("*" | ".*") => BinOp::Mul,
                Tok::Sym("/" | "./") => BinOp::Div,
                _ => return Ok(e),
            };
            self.bump();
            if self.is_sym("-") || self.is_sym("+") {
                return Err(self.err_here(
                    "SYNTAX",
                    "a sign after '*' or '/': put the signed value in parentheses, as 'a * (-b)'"
                        .into(),
                ));
            }
            let r = self.factor()?;
            e = self.bin(op, e, r, start);
        }
    }

    fn factor(&mut self) -> PResult<Expr> {
        let start = self.start();
        let base = self.primary()?;
        if self.is_sym("^") || self.is_sym(".^") {
            self.bump();
            if self.is_sym("-") || self.is_sym("+") {
                return Err(self.err_here(
                    "SYNTAX",
                    "a sign after '^': put the exponent in parentheses, as 'x ^ (-2)'".into(),
                ));
            }
            let exp = self.primary()?;
            if self.is_sym("^") || self.is_sym(".^") {
                return Err(self.err_here(
                    "SYNTAX",
                    "'a ^ b ^ c' is ambiguous: add parentheses, as '(a ^ b) ^ c' or 'a ^ (b ^ c)'"
                        .into(),
                ));
            }
            return Ok(self.bin(BinOp::Pow, base, exp, start));
        }
        Ok(base)
    }

    fn primary(&mut self) -> PResult<Expr> {
        let start = self.start();
        let span = self.cur_span();
        match self.peek().clone() {
            Tok::Num(v) => {
                self.bump();
                Ok(Expr { kind: ExprKind::Num(v), span })
            }
            Tok::Str(s) => {
                self.bump();
                Ok(Expr { kind: ExprKind::Str(s), span })
            }
            Tok::Sym("(") => {
                self.bump();
                let e = self.expr()?;
                if self.is_sym(",") {
                    return Err(self.err_here(
                        "UNSUPPORTED",
                        "a list of values in parentheses '(a, b)' is not supported".into(),
                    ));
                }
                self.expect_sym(")", "to close the '('")?;
                Ok(Expr { kind: ExprKind::Paren(Box::new(e)), span: self.span_from(start) })
            }
            Tok::Sym("{") => {
                self.bump();
                let mut items = vec![];
                if !self.eat_sym("}") {
                    loop {
                        items.push(self.expr()?);
                        if self.eat_sym(",") {
                            continue;
                        }
                        self.expect_sym("}", "to close the '{'")?;
                        break;
                    }
                }
                Ok(Expr { kind: ExprKind::Array(items), span: self.span_from(start) })
            }
            Tok::Sym("[") => {
                self.bump();
                let mut rows = vec![vec![]];
                loop {
                    rows.last_mut().expect("a row").push(self.expr()?);
                    if self.eat_sym(",") {
                        continue;
                    }
                    if self.eat_sym(";") {
                        rows.push(vec![]);
                        continue;
                    }
                    self.expect_sym("]", "to close the '['")?;
                    break;
                }
                Ok(Expr { kind: ExprKind::Matrix(rows), span: self.span_from(start) })
            }
            Tok::Ident(w) if w == "true" || w == "false" => {
                self.bump();
                Ok(Expr { kind: ExprKind::Bool(w == "true"), span })
            }
            Tok::Ident(w) if w == "if" => self.if_expr(),
            Tok::Ident(w) if w == "der" || w == "initial" || w == "pure" || w == "time" => {
                self.bump();
                if self.is_sym("(") {
                    let args = self.call_args(&w)?;
                    return Ok(Expr {
                        kind: ExprKind::Call(vec![w], args),
                        span: self.span_from(start),
                    });
                }
                if w == "time" {
                    return Ok(Expr { kind: ExprKind::Ref(vec![w]), span });
                }
                Err(self.err_missing(
                    "SYNTAX",
                    format!("'{w}' must be followed by '(' (found {})", self.found()),
                ))
            }
            Tok::Ident(w) if KEYWORDS.contains(&w.as_str()) => Err(self.err_here(
                "SYNTAX",
                format!("a value is missing here (found the reserved word '{w}')"),
            )),
            Tok::Ident(_) | Tok::QIdent(_) => {
                let (parts, _) = self.dotted("a name")?;
                self.no_subscripts(&format!("'{}'", parts.join(".")))?;
                if self.is_sym("(") {
                    let args = self.call_args(&parts.join("."))?;
                    return Ok(Expr {
                        kind: ExprKind::Call(parts, args),
                        span: self.span_from(start),
                    });
                }
                Ok(Expr { kind: ExprKind::Ref(parts), span: self.span_from(start) })
            }
            Tok::Sym(s) if matches!(s, ")" | ";" | "," | "}" | "]" | "=") => {
                Err(self.err_here("SYNTAX", format!("a value is missing before '{s}'")))
            }
            Tok::Eof => {
                Err(self.err_here("SYNTAX", "the text ends where a value is expected".into()))
            }
            other => Err(self.err_here(
                "SYNTAX",
                format!("a value is expected here (found {})", describe(&other)),
            )),
        }
    }

    fn call_args(&mut self, fname: &str) -> PResult<Args> {
        self.expect_sym("(", &format!("after '{fname}'"))?;
        let mut args = Args::default();
        if self.eat_sym(")") {
            return Ok(args);
        }
        loop {
            let named = matches!(self.peek(), Tok::Ident(_) | Tok::QIdent(_))
                && matches!(self.peek_at(1), Tok::Sym("="));
            if named {
                let (n, s) = self.ident("an argument name")?;
                self.bump(); // =
                let v = self.expr()?;
                args.named.push((n, s, v));
            } else {
                if !args.named.is_empty() {
                    return Err(self.err_here(
                        "SYNTAX",
                        format!("in the call of '{fname}', unnamed arguments must come before named ones"),
                    ));
                }
                if self.is_kw("function") {
                    return Err(self.err_here(
                        "UNSUPPORTED",
                        "functions as arguments are not supported".into(),
                    ));
                }
                args.positional.push(self.expr()?);
            }
            if self.eat_sym(",") {
                continue;
            }
            if self.is_kw("for") {
                return Err(self.err_here(
                    "UNSUPPORTED",
                    "array comprehensions ('for' in a call) are not supported".into(),
                ));
            }
            self.expect_sym(")", &format!("to close the call of '{fname}'"))?;
            return Ok(args);
        }
    }
}
