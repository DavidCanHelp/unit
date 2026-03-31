// S-expression parser and Forth translator for unit's mesh wire format.
//
// Forth is the execution model. S-expressions are the wire format.
// A new nanobot implementation in any language can parse these messages
// without knowing Forth.

use crate::mesh::NodeId;

// ---------------------------------------------------------------------------
// S-expression type
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub enum Sexp {
    Atom(String),
    Number(i64),
    Str(String),
    List(Vec<Sexp>),
}

impl Sexp {
    pub fn as_atom(&self) -> Option<&str> {
        match self {
            Sexp::Atom(s) => Some(s),
            _ => None,
        }
    }

    pub fn as_number(&self) -> Option<i64> {
        match self {
            Sexp::Number(n) => Some(*n),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Sexp::Str(s) => Some(s),
            _ => None,
        }
    }

    pub fn as_list(&self) -> Option<&[Sexp]> {
        match self {
            Sexp::List(v) => Some(v),
            _ => None,
        }
    }

    /// Look up a keyword argument like :id, :fitness in a flat list.
    pub fn get_key(&self, key: &str) -> Option<&Sexp> {
        let items = self.as_list()?;
        for i in 0..items.len().saturating_sub(1) {
            if let Sexp::Atom(a) = &items[i] {
                if a == key {
                    return Some(&items[i + 1]);
                }
            }
        }
        None
    }
}

// ---------------------------------------------------------------------------
// Display (serialize to string)
// ---------------------------------------------------------------------------

impl core::fmt::Display for Sexp {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Sexp::Atom(s) => write!(f, "{}", s),
            Sexp::Number(n) => write!(f, "{}", n),
            Sexp::Str(s) => {
                write!(f, "\"")?;
                for c in s.chars() {
                    match c {
                        '"' => write!(f, "\\\"")?,
                        '\\' => write!(f, "\\\\")?,
                        '\n' => write!(f, "\\n")?,
                        _ => write!(f, "{}", c)?,
                    }
                }
                write!(f, "\"")
            }
            Sexp::List(items) => {
                write!(f, "(")?;
                for (i, item) in items.iter().enumerate() {
                    if i > 0 {
                        write!(f, " ")?;
                    }
                    write!(f, "{}", item)?;
                }
                write!(f, ")")
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Parser
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub struct ParseError(pub String);

impl core::fmt::Display for ParseError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "sexp parse error: {}", self.0)
    }
}

pub fn parse(input: &str) -> Result<Sexp, ParseError> {
    let mut pos = 0;
    skip_whitespace(input, &mut pos);
    if pos >= input.len() {
        return Err(ParseError("empty input".into()));
    }
    let result = parse_one(input, &mut pos)?;
    skip_whitespace(input, &mut pos);
    if pos < input.len() {
        return Err(ParseError(format!("trailing input at position {}", pos)));
    }
    Ok(result)
}

/// Parse a single S-expression from `input` starting at `pos`.
/// Advances `pos` past the parsed expression.
pub fn parse_at(input: &str, pos: &mut usize) -> Result<Sexp, ParseError> {
    skip_whitespace(input, pos);
    if *pos >= input.len() {
        return Err(ParseError("unexpected end of input".into()));
    }
    parse_one(input, pos)
}

fn parse_one(input: &str, pos: &mut usize) -> Result<Sexp, ParseError> {
    let bytes = input.as_bytes();
    match bytes[*pos] {
        b'(' => parse_list(input, pos),
        b'"' => parse_string(input, pos),
        _ => parse_atom_or_number(input, pos),
    }
}

fn parse_list(input: &str, pos: &mut usize) -> Result<Sexp, ParseError> {
    *pos += 1; // skip '('
    let mut items = Vec::new();
    loop {
        skip_whitespace(input, pos);
        if *pos >= input.len() {
            return Err(ParseError("unterminated list".into()));
        }
        if input.as_bytes()[*pos] == b')' {
            *pos += 1;
            return Ok(Sexp::List(items));
        }
        items.push(parse_one(input, pos)?);
    }
}

