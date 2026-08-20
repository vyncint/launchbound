//! A deliberately small constraint language over integer dimensions.
//!
//! Grammar (no parentheses, two precedence levels):
//!   constraint := arith CMP arith
//!   arith      := term (('+' | '-') term)*
//!   term       := atom (('*' | '/' | '%') atom)*
//!   atom       := integer | dimension-name
//!   CMP        := '==' | '!=' | '<=' | '>=' | '<' | '>'
//!
//! Evaluation is checked u64 arithmetic (division truncates); overflow or
//! division/modulo by zero makes the constraint an error, never silently
//! true or false.

use crate::spec::Value;
use crate::{Config, SpaceError};

#[derive(Debug, Clone, PartialEq, Eq)]
enum Token {
    Num(u64),
    Ident(String),
    Op(char),
    Cmp(&'static str),
}

#[derive(Debug, Clone)]
pub struct Constraint {
    text: String,
    lhs: Vec<Token>,
    cmp: &'static str,
    rhs: Vec<Token>,
}

impl Constraint {
    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn parse(expr: &str, dims: &[&str]) -> Result<Self, SpaceError> {
        let err = |reason: &str| SpaceError::Constraint {
            expr: expr.to_string(),
            reason: reason.to_string(),
        };
        let tokens = tokenize(expr).map_err(|r| err(&r))?;
        let cmp_pos = tokens
            .iter()
            .position(|t| matches!(t, Token::Cmp(_)))
            .ok_or_else(|| err("no comparison operator"))?;
        let Token::Cmp(cmp) = tokens[cmp_pos] else {
            unreachable!()
        };
        if tokens.iter().filter(|t| matches!(t, Token::Cmp(_))).count() != 1 {
            return Err(err("exactly one comparison operator required"));
        }
        let lhs = tokens[..cmp_pos].to_vec();
        let rhs = tokens[cmp_pos + 1..].to_vec();
        for side in [&lhs, &rhs] {
            if side.is_empty() {
                return Err(err("empty side of comparison"));
            }
            for t in side {
                if let Token::Ident(name) = t
                    && !dims.contains(&name.as_str())
                {
                    return Err(err(&format!("unknown dimension `{name}`")));
                }
            }
        }
        Ok(Constraint {
            text: expr.to_string(),
            lhs,
            cmp,
            rhs,
        })
    }

    pub fn eval(&self, config: &Config) -> Result<bool, SpaceError> {
        let resolve = config_resolver(config);
        let l = eval_arith(&self.lhs, &resolve, &self.text)?;
        let r = eval_arith(&self.rhs, &resolve, &self.text)?;
        Ok(match self.cmp {
            "==" => l == r,
            "!=" => l != r,
            "<=" => l <= r,
            ">=" => l >= r,
            "<" => l < r,
            ">" => l > r,
            _ => unreachable!(),
        })
    }
}

fn tokenize(expr: &str) -> Result<Vec<Token>, String> {
    let mut tokens = Vec::new();
    let bytes = expr.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i] as char;
        match c {
            ' ' | '\t' => i += 1,
            '0'..='9' => {
                let start = i;
                while i < bytes.len() && bytes[i].is_ascii_digit() {
                    i += 1;
                }
                let n: u64 = expr[start..i]
                    .parse()
                    .map_err(|_| "integer literal too large".to_string())?;
                tokens.push(Token::Num(n));
            }
            'a'..='z' | '_' => {
                let start = i;
                while i < bytes.len()
                    && (bytes[i].is_ascii_lowercase()
                        || bytes[i].is_ascii_digit()
                        || bytes[i] == b'_')
                {
                    i += 1;
                }
                tokens.push(Token::Ident(expr[start..i].to_string()));
            }
            '*' | '/' | '%' | '+' | '-' => {
                tokens.push(Token::Op(c));
                i += 1;
            }
            '=' | '!' | '<' | '>' => {
                let two = &expr[i..(i + 2).min(expr.len())];
                let cmp = match two {
                    "==" => Some("=="),
                    "!=" => Some("!="),
                    "<=" => Some("<="),
                    ">=" => Some(">="),
                    _ => None,
                };
                if let Some(cmp) = cmp {
                    tokens.push(Token::Cmp(cmp));
                    i += 2;
                } else if c == '<' {
                    tokens.push(Token::Cmp("<"));
                    i += 1;
                } else if c == '>' {
                    tokens.push(Token::Cmp(">"));
                    i += 1;
                } else {
                    return Err(format!("unexpected character `{c}`"));
                }
            }
            other => return Err(format!("unexpected character `{other}`")),
        }
    }
    Ok(tokens)
}

