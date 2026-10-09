//! Malformed texts: each gives an error with a place and a plain message.

use lsim_lang::{LangError, parse, parse_with};

/// A resistor written correctly, for the cases to break.
const GOOD: &str = r#"model R "A resistor."
  connector p: Pin "positive";
  connector n: Pin "negative";
  parameter Real R(unit = "Ohm") = 1 "resistance";
  Real v(unit = "V");
  Real i(unit = "A");
equation
  v = p.v - n.v;
  0 = p.i + n.i;
  i = p.i;
  v = R * i "Ohm's law";
end R;
"#;

/// (what is wrong, the text, the error's code, line, column, the start of
/// its message)
type Case = (&'static str, &'static str, &'static str, u32, u32, &'static str);

const CASES: &[Case] = &[
    // lexical
    (
        "unclosed string",
        "model M \"doc\nend M;",
        "STRING-OPEN",
        1,
        9,
        "this text in quotes is never closed: a closing",
    ),
    (
        "unclosed comment",
        "model M /* note\nend M;",
        "COMMENT-OPEN",
        1,
        9,
        "this comment is never closed: a '*/' is missing",
    ),
    (
        "unclosed quoted name",
        "model M\n  Real 'x(unit = \"V\");\nend M;",
        "QUOTE-OPEN",
        2,
        8,
        "this quoted name runs to the end of the line: a",
    ),
    (
        "stray character",
        "model M\n  Real x # 2;\nend M;",
        "CHARACTER",
        2,
        10,
        "'#' cannot appear here",
    ),
    (
        "C-style not-equal",
        "model M\n  Real x;\nequation\n  x = if x != 1 then 1 else 2;\nend M;",
        "OPERATOR",
        4,
        12,
        "'!=' is not an operator here: write '<>'",
    ),
    (
        "number running into letters",
        "model M\n  parameter Real R(unit = \"Ohm\") = 5Ohm;\nend M;",
        "NUMBER",
        2,
        36,
        "'5Ohm' is not a number: a number cannot run",
    ),
    (
        "exponent without digits",
        "model M\n  parameter Real R(unit = \"Ohm\") = 1e;\nend M;",
        "NUMBER",
        2,
        36,
        "'1e' is not a number: the exponent after 'e'",
    ),
    // structure of definitions
    (
        "no definition keyword",
        "resistor R\nend R;",
        "SYNTAX",
        1,
        1,
        "a definition starts with 'model', 'connector',",
    ),
    (
        "missing end",
        "model M\n  Real x(unit = \"V\");\n",
        "END-MISSING",
        3,
        1,
        "the text ends inside the model 'M': 'end M;' is",
    ),
    (
        "end name differs",
        "model M\nend N;",
        "END-NAME",
        2,
        5,
        "the model 'M' is closed by 'end N;': the two",
    ),
    (
        "end without name",
        "model M\nend;",
        "END-NAME",
        2,
        4,
        "'end' must be followed by the name of the model",
    ),
    (
        "missing semicolon after end",
        "model M\nend M",
        "SYNTAX",
        2,
        6,
        "a ';' is missing after 'end M' (found the end",
    ),
    (
        "missing semicolon after declaration",
        "model M\n  Real x(unit = \"V\")\n  Real y(unit = \"V\");\nend M;",
        "SYNTAX",
        2,
        21,
        "a ';' is missing after the declaration of 'x'",
    ),
    (
        "keyword as a name",
        "model M\n  Real end(unit = \"V\");\nend M;",
        "KEYWORD-NAME",
        2,
        8,
        "'end' is a reserved word and cannot be the name",
    ),
    (
        "extends",
        "model M\n  extends Base;\nend M;",
        "UNSUPPORTED",
        2,
        3,
        "'extends' is not supported: a definition here",
    ),
    (
        "array declaration",
        "model M\n  Real x[3](unit = \"V\");\nend M;",
        "ARRAY",
        2,
        9,
        "'x' has an array subscript '[': arrays are not",
    ),
    (
        "structural without parameter",
        "model M\n  structural Real x(unit = \"V\");\nend M;",
        "SYNTAX",
        2,
        13,
        "'structural' must be followed by 'parameter'",
    ),
    (
        "modifier list not closed",
        "model M\n  parameter Real R(unit = \"Ohm\" = 1;\nend M;",
        "SYNTAX",
        2,
        32,
        "a ',' or ')' is missing in 'R' (found '=')",
    ),
    (
        "port without type",
        "model M\n  connector p: ;\nend M;",
        "SYNTAX",
        2,
        16,
        "the port's connector type is missing here",
    ),
    (
        "algorithm in a model",
        "model M\n  Real x;\nalgorithm\n  x := 1;\nend M;",
        "UNSUPPORTED",
        3,
        1,
        "algorithm sections are supported in functions",
    ),
    // equations
    (
        "equation without equals",
        "model M\n  Real x;\nequation\n  x + 1;\nend M;",
        "SYNTAX",
        4,
        8,
        "an equation needs '=' between its two sides",
    ),
    (
        "double equals",
        "model M\n  Real x;\nequation\n  x == 1;\nend M;",
        "SYNTAX",
        4,
        3,
        "an equation is written with a single '=' ('=='",
    ),
    (
        "assignment in equations",
        "model M\n  Real x;\nequation\n  x := 1;\nend M;",
        "SYNTAX",
        4,
        5,
        "':=' assigns in algorithms; an equation is",
    ),
    (
        "missing semicolon after equation",
        "model M\n  Real x;\nequation\n  x = 1\n  x = 2;\nend M;",
        "SYNTAX",
        4,
        8,
        "a ';' is missing after the equation (found 'x')",
    ),
    (
        "when not closed",
        "model M\n  discrete Real x;\nequation\n  when time > 1 then\n    x = 1;\nend M;",
        "SYNTAX",
        6,
        1,
        "the 'when' that starts on line 4 is not closed:",
    ),
    (
        "when closed by end if",
        "model M\n  discrete Real x;\nequation\n  when time > 1 then\n    x = 1;\n  end if;\nend M;",
        "SYNTAX",
        6,
        3,
        "the 'when' that starts on line 4 is not closed:",
    ),
    (
        "if equation without then",
        "model M\n  Real x;\nequation\n  if time > 1\n    x = 1;\n  else\n    x = 2;\n  end if;\nend M;",
        "SYNTAX",
        5,
        5,
        "'then' is missing after the condition of 'if'",
    ),
    (
        "if branches of different sizes",
        "model M\n  Real x;\n  Real y;\nequation\n  if time > 1 then\n    x = 1;\n    y = 1;\n  else\n    x = 2;\n  end if;\nend M;",
        "IF-EQUATION",
        5,
        3,
        "every branch of an 'if' equation must have as",
    ),
    (
        "for loop",
        "model M\n  Real x;\nequation\n  for k in 1:3 loop\n    x = k;\n  end for;\nend M;",
        "UNSUPPORTED",
        4,
        3,
        "'for' loops are not supported (scalars only):",
    ),
    (
        "connect with one port",
        "model M\n  connector p: Pin;\nequation\n  connect(p);\nend M;",
        "SYNTAX",
        4,
        12,
        "a ',' is missing between the two ports of",
    ),
    // expressions
    (
        "unbalanced parenthesis",
        "model M\n  Real x;\nequation\n  x = (1 + 2;\nend M;",
        "SYNTAX",
        4,
        13,
        "a ')' is missing to close the '(' (found ';')",
    ),
    (
        "missing operand",
        "model M\n  Real x;\nequation\n  x = 1 + ;\nend M;",
        "SYNTAX",
        4,
        11,
        "a value is missing before ';'",
    ),
    (
        "two signs in a row",
        "model M\n  Real x;\nequation\n  x = 1 - -2;\nend M;",
        "SYNTAX",
        4,
        11,
        "two signs in a row: put the second in",
    ),
    (
        "chained power",
        "model M\n  Real x;\nequation\n  x = 2 ^ 3 ^ 2;\nend M;",
        "SYNTAX",
        4,
        13,
        "'a ^ b ^ c' is ambiguous: add parentheses, as",
    ),
    (
        "chained comparison",
        "model M\n  Real x;\nequation\n  x = if 0 < x < 1 then 1 else 0;\nend M;",
        "SYNTAX",
        4,
        16,
        "two comparisons in a row: write 'a < b and b <",
    ),
    (
        "if-expression without else",
        "model M\n  Real x;\nequation\n  x = if time > 1 then 1;\nend M;",
        "SYNTAX",
        4,
        25,
        "an if-expression needs an 'else' part: 'if c",
    ),
    (
        "unknown function",
        "model M\n  Real x;\nequation\n  x = sine(time);\nend M;",
        "UNKNOWN-FUNCTION",
        4,
        7,
        "'sine()' cannot be used: it is not a built-in",
    ),
    (
        "wrong number of arguments",
        "model M\n  Real x;\nequation\n  x = atan2(time);\nend M;",
        "CALL-ARGS",
        4,
        7,
        "atan2() takes 2 arguments, but is given 1",
    ),
    (
        "der of an expression",
        "model M\n  Real x(unit = \"1\");\nequation\n  der(2 * x) = 1;\nend M;",
        "DER-ARG",
        4,
        7,
        "der() applies to a variable's name, not to an",
    ),
    (
        "array subscript in an equation",
        "model M\n  Real x;\nequation\n  x[1] = 1;\nend M;",
        "ARRAY",
        4,
        4,
        "'x' has an array subscript '[': arrays are not",
    ),
    (
        "text as a value",
        "model M\n  Real x;\nequation\n  x = \"one\";\nend M;",
        "TEXT-VALUE",
        4,
        7,
        "text in quotes cannot be a value in an equation",
    ),
    // names and values
    (
        "unknown name",
        "model M\n  Real x(unit = \"V\");\nequation\n  x = y;\nend M;",
        "UNKNOWN-NAME",
        4,
        7,
        "'y' is not a variable, parameter or port of 'M'",
    ),
    (
        "port used as a value",
        "model M\n  connector p: Pin;\n  Real x;\nequation\n  x = p;\nend M;",
        "UNKNOWN-NAME",
        5,
        7,
        "'p' is a port: write one of its quantities, as",
    ),
    (
        "declared twice",
        "model M\n  Real x(unit = \"V\");\n  Real x(unit = \"A\");\nend M;",
        "DUPLICATE",
        3,
        8,
        "'x' is declared twice in 'M' (first on line 2)",
    ),
    (
        "parameter without value",
        "model M\n  parameter Real R(unit = \"Ohm\");\nend M;",
        "PARAM-VALUE",
        2,
        18,
        "the parameter 'R' needs a value, as 'parameter",
    ),
    (
        "Boolean parameter with a number",
        "model M\n  parameter Boolean on = 1;\nend M;",
        "PARAM-VALUE",
        2,
        26,
        "the Boolean parameter 'on' is true or false",
    ),
    (
        "enumeration value of another type",
        "model M\n  type Mode = enumeration(A, B);\n  parameter Mode m = Gear.A;\nend M;",
        "PARAM-VALUE",
        3,
        22,
        "'Gear.A' is not an option of Mode",
    ),
    (
        "unknown enumeration option",
        "model M\n  type Mode = enumeration(A, B);\n  parameter Mode m = Mode.C;\nend M;",
        "PARAM-VALUE",
        3,
        3,
        "'C' is not an option of Mode (A, B)",
    ),
    (
        "parameter value uses a variable",
        "model M\n  parameter Real R(unit = \"Ohm\") = v / 2;\n  Real v(unit = \"Ohm\");\nend M;",
        "PARAM-VALUE",
        2,
        3,
        "the value of the parameter 'R' uses 'v', which",
    ),
    (
        "parameter and part share a name",
        "model M\n  parameter Real r(unit = \"Ohm\") = 1;\n  Electrical.Resistor r(R = r);\nend M;",
        "NAME-CLASH",
        2,
        3,
        "'r' names both a parameter and a part of 'M':",
    ),
    (
        "event assigns a continuous variable",
        "model M\n  Real x(unit = \"1\");\nequation\n  der(x) = 1;\n  when x > 1 then\n    x = 0;\n  end when;\nend M;",
        "WHEN",
        5,
        3,
        "In 'M', “when x > 1 then x = 0; end when”",
    ),
    (
        "unknown attribute",
        "model M\n  Real x(unit = \"V\", colour = \"red\");\nend M;",
        "ATTRIBUTE",
        2,
        22,
        "a variable takes unit, displayUnit, start,",
    ),
    (
        "table with too few values",
        "model M\n  parameter Real t(unit = \"V\") = table(x = {0, 1, 2}, y = {1, 2}, xUnit = \"1\");\nend M;",
        "TABLE",
        2,
        34,
        "this table is not valid: the table has 3 grid",
    ),
    (
        "table with decreasing points",
        "model M\n  parameter Real t(unit = \"V\") = table(x = {0, 2, 1}, y = {1, 2, 3}, xUnit = \"1\");\nend M;",
        "TABLE",
        2,
        34,
        "this table is not valid: the points of its axis",
    ),
    (
        "table read at the wrong number of points",
        "model M\n  parameter Real t(unit = \"V\") = table(x = {0, 1}, y = {1, 2}, xUnit = \"1\");\n  Real v(unit = \"V\");\nequation\n  v = t(1, 2);\nend M;",
        "CALL-ARGS",
        5,
        7,
        "the table 't' has one axis, so it is read at",
    ),
    // units
    (
        "unknown unit",
        "model M\n  Real x(unit = \"furlong\");\nend M;",
        "UNIT-SYNTAX",
        2,
        10,
        "the variable 'x': 'furlong' in the unit",
    ),
    (
        "unit that is not SI",
        "model M\n  parameter Real P(unit = \"kW\") = 1;\nend M;",
        "UNIT-NOT-SI",
        2,
        20,
        "the parameter 'P' is declared in 'kW', which is",
    ),
    (
        "display unit of another quantity",
        "model M\n  Real v(unit = \"m/s\", displayUnit = \"rpm\");\nend M;",
        "UNIT-DISPLAY",
        2,
        3,
        "the variable 'v' is in 'm/s' but shown in",
    ),
    (
        "unbalanced equation",
        "model M\n  Real v(unit = \"V\");\n  Real i(unit = \"A\");\n  parameter Real R(unit = \"Ohm\") = 1;\nequation\n  v = R / i \"Ohm's law, mistyped\";\nend M;",
        "UNIT-MISMATCH",
        6,
        3,
        "In 'M', the equation “v = R / i” does not",
    ),
    (
        "sum of different units",
        "model M\n  Real v(unit = \"V\");\n  Real i(unit = \"A\");\nequation\n  v = 1 + v\n      + i;\nend M;",
        "UNIT-MISMATCH",
        5,
        3,
        "In 'M', the equation “v = 1 + v + i” does not",
    ),
    (
        "exp of a dimensioned value",
        "model M\n  Real v(unit = \"V\");\nequation\n  v = exp(v);\nend M;",
        "UNIT-MISMATCH",
        4,
        3,
        "In 'M', the equation “v = exp(v)” does not",
    ),
    (
        "parameter value in other units",
        "model M\n  parameter Real a(unit = \"m\") = 1;\n  parameter Real b(unit = \"s\") = a;\nend M;",
        "UNIT-MISMATCH",
        3,
        3,
        "In 'M', the value of the parameter 'b' does not",
    ),
    (
        "energy loss not in watts",
        "model M\n  Real v(unit = \"V\");\nequation\n  v = 1;\n  annotation(__LightSim_energy(loss = v));\nend M;",
        "UNIT-MISMATCH",
        5,
        14,
        "In 'M', the loss “v” is not in W: it is in V,",
    ),
];

