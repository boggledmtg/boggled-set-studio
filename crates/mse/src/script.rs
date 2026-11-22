//! A subset of Magic Set Editor's expression language.
//!
//! Enough to lay out the M15 creature frame: `if/then/else`, `and`/`or`/`not`,
//! comparisons, arithmetic, string interpolation, calls, field access, and
//! `{ ... }` blocks. `for` is rejected with the function-style error the
//! renderer surfaces when a stylesheet needs something this build does not do.

use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Nil,
    Bool(bool),
    Num(f64),
    Str(String),
    List(Vec<Value>),
    Map(BTreeMap<String, Value>),
    Func(Box<Expr>),
}

impl Value {
    pub fn as_num(&self) -> Option<f64> {
        match self {
            Value::Num(n) => Some(*n),
            Value::Bool(true) => Some(1.0),
            Value::Bool(false) | Value::Nil => Some(0.0),
            _ => None,
        }
    }

    pub fn truthy(&self) -> bool {
        !matches!(self, Value::Nil | Value::Bool(false))
    }

    pub fn missing(&self) -> bool {
        match self {
            Value::Nil => true,
            Value::Str(text) => text.is_empty(),
            _ => false,
        }
    }

    fn display(&self) -> String {
        match self {
            Value::Nil => String::new(),
            Value::Bool(v) => v.to_string(),
            Value::Num(n) => {
                if n.fract() == 0.0 && n.abs() < 1e15 {
                    format!("{}", *n as i64)
                } else {
                    format!("{n}")
                }
            }
            Value::Str(text) => text.clone(),
            Value::List(items) => items
                .iter()
                .map(Value::display)
                .collect::<Vec<_>>()
                .join(","),
            Value::Map(_) => String::new(),
            Value::Func(_) => String::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    Lit(Value),
    Var(String),
    List(Vec<Expr>),
    Block(Vec<Stmt>),
    If {
        cond: Box<Expr>,
        then_branch: Box<Expr>,
        else_branch: Option<Box<Expr>>,
    },
    Unary(UnaryOp, Box<Expr>),
    Binary(BinOp, Box<Expr>, Box<Expr>),
    Call {
        callee: Box<Expr>,
        args: Vec<CallArg>,
    },
    Field {
        base: Box<Expr>,
        name: String,
    },
    Str(Vec<StrPart>),
}

#[derive(Debug, Clone, PartialEq)]
pub struct CallArg {
    pub name: Option<String>,
    pub value: Expr,
}

#[derive(Debug, Clone, PartialEq)]
pub enum StrPart {
    Lit(String),
    Expr(Expr),
}

#[derive(Debug, Clone, PartialEq)]
pub enum Stmt {
    Let(String, Expr),
    Expr(Expr),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnaryOp {
    Not,
    Neg,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinOp {
    Or,
    OrElse,
    And,
    Eq,
    Ne,
    Lt,
    Gt,
    Le,
    Ge,
    Add,
    Sub,
    Mul,
    Div,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScriptError(pub String);

impl std::fmt::Display for ScriptError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for ScriptError {}

pub struct Arg {
    pub name: Option<String>,
    pub value: Value,
}

pub fn eval_script(
    text: &str,
    vars: &BTreeMap<String, Value>,
    host: &mut dyn FnMut(&str, &[Arg]) -> Result<Value, String>,
) -> Result<Value, ScriptError> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Ok(Value::Nil);
    }
    let wrapped = if trimmed.starts_with('{') {
        trimmed.to_string()
    } else {
        format!("{{ {trimmed} }}")
    };
    let expr = parse(&wrapped)?;
    let mut env = vars.clone();
    eval(&expr, &mut env, host)
}

pub fn parse(text: &str) -> Result<Expr, ScriptError> {
    let mut parser = Parser { s: text, i: 0 };
    let expr = parser.parse_expr()?;
    parser.skip();
    if parser.i < parser.s.len() {
        return Err(ScriptError(format!(
            "unexpected script text near '{}'",
            parser.s[parser.i..].chars().take(24).collect::<String>()
        )));
    }
    Ok(expr)
}

struct Parser<'a> {
    s: &'a str,
    i: usize,
}

impl<'a> Parser<'a> {
    fn parse_expr(&mut self) -> Result<Expr, ScriptError> {
        self.skip();
        if self.eat_word("for") {
            return Err(ScriptError("not implemented: for".into()));
        }
        if self.eat_word("if") {
            let cond = self.parse_expr()?;
            self.expect_word("then")?;
            let then_branch = self.parse_expr()?;
            let else_branch = if self.eat_word("else") {
                Some(Box::new(self.parse_expr()?))
            } else {
                None
            };
            return Ok(Expr::If {
                cond: Box::new(cond),
                then_branch: Box::new(then_branch),
                else_branch,
            });
        }
        self.parse_or()
    }

    fn parse_or(&mut self) -> Result<Expr, ScriptError> {
        let mut left = self.parse_and()?;
        loop {
            self.skip();
            if self.eat_word("or") {
                if self.eat_word("else") {
                    let right = self.parse_and()?;
                    left = Expr::Binary(BinOp::OrElse, Box::new(left), Box::new(right));
                } else {
                    let right = self.parse_and()?;
                    left = Expr::Binary(BinOp::Or, Box::new(left), Box::new(right));
                }
            } else {
                break;
            }
        }
        Ok(left)
    }

    fn parse_and(&mut self) -> Result<Expr, ScriptError> {
        let mut left = self.parse_not()?;
        while self.eat_word("and") {
            let right = self.parse_not()?;
            left = Expr::Binary(BinOp::And, Box::new(left), Box::new(right));
        }
        Ok(left)
    }

    fn parse_not(&mut self) -> Result<Expr, ScriptError> {
        self.skip();
        if self.eat_word("not") {
            return Ok(Expr::Unary(UnaryOp::Not, Box::new(self.parse_not()?)));
        }
        self.parse_cmp()
    }

    fn parse_cmp(&mut self) -> Result<Expr, ScriptError> {
        let mut left = self.parse_add()?;
        loop {
            self.skip();
            let op = if self.eat("==") {
                BinOp::Eq
            } else if self.eat("!=") {
                BinOp::Ne
            } else if self.eat("<=") {
                BinOp::Le
            } else if self.eat(">=") {
                BinOp::Ge
            } else if self.eat("<") {
                BinOp::Lt
            } else if self.eat(">") {
                BinOp::Gt
            } else if self.eat("=") {
                BinOp::Eq
            } else {
                break;
            };
            let right = self.parse_add()?;
            left = Expr::Binary(op, Box::new(left), Box::new(right));
        }
        Ok(left)
    }

    fn parse_add(&mut self) -> Result<Expr, ScriptError> {
        let mut left = self.parse_mul()?;
        loop {
            self.skip();
            let op = if self.eat("+") {
                BinOp::Add
            } else if self.eat("-") {
                BinOp::Sub
            } else {
                break;
            };
            let right = self.parse_mul()?;
            left = Expr::Binary(op, Box::new(left), Box::new(right));
        }
        Ok(left)
    }

    fn parse_mul(&mut self) -> Result<Expr, ScriptError> {
        let mut left = self.parse_unary()?;
        loop {
            self.skip();
            let op = if self.eat("*") {
                BinOp::Mul
            } else if self.eat("/") {
                BinOp::Div
            } else {
                break;
            };
            let right = self.parse_unary()?;
            left = Expr::Binary(op, Box::new(left), Box::new(right));
        }
        Ok(left)
    }

    fn parse_unary(&mut self) -> Result<Expr, ScriptError> {
        self.skip();
        if self.eat("-") {
            return Ok(Expr::Unary(UnaryOp::Neg, Box::new(self.parse_unary()?)));
        }
        self.parse_postfix()
    }

    fn parse_postfix(&mut self) -> Result<Expr, ScriptError> {
        let mut expr = self.parse_primary()?;
        loop {
            self.skip();
            if self.eat("(") {
                let args = self.parse_args()?;
                expr = Expr::Call {
                    callee: Box::new(expr),
                    args,
                };
            } else if self.eat(".") {
                let name = self.ident()?;
                expr = Expr::Field {
                    base: Box::new(expr),
                    name,
                };
            } else {
                break;
            }
        }
        Ok(expr)
    }

    fn parse_args(&mut self) -> Result<Vec<CallArg>, ScriptError> {
        let mut args = Vec::new();
        loop {
            self.skip();
            if self.eat(")") {
                break;
            }
            if !args.is_empty() {
                self.expect(",")?;
                self.skip();
                if self.eat(")") {
                    break;
                }
            }
            args.push(self.parse_arg()?);
        }
        Ok(args)
    }

    fn parse_arg(&mut self) -> Result<CallArg, ScriptError> {
        self.skip();
        let save = self.i;
        if let Ok(name) = self.ident() {
            self.skip();
            if self.starts_with(":=") {
                self.i = save;
            } else if self.eat(":") {
                return Ok(CallArg {
                    name: Some(name),
                    value: self.parse_expr()?,
                });
            } else {
                self.i = save;
            }
        }
        Ok(CallArg {
            name: None,
            value: self.parse_expr()?,
        })
    }

    fn parse_primary(&mut self) -> Result<Expr, ScriptError> {
        self.skip();
        if self.eat_word("for") {
            return Err(ScriptError("not implemented: for".into()));
        }
        if self.eat_word("true") {
            return Ok(Expr::Lit(Value::Bool(true)));
        }
        if self.eat_word("false") {
            return Ok(Expr::Lit(Value::Bool(false)));
        }
        if self.eat_word("nil") {
            return Ok(Expr::Lit(Value::Nil));
        }
        if self.peek() == Some('"') {
            return Ok(Expr::Str(self.parse_string()?));
        }
        if self.eat("(") {
            let expr = self.parse_expr()?;
            self.expect(")")?;
            return Ok(expr);
        }
        if self.eat("{") {
            return self.parse_block();
        }
        if self.eat("[") {
            let mut items = Vec::new();
            loop {
                self.skip();
                if self.eat("]") {
                    break;
                }
                if !items.is_empty() {
                    self.expect(",")?;
                    self.skip();
                    if self.eat("]") {
                        break;
                    }
                }
                items.push(self.parse_expr()?);
            }
            return Ok(Expr::List(items));
        }
        if self.peek().is_some_and(|c| c.is_ascii_digit()) {
            return Ok(Expr::Lit(Value::Num(self.number()?)));
        }
        if self.peek().is_some_and(|c| c.is_ascii_alphabetic() || c == '_') {
            return Ok(Expr::Var(self.ident()?));
        }
        Err(ScriptError(format!(
            "unexpected script text near '{}'",
            self.s[self.i..].chars().take(24).collect::<String>()
        )))
    }

    fn parse_block(&mut self) -> Result<Expr, ScriptError> {
        let mut stmts = Vec::new();
        loop {
            self.skip();
            if self.eat("}") {
                break;
            }
            if self.i >= self.s.len() {
                return Err(ScriptError("unclosed script block".into()));
            }
            if self.eat_word("for") {
                return Err(ScriptError("not implemented: for".into()));
            }
            if self.starts_assign() {
                let name = self.ident()?;
                self.expect(":=")?;
                let value = self.parse_expr()?;
                stmts.push(Stmt::Let(name, value));
            } else {
                stmts.push(Stmt::Expr(self.parse_expr()?));
            }
            self.skip();
            let _ = self.eat(";");
        }
        Ok(Expr::Block(stmts))
    }

    fn parse_string(&mut self) -> Result<Vec<StrPart>, ScriptError> {
        self.expect("\"")?;
        let mut parts = Vec::new();
        let mut buf = String::new();
        while let Some(c) = self.bump() {
            match c {
                '"' => {
                    if !buf.is_empty() {
                        parts.push(StrPart::Lit(buf));
                    }
                    return Ok(parts);
                }
                '\\' => {
                    let escaped = self.bump().ok_or_else(|| ScriptError("bad string".into()))?;
                    buf.push(match escaped {
                        'n' => '\n',
                        't' => '\t',
                        other => other,
                    });
                }
                '{' => {
                    if !buf.is_empty() {
                        parts.push(StrPart::Lit(std::mem::take(&mut buf)));
                    }
                    let expr = self.parse_expr()?;
                    self.expect("}")?;
                    parts.push(StrPart::Expr(expr));
                }
                other => buf.push(other),
            }
        }
        Err(ScriptError("unterminated string".into()))
    }

    fn number(&mut self) -> Result<f64, ScriptError> {
        let start = self.i;
        while self.peek().is_some_and(|c| c.is_ascii_digit()) {
            self.bump();
        }
        if self.peek() == Some('.') {
            self.bump();
            while self.peek().is_some_and(|c| c.is_ascii_digit()) {
                self.bump();
            }
        }
        self.s[start..self.i]
            .parse()
            .map_err(|_| ScriptError("bad number".into()))
    }

    fn ident(&mut self) -> Result<String, ScriptError> {
        self.skip();
        let start = self.i;
        let Some(first) = self.peek() else {
            return Err(ScriptError("expected a name".into()));
        };
        if !(first.is_ascii_alphabetic() || first == '_') {
            return Err(ScriptError("expected a name".into()));
        }
        self.bump();
        while self.peek().is_some_and(|c| c.is_ascii_alphanumeric() || c == '_') {
            self.bump();
        }
        Ok(self.s[start..self.i].to_string())
    }

    fn starts_assign(&mut self) -> bool {
        let save = self.i;
        if self.ident().is_err() {
            self.i = save;
            return false;
        }
        self.skip();
        let yes = self.starts_with(":=");
        self.i = save;
        yes
    }

    fn starts_with(&self, token: &str) -> bool {
        self.s[self.i..].starts_with(token)
    }

    fn expect(&mut self, token: &str) -> Result<(), ScriptError> {
        if self.eat(token) {
            Ok(())
        } else {
            Err(ScriptError(format!("expected {token}")))
        }
    }

    fn expect_word(&mut self, word: &str) -> Result<(), ScriptError> {
        if self.eat_word(word) {
            Ok(())
        } else {
            Err(ScriptError(format!("expected {word}")))
        }
    }

    fn eat(&mut self, token: &str) -> bool {
        self.skip();
        if self.starts_with(token) {
            self.i += token.len();
            true
        } else {
            false
        }
    }

    fn eat_word(&mut self, word: &str) -> bool {
        self.skip();
        if !self.starts_with(word) {
            return false;
        }
        let after = self.s[self.i + word.len()..].chars().next();
        if after.is_some_and(|c| c.is_ascii_alphanumeric() || c == '_') {
            return false;
        }
        self.i += word.len();
        true
    }

    fn skip(&mut self) {
        loop {
            let Some(c) = self.peek() else {
                return;
            };
            if c.is_whitespace() {
                self.bump();
                continue;
            }
            if c == '#' {
                while self.peek().is_some_and(|ch| ch != '\n') {
                    self.bump();
                }
                continue;
            }
            return;
        }
    }

    fn peek(&self) -> Option<char> {
        self.s[self.i..].chars().next()
    }

    fn bump(&mut self) -> Option<char> {
        let c = self.peek()?;
        self.i += c.len_utf8();
        Some(c)
    }
}

fn eval(
    expr: &Expr,
    env: &mut BTreeMap<String, Value>,
    host: &mut dyn FnMut(&str, &[Arg]) -> Result<Value, String>,
) -> Result<Value, ScriptError> {
    match expr {
        Expr::Lit(value) => Ok(value.clone()),
        Expr::Var(name) => Ok(env.get(name).cloned().unwrap_or(Value::Nil)),
        Expr::List(items) => {
            let mut out = Vec::new();
            for item in items {
                out.push(eval(item, env, host)?);
            }
            Ok(Value::List(out))
        }
        Expr::Block(stmts) => {
            let mut last = Value::Nil;
            for stmt in stmts {
                match stmt {
                    Stmt::Let(name, value) => {
                        // `{ ... }` on the right of `:=` is a function, matching MSE init scripts.
                        // A brace block used as a field value is evaluated by the caller instead.
                        let stored = if let Expr::Block(_) = value {
                            Value::Func(Box::new(value.clone()))
                        } else {
                            eval(value, env, host)?
                        };
                        last = stored.clone();
                        env.insert(name.clone(), stored);
                    }
                    Stmt::Expr(expr) => last = eval(expr, env, host)?,
                }
            }
            Ok(last)
        }
        Expr::If {
            cond,
            then_branch,
            else_branch,
        } => {
            if eval(cond, env, host)?.truthy() {
                eval(then_branch, env, host)
            } else if let Some(else_branch) = else_branch {
                eval(else_branch, env, host)
            } else {
                Ok(Value::Nil)
            }
        }
        Expr::Unary(UnaryOp::Not, inner) => Ok(Value::Bool(!eval(inner, env, host)?.truthy())),
        Expr::Unary(UnaryOp::Neg, inner) => {
            let n = eval(inner, env, host)?
                .as_num()
                .ok_or_else(|| ScriptError("expected a number".into()))?;
            Ok(Value::Num(-n))
        }
        Expr::Binary(op, left, right) => eval_binary(*op, left, right, env, host),
        Expr::Call { callee, args } => {
            let name = match callee.as_ref() {
                Expr::Var(name) => name.clone(),
                other => eval(other, env, host)?.display(),
            };
            if let Some(Value::Func(body)) = env.get(&name).cloned() {
                let mut local = env.clone();
                for arg in args {
                    let value = eval(&arg.value, env, host)?;
                    if let Some(param) = &arg.name {
                        local.insert(param.clone(), value);
                    }
                }
                return eval(body.as_ref(), &mut local, host);
            }
            let mut evaluated = Vec::new();
            for arg in args {
                evaluated.push(Arg {
                    name: arg.name.clone(),
                    value: eval(&arg.value, env, host)?,
                });
            }
            host(&name, &evaluated).map_err(ScriptError)
        }
        Expr::Field { base, name } => {
            let value = eval(base, env, host)?;
            Ok(field(&value, name))
        }
        Expr::Str(parts) => {
            let mut out = String::new();
            for part in parts {
                match part {
                    StrPart::Lit(text) => out.push_str(text),
                    StrPart::Expr(expr) => out.push_str(&eval(expr, env, host)?.display()),
                }
            }
            Ok(Value::Str(out))
        }
    }
}

fn field(value: &Value, name: &str) -> Value {
    match value {
        Value::Map(map) => map.get(name).cloned().unwrap_or(Value::Nil),
        Value::List(items) => name
            .parse::<usize>()
            .ok()
            .and_then(|index| items.get(index).cloned())
            .unwrap_or(Value::Nil),
        _ => Value::Nil,
    }
}

fn eval_binary(
    op: BinOp,
    left: &Expr,
    right: &Expr,
    env: &mut BTreeMap<String, Value>,
    host: &mut dyn FnMut(&str, &[Arg]) -> Result<Value, String>,
) -> Result<Value, ScriptError> {
    if op == BinOp::OrElse {
        let left_value = eval(left, env, host)?;
        if left_value.missing() {
            return eval(right, env, host);
        }
        return Ok(left_value);
    }
    if op == BinOp::Or {
        let left_value = eval(left, env, host)?;
        if left_value.truthy() {
            return Ok(left_value);
        }
        return eval(right, env, host);
    }
    if op == BinOp::And {
        let left_value = eval(left, env, host)?;
        if !left_value.truthy() {
            return Ok(left_value);
        }
        return eval(right, env, host);
    }
    let left_value = eval(left, env, host)?;
    let right_value = eval(right, env, host)?;
    match op {
        BinOp::Eq => Ok(Value::Bool(equals(&left_value, &right_value))),
        BinOp::Ne => Ok(Value::Bool(!equals(&left_value, &right_value))),
        BinOp::Lt | BinOp::Gt | BinOp::Le | BinOp::Ge => {
            let l = left_value
                .as_num()
                .ok_or_else(|| ScriptError("expected a number".into()))?;
            let r = right_value
                .as_num()
                .ok_or_else(|| ScriptError("expected a number".into()))?;
            let result = match op {
                BinOp::Lt => l < r,
                BinOp::Gt => l > r,
                BinOp::Le => l <= r,
                BinOp::Ge => l >= r,
                _ => false,
            };
            Ok(Value::Bool(result))
        }
        BinOp::Add => {
            if matches!(left_value, Value::Str(_) | Value::Nil)
                || matches!(right_value, Value::Str(_))
                    && !matches!(left_value, Value::Num(_))
            {
                return Ok(Value::Str(format!(
                    "{}{}",
                    left_value.display(),
                    right_value.display()
                )));
            }
            if matches!(left_value, Value::Str(_)) || matches!(right_value, Value::Str(_)) {
                return Ok(Value::Str(format!(
                    "{}{}",
                    left_value.display(),
                    right_value.display()
                )));
            }
            Ok(Value::Num(num(&left_value)? + num(&right_value)?))
        }
        BinOp::Sub => Ok(Value::Num(num(&left_value)? - num(&right_value)?)),
        BinOp::Mul => Ok(Value::Num(num(&left_value)? * num(&right_value)?)),
        BinOp::Div => {
            let divisor = num(&right_value)?;
            if divisor == 0.0 {
                return Err(ScriptError("division by zero".into()));
            }
            Ok(Value::Num(num(&left_value)? / divisor))
        }
        BinOp::Or | BinOp::OrElse | BinOp::And => unreachable!(),
    }
}

fn equals(left: &Value, right: &Value) -> bool {
    match (left, right) {
        (Value::Num(a), Value::Num(b)) => a == b,
        (Value::Bool(a), Value::Bool(b)) => a == b,
        (Value::Nil, Value::Nil) => true,
        (Value::Nil, _) | (_, Value::Nil) => false,
        _ => left.display() == right.display(),
    }
}

fn num(value: &Value) -> Result<f64, ScriptError> {
    value
        .as_num()
        .ok_or_else(|| ScriptError("expected a number".into()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(text: &str) -> Result<Value, ScriptError> {
        eval_script(text, &BTreeMap::new(), &mut |name, args| match name {
            "max" => {
                let n = args.iter().filter_map(|arg| arg.value.as_num()).fold(0.0, f64::max);
                Ok(Value::Num(n))
            }
            "is_leveler" => Ok(Value::Bool(false)),
            other => Err(format!("missing function: {other}")),
        })
    }

    #[test]
    fn if_else_and_arithmetic_choose_the_else_branch() {
        let value = run("if is_leveler() then 75 else 29 + 3").unwrap();
        assert_eq!(value, Value::Num(32.0));
    }

    #[test]
    fn strings_interpolate_and_or_else_fills_a_missing_value() {
        let mut vars = BTreeMap::new();
        vars.insert(
            "card".into(),
            Value::Map(BTreeMap::from([("name".into(), Value::Str("Owl".into()))])),
        );
        let value = eval_script(
            "\"n={card.name}\" + (missing or else \"!\")",
            &vars,
            &mut |name, _| Err(format!("missing function: {name}")),
        )
        .unwrap();
        assert_eq!(value, Value::Str("n=Owl!".into()));
    }

    #[test]
    fn assignment_defines_a_function() {
        let value = run("width := { if false then 1 else 10 }\nwidth()").unwrap();
        assert_eq!(value, Value::Num(10.0));
    }

    #[test]
    fn for_is_rejected_by_name() {
        let error = run("for each in cards do 1").unwrap_err();
        assert!(error.0.contains("not implemented: for"));
    }

    #[test]
    fn a_missing_function_is_named() {
        let error = run("top_of_textbox()").unwrap_err();
        assert_eq!(error.0, "missing function: top_of_textbox");
    }
}