fn eval_arith(
    tokens: &[Token],
    resolve: &dyn Fn(&str) -> Result<u64, String>,
    text: &str,
) -> Result<u64, SpaceError> {
    let err = |reason: String| SpaceError::Constraint {
        expr: text.to_string(),
        reason,
    };
    let atom = |t: &Token| -> Result<u64, SpaceError> {
        match t {
            Token::Num(n) => Ok(*n),
            Token::Ident(name) => resolve(name).map_err(err),
            Token::Op(_) | Token::Cmp(_) => Err(err("misplaced operator".into())),
        }
    };

    // First pass: fold * and % into a term list separated by +/-.
    let mut terms: Vec<(char, u64)> = Vec::new(); // (sign-op, value)
    let mut pending_op: Option<char> = None; // within-term * or %
    let mut sign: char = '+';
    let mut current: Option<u64> = None;
    for t in tokens {
        match t {
            Token::Op(op @ ('*' | '/' | '%')) => {
                if current.is_none() {
                    return Err(err(format!("`{op}` with no left operand")));
                }
                pending_op = Some(*op);
            }
            Token::Op(op @ ('+' | '-')) => {
                let value = current
                    .take()
                    .ok_or_else(|| err(format!("`{op}` with no left operand")))?;
                terms.push((sign, value));
                sign = *op;
                pending_op = None;
            }
            atom_token => {
                let v = atom(atom_token)?;
                current = Some(match (current, pending_op.take()) {
                    (None, None) => v,
                    (Some(acc), Some('*')) => acc
                        .checked_mul(v)
                        .ok_or_else(|| err("multiplication overflow".into()))?,
                    (Some(acc), Some('%')) => {
                        if v == 0 {
                            return Err(err("modulo by zero".into()));
                        }
                        acc % v
                    }
                    (Some(acc), Some('/')) => {
                        if v == 0 {
                            return Err(err("division by zero".into()));
                        }
                        acc / v
                    }
                    (Some(_), None) => {
                        return Err(err("two operands with no operator".into()));
                    }
                    (None, Some(_)) => unreachable!(),
                    (Some(_), Some(_)) => unreachable!(),
                });
            }
        }
    }
    let value = current.ok_or_else(|| err("trailing operator".into()))?;
    terms.push((sign, value));

    let mut acc: u64 = 0;
    for (op, v) in terms {
        acc = match op {
            '+' => acc
                .checked_add(v)
                .ok_or_else(|| err("addition overflow".into()))?,
            '-' => acc
                .checked_sub(v)
                .ok_or_else(|| err("subtraction underflow".into()))?,
            _ => unreachable!(),
        };
    }
    Ok(acc)
}

fn config_resolver(config: &Config) -> impl Fn(&str) -> Result<u64, String> + '_ {
    move |name: &str| match config.get(name) {
        Some(Value::Int(n)) => Ok(*n),
        Some(Value::Str(_)) => Err(format!(
            "dimension `{name}` is a string and cannot be used in arithmetic"
        )),
        None => Err(format!("dimension `{name}` missing from config")),
    }
}

/// Evaluate a comparison-free arithmetic expression against a candidate's
/// dimensions plus extra named variables (bench plans use this for grid
/// shapes and buffer sizes, e.g. `elements / block_x`).
pub fn eval_arith_expr(
    expr: &str,
    config: &Config,
    extra: &std::collections::BTreeMap<String, u64>,
) -> Result<u64, SpaceError> {
    let err = |reason: &str| SpaceError::Constraint {
        expr: expr.to_string(),
        reason: reason.to_string(),
    };
    let tokens = tokenize(expr).map_err(|r| err(&r))?;
    if tokens.iter().any(|t| matches!(t, Token::Cmp(_))) {
        return Err(err("comparison operators are not allowed here"));
    }
    let base = config_resolver(config);
    let resolve = move |name: &str| -> Result<u64, String> {
        if let Some(v) = extra.get(name) {
            return Ok(*v);
        }
        base(name)
    };
    eval_arith(&tokens, &resolve, expr)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::KernelSpec;

    fn config_with(block: u64, tile: u64) -> Config {
        let spec = KernelSpec::from_toml_str(
            "t",
            &format!(
                r#"
                [kernel]
                name = "t"
                entry = "t"
                domain = 1
                [dims.block_x]
                values = [{block}]
                [dims.tile]
                values = [{tile}]
                "#
            ),
        )
        .unwrap();
        crate::enumerate(&spec).unwrap().into_iter().next().unwrap()
    }

    #[test]
    fn arithmetic_and_comparisons() {
        let dims = ["block_x", "tile"];
        let c = config_with(64, 256);
        for (expr, expected) in [
            ("tile % block_x == 0", true),
            ("tile % block_x != 0", false),
            ("block_x * tile <= 16384", true),
            ("block_x * tile < 16384", false),
            ("tile - block_x == 192", true),
            ("tile + block_x >= 320", true),
            ("block_x > 32", true),
        ] {
            let parsed = Constraint::parse(expr, &dims).unwrap();
            assert_eq!(parsed.eval(&c).unwrap(), expected, "{expr}");
        }
    }

    #[test]
    fn rejects_unknown_dimension_and_junk() {
        let dims = ["block_x"];
        assert!(Constraint::parse("bogus == 1", &dims).is_err());
        assert!(Constraint::parse("block_x == ", &dims).is_err());
        assert!(Constraint::parse("block_x", &dims).is_err());
        assert!(Constraint::parse("block_x == 1 == 2", &dims).is_err());
        assert!(Constraint::parse("block_x @ 2", &dims).is_err());
    }

    #[test]
    fn division_by_zero_is_an_error_not_a_verdict() {
        let dims = ["block_x", "tile"];
        let c = config_with(64, 0);
        let parsed = Constraint::parse("block_x % tile == 0", &dims).unwrap();
        assert!(parsed.eval(&c).is_err());
    }
}