fn first_error(text: &str) -> LangError {
    match parse(text) {
        Ok(d) => panic!("parsed without an error:\n{text}\n{d:#?}"),
        Err(e) => e[0].clone(),
    }
}

#[test]
fn the_good_resistor_parses() {
    let defs = parse(GOOD).expect("parses");
    assert_eq!(defs[0].equations.len(), 4);
    assert_eq!(defs[0].equations[3].label.as_deref(), Some("Ohm's law"));
}

#[test]
fn every_malformed_text_gives_a_place_and_a_plain_message() {
    assert!(CASES.len() >= 50, "{} cases", CASES.len());
    for (what, text, code, line, col, start) in CASES {
        let e = first_error(text);
        let at = format!("{what}: {e:?}\n{}", e.render(text));
        assert_eq!(e.code, *code, "{at}");
        assert_eq!((e.span.line, e.span.col), (*line, *col), "{at}");
        assert!(e.message.starts_with(start), "{at}");
        // a span lies inside the text and runs forwards
        assert!(e.span.start <= e.span.end && e.span.end <= text.len(), "{at}");
        assert!((e.span.end_line, e.span.end_col) >= (e.span.line, e.span.col), "{at}");
        // plain words: no parser jargon
        for jargon in
            ["token", "expected", "unexpected", "EOF", "identifier", "production", "lexer"]
        {
            assert!(!e.message.contains(jargon), "{at}: says '{jargon}'");
        }
    }
    println!("{} malformed texts, each with a place and a plain message", CASES.len());
}