fn parse_string(input: &str, pos: &mut usize) -> Result<Sexp, ParseError> {
    *pos += 1; // skip opening '"'
    let bytes = input.as_bytes();
    let mut s = String::new();
    while *pos < bytes.len() {
        match bytes[*pos] {
            b'"' => {
                *pos += 1;
                return Ok(Sexp::Str(s));
            }
            b'\\' => {
                *pos += 1;
                if *pos >= bytes.len() {
                    return Err(ParseError("unterminated escape".into()));
                }
                match bytes[*pos] {
                    b'n' => s.push('\n'),
                    b'"' => s.push('"'),
                    b'\\' => s.push('\\'),
                    c => {
                        s.push('\\');
                        s.push(c as char);
                    }
                }
            }
            c => s.push(c as char),
        }
        *pos += 1;
    }
    Err(ParseError("unterminated string".into()))
}

fn parse_atom_or_number(input: &str, pos: &mut usize) -> Result<Sexp, ParseError> {
    let start = *pos;
    let bytes = input.as_bytes();
    while *pos < bytes.len() && !is_delimiter(bytes[*pos]) {
        *pos += 1;
    }
    if *pos == start {
        return Err(ParseError("expected atom".into()));
    }
    let token = &input[start..*pos];
    if let Ok(n) = token.parse::<i64>() {
        Ok(Sexp::Number(n))
    } else {
        Ok(Sexp::Atom(token.to_string()))
    }
}

fn is_delimiter(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\n' | b'\r' | b'(' | b')' | b'"')
}

fn skip_whitespace(input: &str, pos: &mut usize) {
    let bytes = input.as_bytes();
    while *pos < bytes.len() && matches!(bytes[*pos], b' ' | b'\t' | b'\n' | b'\r') {
        *pos += 1;
    }
}

// ---------------------------------------------------------------------------
// S-expression → Forth translator
// ---------------------------------------------------------------------------

/// Translate an S-expression into Forth source. Only handles executable
/// expressions (arithmetic, function calls). Declarative messages should be
/// handled by the mesh layer directly, not translated.
pub fn to_forth(sexp: &Sexp) -> String {
    match sexp {
        Sexp::Number(n) => format!("{}", n),
        Sexp::Atom(a) => a.clone(),
        Sexp::Str(s) => format!(".\" {}\"", s),
        Sexp::List(items) if items.is_empty() => String::new(),
        Sexp::List(items) => {
            // (op arg1 arg2 ...) → arg1 arg2 ... op
            let op = &items[0];
            let args: Vec<String> = items[1..].iter().map(|a| to_forth(a)).collect();
            let op_str = match op {
                Sexp::Atom(a) => translate_op(a),
                other => to_forth(other),
            };
            if args.is_empty() {
                op_str
            } else {
                format!("{} {}", args.join(" "), op_str)
            }
        }
    }
}

fn translate_op(op: &str) -> String {
    match op {
        "+" | "-" | "*" | "/" | "mod" => op.to_uppercase(),
        "=" | "<" | ">" => op.to_string(),
        "print" | "." => ".".to_string(),
        "cr" => "CR".to_string(),
        "dup" => "DUP".to_string(),
        "drop" => "DROP".to_string(),
        "swap" => "SWAP".to_string(),
        "over" => "OVER".to_string(),
        _ => op.to_uppercase(),
    }
}

// ---------------------------------------------------------------------------
// Mesh message constructors
// ---------------------------------------------------------------------------

pub fn msg_peer_hello(id: &NodeId, gen: i64, fitness: i64, peers: usize) -> Sexp {
    Sexp::List(vec![
        Sexp::Atom("peer-hello".into()),
        Sexp::Atom(":id".into()),
        Sexp::Str(crate::mesh::id_to_hex(id)),
        Sexp::Atom(":gen".into()),
        Sexp::Number(gen),
        Sexp::Atom(":fitness".into()),
        Sexp::Number(fitness),
        Sexp::Atom(":peers".into()),
        Sexp::Number(peers as i64),
    ])
}

pub fn msg_peer_status(id: &NodeId, peers: usize, fitness: i64, load: u32, capacity: u32) -> Sexp {
    Sexp::List(vec![
        Sexp::Atom("peer-status".into()),
        Sexp::Atom(":id".into()),
        Sexp::Str(crate::mesh::id_to_hex(id)),
        Sexp::Atom(":peers".into()),
        Sexp::Number(peers as i64),
        Sexp::Atom(":fitness".into()),
        Sexp::Number(fitness),
        Sexp::Atom(":load".into()),
        Sexp::Number(load as i64),
        Sexp::Atom(":capacity".into()),
        Sexp::Number(capacity as i64),
    ])
}

pub fn msg_goal(goal_id: u64, code: &str) -> Sexp {
    Sexp::List(vec![
        Sexp::Atom("goal".into()),
        Sexp::Atom(":id".into()),
        Sexp::Number(goal_id as i64),
        Sexp::Atom(":code".into()),
        Sexp::Str(code.into()),
    ])
}

pub fn msg_goal_result(goal_id: u64, unit_id: &NodeId, success: bool, output: &str) -> Sexp {
    Sexp::List(vec![
        Sexp::Atom("goal-result".into()),
        Sexp::Atom(":goal".into()),
        Sexp::Number(goal_id as i64),
        Sexp::Atom(":unit".into()),
        Sexp::Str(crate::mesh::id_to_hex(unit_id)),
        Sexp::Atom(":ok".into()),
        Sexp::Number(if success { 1 } else { 0 }),
        Sexp::Atom(":output".into()),
        Sexp::Str(output.into()),
    ])
}

pub fn msg_word_share(name: &str, source: &str, origin: &NodeId) -> Sexp {
    Sexp::List(vec![
        Sexp::Atom("word-share".into()),
        Sexp::Atom(":name".into()),
        Sexp::Str(name.into()),
        Sexp::Atom(":source".into()),
        Sexp::Str(source.into()),
        Sexp::Atom(":from".into()),
        Sexp::Str(crate::mesh::id_to_hex(origin)),
    ])
}

pub fn msg_event(event_type: &str, data: &str) -> Sexp {
    Sexp::List(vec![
        Sexp::Atom("event".into()),
        Sexp::Atom(":type".into()),
        Sexp::Str(event_type.into()),
        Sexp::Atom(":data".into()),
        Sexp::Str(data.into()),
    ])
}

pub fn msg_snapshot(id: &NodeId, fitness: i64, gen: u32) -> Sexp {
    Sexp::List(vec![
        Sexp::Atom("snapshot".into()),
        Sexp::Atom(":id".into()),
        Sexp::Str(crate::mesh::id_to_hex(id)),
        Sexp::Atom(":fitness".into()),
        Sexp::Number(fitness),
        Sexp::Atom(":gen".into()),
        Sexp::Number(gen as i64),
    ])
}

pub fn msg_resurrect(id: &NodeId, fitness: i64, gen: u32, saved_at: u64) -> Sexp {
    Sexp::List(vec![
        Sexp::Atom("resurrect".into()),
        Sexp::Atom(":id".into()),
        Sexp::Str(crate::mesh::id_to_hex(id)),
        Sexp::Atom(":fitness".into()),
        Sexp::Number(fitness),
        Sexp::Atom(":gen".into()),
        Sexp::Number(gen as i64),
        Sexp::Atom(":saved-at".into()),
        Sexp::Str(format!("{}", saved_at)),
    ])
}

/// Try to determine the message type from a parsed S-expression.
pub fn msg_type(sexp: &Sexp) -> Option<&str> {
    let items = sexp.as_list()?;
    items.first()?.as_atom()
}

// ---------------------------------------------------------------------------
// Parse mesh messages from S-expression strings
// ---------------------------------------------------------------------------