#[test]
fn a_unit_error_quotes_the_equation_as_written() {
    let text = "model Heater\n  parameter Real R(unit = \"Ohm\") = 2;\n  Real v(unit = \"V\");\n  \
                Real i(unit = \"A\");\nequation\n  v =   R /\n    i \"Ohm's law, mistyped\";\n  i = 1;\nend Heater;";
    let errs = parse(text).expect_err("does not balance");
    assert_eq!(errs.len(), 1, "{errs:?}");
    let e = &errs[0];
    assert_eq!(e.code, "UNIT-MISMATCH");
    assert_eq!(
        e.message,
        "In 'Heater', the equation “v = R / i” does not balance its units: the left side is in \
         V, the right side in m2.kg.s-3.A-3."
    );
    assert_eq!((e.span.line, e.span.col, e.span.end_line, e.span.end_col), (6, 3, 7, 6));
    println!("{}", e.render(text));
}

#[test]
fn the_library_finds_what_the_text_alone_cannot() {
    // alone, the parts and the connector are taken on trust
    let text = "model Divider\n  parameter Real R(unit = \"Ohm\") = 10;\n  \
                Electrical.Resistor r1(R = R);\n  Electrical.Resistor r2(Rx = 2);\n  \
                Electrical.Capacitor c(C = R);\n  Real v(unit = \"V\");\nequation\n  \
                v = r1.v + r2.p.i;\n  connect(r1.n, r2.q);\nend Divider;";
    assert!(parse(text).is_ok());
    let lib = lsim_lib::library();
    let errs = parse_with(text, &lib).expect_err("the library knows better");
    let msgs: Vec<String> = errs
        .iter()
        .map(|e| format!("{}:{} [{}] {}", e.span.line, e.span.col, e.code, e.message))
        .collect();
    let all = msgs.join("\n");
    assert!(
        all.contains(
            "[UNKNOWN-PARAMETER] Electrical.Resistor has no parameter 'Rx' (its parameters: R)"
        ),
        "{all}"
    );
    assert!(all.contains("[UNIT-MISMATCH] In 'Divider', the value given to c.C does not have the parameter's unit (F): it is in Ohm, not F"), "{all}");
    assert!(all.contains("[UNIT-MISMATCH] In 'Divider', the equation “v = r1.v + r2.p.i” does not balance its units: a sum mixes V and A."), "{all}");
    assert!(
        all.contains("[UNKNOWN-PORT] 'r2' (Electrical.Resistor) has no port 'q' (its ports: p, n)"),
        "{all}"
    );
    assert_eq!(errs.len(), 4, "{all}");
}