/// Try to parse a string as an S-expression mesh message.
/// Returns None if it's not a valid S-expression (could be raw Forth).
pub fn try_parse_mesh_msg(input: &str) -> Option<Sexp> {
    let trimmed = input.trim();
    if !trimmed.starts_with('(') {
        return None; // not an S-expression — probably raw Forth
    }
    parse(trimmed).ok()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_number() {
        assert_eq!(parse("42").unwrap(), Sexp::Number(42));
        assert_eq!(parse("-7").unwrap(), Sexp::Number(-7));
        assert_eq!(parse("0").unwrap(), Sexp::Number(0));
    }

    #[test]
    fn test_parse_atom() {
        assert_eq!(parse("hello").unwrap(), Sexp::Atom("hello".into()));
        assert_eq!(parse(":id").unwrap(), Sexp::Atom(":id".into()));
        assert_eq!(parse("+").unwrap(), Sexp::Atom("+".into()));
    }

    #[test]
    fn test_parse_string() {
        assert_eq!(parse("\"hello\"").unwrap(), Sexp::Str("hello".into()));
        assert_eq!(
            parse("\"he said \\\"hi\\\"\"").unwrap(),
            Sexp::Str("he said \"hi\"".into())
        );
        assert_eq!(parse("\"line\\n2\"").unwrap(), Sexp::Str("line\n2".into()));
    }

    #[test]
    fn test_parse_list() {
        assert_eq!(
            parse("(+ 2 3)").unwrap(),
            Sexp::List(vec![
                Sexp::Atom("+".into()),
                Sexp::Number(2),
                Sexp::Number(3),
            ])
        );
    }

    #[test]
    fn test_parse_nested() {
        assert_eq!(
            parse("(goal (* 6 7))").unwrap(),
            Sexp::List(vec![
                Sexp::Atom("goal".into()),
                Sexp::List(vec![
                    Sexp::Atom("*".into()),
                    Sexp::Number(6),
                    Sexp::Number(7),
                ]),
            ])
        );
    }

    #[test]
    fn test_parse_empty_list() {
        assert_eq!(parse("()").unwrap(), Sexp::List(vec![]));
    }

    #[test]
    fn test_parse_deeply_nested() {
        let s = parse("(a (b (c 1)))").unwrap();
        if let Sexp::List(outer) = &s {
            assert_eq!(outer.len(), 2);
            if let Sexp::List(inner) = &outer[1] {
                assert_eq!(inner.len(), 2);
            } else {
                panic!("expected nested list");
            }
        } else {
            panic!("expected list");
        }
    }

    #[test]
    fn test_parse_errors() {
        assert!(parse("").is_err());
        assert!(parse("(").is_err());
        assert!(parse("(a b").is_err());
        assert!(parse("\"unterminated").is_err());
        assert!(parse("a b").is_err()); // trailing input
    }

    #[test]
    fn test_roundtrip() {
        let cases = vec![
            "(+ 2 3)",
            "(peer-hello :id \"abc\" :gen 0 :fitness 10 :peers 3)",
            "(goal :id 42 :code \"6 7 *\")",
            "()",
            "42",
        ];
        for input in cases {
            let parsed = parse(input).unwrap();
            let output = parsed.to_string();
            let reparsed = parse(&output).unwrap();
            assert_eq!(parsed, reparsed, "roundtrip failed for: {}", input);
        }
    }

    #[test]
    fn test_to_forth_arithmetic() {
        let sexp = parse("(+ 2 3)").unwrap();
        assert_eq!(to_forth(&sexp), "2 3 +");
    }

    #[test]
    fn test_to_forth_nested() {
        let sexp = parse("(* (+ 1 2) 4)").unwrap();
        assert_eq!(to_forth(&sexp), "1 2 + 4 *");
    }

    #[test]
    fn test_to_forth_single_op() {
        let sexp = parse("(cr)").unwrap();
        assert_eq!(to_forth(&sexp), "CR");
    }

    #[test]
    fn test_to_forth_number() {
        assert_eq!(to_forth(&Sexp::Number(42)), "42");
    }

    #[test]
    fn test_msg_constructors() {
        let id = [0u8; 8];
        let hello = msg_peer_hello(&id, 0, 10, 3);
        assert_eq!(msg_type(&hello), Some("peer-hello"));
        let s = hello.to_string();
        assert!(s.starts_with("(peer-hello"));

        let goal = msg_goal(42, "6 7 *");
        assert_eq!(msg_type(&goal), Some("goal"));
        assert_eq!(
            goal.get_key(":code").and_then(|s| s.as_str()),
            Some("6 7 *")
        );
    }

    #[test]
    fn test_get_key() {
        let msg = parse("(peer-hello :id \"abc\" :gen 0 :fitness 10)").unwrap();
        assert_eq!(msg.get_key(":id").unwrap().as_str(), Some("abc"));
        assert_eq!(msg.get_key(":gen").unwrap().as_number(), Some(0));
        assert_eq!(msg.get_key(":fitness").unwrap().as_number(), Some(10));
        assert!(msg.get_key(":missing").is_none());
    }

    #[test]
    fn test_try_parse_mesh_msg() {
        assert!(try_parse_mesh_msg("(peer-hello :id \"abc\")").is_some());
        assert!(try_parse_mesh_msg("2 3 + .").is_none()); // raw Forth
        assert!(try_parse_mesh_msg("").is_none());
    }

    #[test]
    fn test_to_forth_goal() {
        // Extract code from a goal message and translate
        let msg = parse("(goal (* 42 42))").unwrap();
        if let Sexp::List(items) = &msg {
            let code_sexp = &items[1];
            assert_eq!(to_forth(code_sexp), "42 42 *");
        }
    }
}
