use std::collections::{BTreeMap, BTreeSet};

/// Modifier bits used for hotkey conflict detection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Modifier {
    Ctrl,
    Alt,
    Shift,
}

/// A 3DMigoto binding split into its parts.
///
/// Syntax (from d3dx.ini): `key = ctrl alt VK_F1`, with negative constraints
/// `no_ctrl`, `no_alt`, `no_shift`, or `no_modifiers` to require a bare key.
/// Unspecified modifiers are not required, which is why `F1` and `CTRL VK_F1`
/// can both fire on Ctrl+F1.
#[derive(Debug, Clone, Default)]
pub struct ParsedBinding {
    pub required: BTreeSet<Modifier>,
    pub forbidden: BTreeSet<Modifier>,
    /// Canonical key token, e.g. `VK_F1` or `X`.
    pub key: Option<String>,
    /// Optional condition expression, e.g. `$active == 1`.
    pub condition: Option<String>,
}

impl ParsedBinding {
    pub fn is_empty(&self) -> bool {
        self.key.is_none()
    }

    pub fn with_condition(mut self, condition: Option<String>) -> Self {
        self.condition = condition;
        self
    }
}

/// Map the many ways a key can be written onto one canonical token.
pub fn canonical_key(token: &str) -> String {
    let upper = token.trim().to_ascii_uppercase();

    if upper.starts_with("VK_") {
        return upper;
    }

    match upper.as_str() {
        "ESC" | "ESCAPE" => "VK_ESCAPE".to_string(),
        "RETURN" | "ENTER" => "VK_RETURN".to_string(),
        "BACK" | "BACKSPACE" => "VK_BACK".to_string(),
        "DEL" | "DELETE" => "VK_DELETE".to_string(),
        "INS" | "INSERT" => "VK_INSERT".to_string(),
        "PGUP" | "PAGEUP" | "PRIOR" => "VK_PRIOR".to_string(),
        "PGDN" | "PAGEDOWN" | "NEXT" => "VK_NEXT".to_string(),
        "UP" => "VK_UP".to_string(),
        "DOWN" => "VK_DOWN".to_string(),
        "LEFT" => "VK_LEFT".to_string(),
        "RIGHT" => "VK_RIGHT".to_string(),
        "SPACE" | "SPACEBAR" => "VK_SPACE".to_string(),
        "TAB" => "VK_TAB".to_string(),
        "CAPS" | "CAPSLOCK" => "VK_CAPITAL".to_string(),
        "PLUS" => "VK_OEM_PLUS".to_string(),
        "MINUS" => "VK_OEM_MINUS".to_string(),
        _ => upper,
    }
}

pub fn parse_binding(raw: &str) -> ParsedBinding {
    let mut parsed = ParsedBinding::default();

    for token in raw.split_whitespace() {
        let normalized = token.trim().to_ascii_uppercase().replace('-', "_");

        match normalized.as_str() {
            "CTRL" | "CONTROL" => {
                parsed.required.insert(Modifier::Ctrl);
            }
            "ALT" => {
                parsed.required.insert(Modifier::Alt);
            }
            "SHIFT" => {
                parsed.required.insert(Modifier::Shift);
            }
            "NO_MODIFIERS" | "NO_MODIFIER" => {
                parsed.forbidden.insert(Modifier::Ctrl);
                parsed.forbidden.insert(Modifier::Alt);
                parsed.forbidden.insert(Modifier::Shift);
            }
            "NO_CTRL" | "NO_CONTROL" => {
                parsed.forbidden.insert(Modifier::Ctrl);
            }
            "NO_ALT" => {
                parsed.forbidden.insert(Modifier::Alt);
            }
            "NO_SHIFT" => {
                parsed.forbidden.insert(Modifier::Shift);
            }
            // Key-exclusion tokens (`no_vk_decimal`) are rare and only refine a
            // binding; they never make two bindings safe from each other.
            other if other.starts_with("NO_VK_") => {}
            other if !other.is_empty() && parsed.key.is_none() => {
                parsed.key = Some(canonical_key(other));
            }
            _ => {}
        }
    }

    parsed
}

/// True when a single key press can satisfy both bindings.
///
/// Required modifiers are additive in 3DMigoto, so the *union* of both required
/// sets must not violate either binding's forbidden set.
///
/// In addition, if both bindings specify a `condition` expression, those
/// expressions must be able to hold true at the same time.
pub fn can_fire_together(a: &ParsedBinding, b: &ParsedBinding) -> bool {
    let (Some(left), Some(right)) = (a.key.as_deref(), b.key.as_deref()) else {
        return false;
    };

    if left != right {
        return false;
    }

    if !a.required
        .union(&b.required)
        .all(|modifier| !a.forbidden.contains(modifier) && !b.forbidden.contains(modifier))
    {
        return false;
    }

    conditions_can_overlap(a.condition.as_deref(), b.condition.as_deref())
}

/// True when condition `a` and condition `b` can both be satisfied at the same time.
///
/// Unconditional bindings (missing or empty condition) can always fire, so they
/// overlap with any other condition. When both conditions are specified, they
/// are evaluated across possible variable assignments; if no assignment can
/// make both true (e.g. `$active == 1` vs `$active == 2`), they cannot fire
/// together and this returns `false`.
pub fn conditions_can_overlap(a: Option<&str>, b: Option<&str>) -> bool {
    let (Some(a_raw), Some(b_raw)) = (a, b) else {
        return true;
    };
    let a_trim = strip_outer_quotes(a_raw.trim());
    let b_trim = strip_outer_quotes(b_raw.trim());
    if a_trim.is_empty() || b_trim.is_empty() {
        return true;
    }
    if a_trim.eq_ignore_ascii_case(b_trim) {
        return true;
    }

    let (Some(expr_a), Some(expr_b)) = (parse_condition(a_trim), parse_condition(b_trim)) else {
        // Fail open if an expression cannot be parsed.
        return true;
    };

    if has_relational_string_cmp(&expr_a) || has_relational_string_cmp(&expr_b) {
        // Relational string comparisons (<, <=, >, >=) have continuous ordering
        // intervals that cannot be reliably partitioned by point samples; fail open.
        return true;
    }

    let mut vars = BTreeSet::new();
    let mut nums = BTreeMap::new();
    let mut strs = BTreeMap::new();

    collect_domains(&expr_a, &mut vars, &mut nums, &mut strs);
    collect_domains(&expr_b, &mut vars, &mut nums, &mut strs);

    if vars.is_empty() {
        let empty = BTreeMap::new();
        return expr_a.eval(&empty) && expr_b.eval(&empty);
    }

    if vars.len() > 5 {
        return true;
    }

    let mut domains = Vec::new();
    for v in &vars {
        let mut domain = Vec::new();

        if let Some(mut raw_nums) = nums.remove(v) {
            raw_nums.push(0.0);
            raw_nums.sort_by(|x, y| x.partial_cmp(y).unwrap_or(std::cmp::Ordering::Equal));
            raw_nums.dedup_by(|x, y| (*x - *y).abs() < 1e-6);

            if let Some(&first) = raw_nums.first() {
                domain.push(Val::Number(first - 1.0));
            }
            for i in 0..raw_nums.len() {
                domain.push(Val::Number(raw_nums[i]));
                if i + 1 < raw_nums.len() {
                    domain.push(Val::Number((raw_nums[i] + raw_nums[i + 1]) / 2.0));
                }
            }
            if let Some(&last) = raw_nums.last() {
                domain.push(Val::Number(last + 1.0));
            }
        }

        if let Some(raw_strs) = strs.remove(v) {
            for s in raw_strs {
                domain.push(Val::Str(s));
            }
            domain.push(Val::Str("__other__".to_string()));
        }

        if domain.is_empty() {
            domain.push(Val::Number(0.0));
            domain.push(Val::Number(1.0));
            domain.push(Val::Number(2.0));
        }

        if !domain.iter().any(|val| !val.is_truthy()) {
            domain.push(Val::Number(0.0));
        }

        domains.push((v.clone(), domain));
    }

    let total_combinations: usize = domains.iter().map(|(_, d)| d.len()).product();
    if total_combinations > 1000 {
        return true;
    }

    let mut env = BTreeMap::new();
    can_satisfy_both(&domains, 0, &mut env, &expr_a, &expr_b)
}

fn strip_outer_quotes(s: &str) -> &str {
    if (s.starts_with('"') && s.ends_with('"')) || (s.starts_with('\'') && s.ends_with('\'')) {
        if s.len() >= 2 {
            let quote = s.chars().next().unwrap();
            let inner = &s[1..s.len() - 1];
            if !inner.contains(quote) {
                return inner.trim();
            }
        }
    }
    s
}

fn can_satisfy_both(
    domains: &[(String, Vec<Val>)],
    index: usize,
    env: &mut BTreeMap<String, Val>,
    expr_a: &Expr,
    expr_b: &Expr,
) -> bool {
    if index == domains.len() {
        return expr_a.eval(env) && expr_b.eval(env);
    }

    let (var_name, values) = &domains[index];
    for val in values {
        env.insert(var_name.clone(), val.clone());
        if can_satisfy_both(domains, index + 1, env, expr_a, expr_b) {
            return true;
        }
    }
    env.remove(var_name);

    false
}

#[derive(Debug, Clone, PartialEq)]
enum Val {
    Number(f64),
    Str(String),
}

impl Val {
    fn is_truthy(&self) -> bool {
        match self {
            Val::Number(n) => *n != 0.0,
            Val::Str(s) => !s.is_empty() && s != "0",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
enum Expr {
    Compare(Box<Operand>, CmpOp, Box<Operand>),
    Truthy(Operand),
    And(Box<Expr>, Box<Expr>),
    Or(Box<Expr>, Box<Expr>),
    Not(Box<Expr>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CmpOp {
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
}

#[derive(Debug, Clone, PartialEq)]
enum Operand {
    Var(String),
    Number(f64),
    Str(String),
}

impl Expr {
    fn eval(&self, env: &BTreeMap<String, Val>) -> bool {
        match self {
            Expr::Not(inner) => !inner.eval(env),
            Expr::And(left, right) => left.eval(env) && right.eval(env),
            Expr::Or(left, right) => left.eval(env) || right.eval(env),
            Expr::Truthy(op) => eval_operand(op, env).is_truthy(),
            Expr::Compare(left, op, right) => {
                let v_left = eval_operand(left, env);
                let v_right = eval_operand(right, env);
                eval_cmp(&v_left, *op, &v_right)
            }
        }
    }
}

fn eval_operand(op: &Operand, env: &BTreeMap<String, Val>) -> Val {
    match op {
        Operand::Var(name) => env.get(name).cloned().unwrap_or(Val::Number(0.0)),
        Operand::Number(n) => Val::Number(*n),
        Operand::Str(s) => Val::Str(s.clone()),
    }
}

fn eval_cmp(left: &Val, op: CmpOp, right: &Val) -> bool {
    match (left, right) {
        (Val::Number(a), Val::Number(b)) => match op {
            CmpOp::Eq => (a - b).abs() < 1e-9,
            CmpOp::Ne => (a - b).abs() >= 1e-9,
            CmpOp::Lt => a < b,
            CmpOp::Le => a <= b,
            CmpOp::Gt => a > b,
            CmpOp::Ge => a >= b,
        },
        (Val::Str(a), Val::Str(b)) => {
            let a = a.to_ascii_lowercase();
            let b = b.to_ascii_lowercase();
            match op {
                CmpOp::Eq => a == b,
                CmpOp::Ne => a != b,
                CmpOp::Lt => a < b,
                CmpOp::Le => a <= b,
                CmpOp::Gt => a > b,
                CmpOp::Ge => a >= b,
            }
        }
        (Val::Number(a), Val::Str(b)) => {
            if let Ok(b_num) = b.parse::<f64>() {
                eval_cmp(&Val::Number(*a), op, &Val::Number(b_num))
            } else {
                eval_cmp(&Val::Str(a.to_string()), op, &Val::Str(b.clone()))
            }
        }
        (Val::Str(a), Val::Number(b)) => {
            if let Ok(a_num) = a.parse::<f64>() {
                eval_cmp(&Val::Number(a_num), op, &Val::Number(*b))
            } else {
                eval_cmp(&Val::Str(a.clone()), op, &Val::Str(b.to_string()))
            }
        }
    }
}

fn collect_domains(
    expr: &Expr,
    vars: &mut BTreeSet<String>,
    nums: &mut BTreeMap<String, Vec<f64>>,
    strs: &mut BTreeMap<String, BTreeSet<String>>,
) {
    match expr {
        Expr::Not(inner) => collect_domains(inner, vars, nums, strs),
        Expr::And(left, right) | Expr::Or(left, right) => {
            collect_domains(left, vars, nums, strs);
            collect_domains(right, vars, nums, strs);
        }
        Expr::Truthy(op) => {
            if let Operand::Var(name) = op {
                vars.insert(name.clone());
            }
        }
        Expr::Compare(left, _op, right) => {
            match (&**left, &**right) {
                (Operand::Var(name), Operand::Number(n))
                | (Operand::Number(n), Operand::Var(name)) => {
                    vars.insert(name.clone());
                    nums.entry(name.clone()).or_default().push(*n);
                }
                (Operand::Var(name), Operand::Str(s))
                | (Operand::Str(s), Operand::Var(name)) => {
                    vars.insert(name.clone());
                    strs.entry(name.clone()).or_default().insert(s.to_ascii_lowercase());
                }
                (Operand::Var(a), Operand::Var(b)) => {
                    vars.insert(a.clone());
                    vars.insert(b.clone());
                }
                _ => {}
            }
        }
    }
}
fn has_relational_string_cmp(expr: &Expr) -> bool {
    match expr {
        Expr::Not(inner) => has_relational_string_cmp(inner),
        Expr::And(left, right) | Expr::Or(left, right) => {
            has_relational_string_cmp(left) || has_relational_string_cmp(right)
        }
        Expr::Truthy(_) => false,
        Expr::Compare(left, op, right) => {
            let is_relational = matches!(op, CmpOp::Lt | CmpOp::Le | CmpOp::Gt | CmpOp::Ge);
            if !is_relational {
                return false;
            }
            matches!(
                (&**left, &**right),
                (Operand::Str(_), _) | (_, Operand::Str(_))
            )
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
enum Token {
    Var(String),
    Number(f64),
    Str(String),
    Op(String),
    LParen,
    RParen,
}

fn tokenize_condition(expr: &str) -> Vec<Token> {
    let mut chars = expr.chars().peekable();
    let mut tokens = Vec::new();

    while let Some(&ch) = chars.peek() {
        if ch.is_whitespace() || ch == ',' {
            chars.next();
            continue;
        }

        if ch == ';' {
            break;
        }

        if ch == '(' {
            chars.next();
            tokens.push(Token::LParen);
            continue;
        }

        if ch == ')' {
            chars.next();
            tokens.push(Token::RParen);
            continue;
        }

        if ch == '"' || ch == '\'' {
            chars.next();
            let quote = ch;
            let mut s = String::new();
            while let Some(&c) = chars.peek() {
                chars.next();
                if c == quote {
                    break;
                }
                s.push(c);
            }
            tokens.push(Token::Str(s.to_ascii_lowercase()));
            continue;
        }

        if ch == '$' {
            chars.next();
            let mut name = String::new();
            while let Some(&c) = chars.peek() {
                if c.is_whitespace() || "()!<>=&|,;\"'~".contains(c) {
                    break;
                }
                name.push(c);
                chars.next();
            }
            if !name.is_empty() {
                tokens.push(Token::Var(name.to_ascii_lowercase()));
            }
            continue;
        }

        if "!<>=&|~".contains(ch) {
            chars.next();
            let next_ch = chars.peek().copied();
            match (ch, next_ch) {
                ('=', Some('=')) => {
                    chars.next();
                    tokens.push(Token::Op("==".to_string()));
                }
                ('!', Some('=')) | ('~', Some('=')) => {
                    chars.next();
                    tokens.push(Token::Op("!=".to_string()));
                }
                ('<', Some('=')) => {
                    chars.next();
                    tokens.push(Token::Op("<=".to_string()));
                }
                ('>', Some('=')) => {
                    chars.next();
                    tokens.push(Token::Op(">=".to_string()));
                }
                ('&', Some('&')) => {
                    chars.next();
                    tokens.push(Token::Op("&&".to_string()));
                }
                ('|', Some('|')) => {
                    chars.next();
                    tokens.push(Token::Op("||".to_string()));
                }
                ('&', _) => {
                    tokens.push(Token::Op("&&".to_string()));
                }
                ('|', _) => {
                    tokens.push(Token::Op("||".to_string()));
                }
                ('=', _) => {
                    tokens.push(Token::Op("==".to_string()));
                }
                ('!', _) => {
                    tokens.push(Token::Op("!".to_string()));
                }
                ('<', _) => {
                    tokens.push(Token::Op("<".to_string()));
                }
                ('>', _) => {
                    tokens.push(Token::Op(">".to_string()));
                }
                _ => {}
            }
            continue;
        }

        if ch == '-' {
            chars.next();
            if let Some(&next) = chars.peek() {
                if next.is_ascii_digit() {
                    let mut num_str = String::from("-");
                    while let Some(&c) = chars.peek() {
                        if c.is_ascii_digit() || c == '.' {
                            num_str.push(c);
                            chars.next();
                        } else {
                            break;
                        }
                    }
                    if let Ok(num) = num_str.parse::<f64>() {
                        tokens.push(Token::Number(num));
                        continue;
                    }
                }
            }
        }

        let mut word = String::new();
        while let Some(&c) = chars.peek() {
            if c.is_whitespace() || "()!<>=&|,;\"'$~".contains(c) {
                break;
            }
            word.push(c);
            chars.next();
        }

        if word.is_empty() {
            chars.next();
            continue;
        }

        if let Ok(num) = word.parse::<f64>() {
            tokens.push(Token::Number(num));
        } else {
            let lower = word.to_ascii_lowercase();
            match lower.as_str() {
                "and" => tokens.push(Token::Op("&&".to_string())),
                "or" => tokens.push(Token::Op("||".to_string())),
                "not" => tokens.push(Token::Op("!".to_string())),
                "true" => tokens.push(Token::Number(1.0)),
                "false" => tokens.push(Token::Number(0.0)),
                _ => tokens.push(Token::Str(lower)),
            }
        }
    }

    tokens
}

struct ConditionParser {
    tokens: Vec<Token>,
    pos: usize,
}

impl ConditionParser {
    fn new(tokens: Vec<Token>) -> Self {
        Self { tokens, pos: 0 }
    }

    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.pos)
    }

    fn next_token(&mut self) -> Option<Token> {
        let t = self.tokens.get(self.pos).cloned();
        if t.is_some() {
            self.pos += 1;
        }
        t
    }

    fn expect_op(&mut self, op: &str) -> bool {
        if let Some(Token::Op(s)) = self.peek() {
            if s == op {
                self.pos += 1;
                return true;
            }
        }
        false
    }

    fn parse_expression(&mut self) -> Option<Expr> {
        self.parse_or()
    }

    fn parse_or(&mut self) -> Option<Expr> {
        let mut left = self.parse_and()?;
        while self.expect_op("||") {
            let right = self.parse_and()?;
            left = Expr::Or(Box::new(left), Box::new(right));
        }
        Some(left)
    }

    fn parse_and(&mut self) -> Option<Expr> {
        let mut left = self.parse_unary()?;
        while self.expect_op("&&") {
            let right = self.parse_unary()?;
            left = Expr::And(Box::new(left), Box::new(right));
        }
        Some(left)
    }

    fn parse_unary(&mut self) -> Option<Expr> {
        if self.expect_op("!") {
            let inner = self.parse_unary()?;
            return Some(Expr::Not(Box::new(inner)));
        }

        if let Some(Token::LParen) = self.peek() {
            self.pos += 1;
            let expr = self.parse_or()?;
            if let Some(Token::RParen) = self.peek() {
                self.pos += 1;
                return Some(expr);
            }
            return None;
        }

        self.parse_comparison_or_truthy()
    }

    fn parse_comparison_or_truthy(&mut self) -> Option<Expr> {
        let left_op = self.parse_operand()?;

        if let Some(Token::Op(op_str)) = self.peek() {
            let cmp_op = match op_str.as_str() {
                "==" => Some(CmpOp::Eq),
                "!=" => Some(CmpOp::Ne),
                "<=" => Some(CmpOp::Le),
                ">=" => Some(CmpOp::Ge),
                "<" => Some(CmpOp::Lt),
                ">" => Some(CmpOp::Gt),
                _ => None,
            };

            if let Some(op) = cmp_op {
                self.pos += 1;
                let right_op = self.parse_operand()?;
                return Some(Expr::Compare(Box::new(left_op), op, Box::new(right_op)));
            }
        }

        Some(Expr::Truthy(left_op))
    }

    fn parse_operand(&mut self) -> Option<Operand> {
        match self.next_token()? {
            Token::Var(name) => Some(Operand::Var(name)),
            Token::Number(num) => Some(Operand::Number(num)),
            Token::Str(s) => Some(Operand::Str(s)),
            _ => None,
        }
    }
}

fn parse_condition(expr_str: &str) -> Option<Expr> {
    let tokens = tokenize_condition(expr_str);
    if tokens.is_empty() {
        return None;
    }
    let mut parser = ConditionParser::new(tokens);
    let expr = parser.parse_expression()?;
    if parser.pos != parser.tokens.len() {
        return None;
    }
    Some(expr)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_modifiers_and_keys() {
        let binding = parse_binding("ctrl alt VK_F1");
        assert_eq!(binding.key.as_deref(), Some("VK_F1"));
        assert!(binding.required.contains(&Modifier::Ctrl));
        assert!(binding.required.contains(&Modifier::Alt));
        assert!(!binding.required.contains(&Modifier::Shift));
        assert!(binding.forbidden.is_empty());
    }

    #[test]
    fn normalizes_key_aliases() {
        assert_eq!(parse_binding("f1").key.as_deref(), Some("F1"));
        assert_eq!(parse_binding("esc").key.as_deref(), Some("VK_ESCAPE"));
        assert_eq!(parse_binding("vk_escape").key.as_deref(), Some("VK_ESCAPE"));
        assert_eq!(parse_binding("x").key.as_deref(), Some("X"));
    }

    #[test]
    fn unmodified_and_modified_bindings_can_overlap() {
        let plain = parse_binding("VK_F1");
        let ctrl = parse_binding("ctrl VK_F1");
        assert!(can_fire_together(&plain, &ctrl));
    }

    #[test]
    fn no_modifiers_excludes_modified_bindings() {
        let bare = parse_binding("no_modifiers VK_F1");
        let ctrl = parse_binding("ctrl VK_F1");
        assert!(!can_fire_together(&bare, &ctrl));

        let no_ctrl = parse_binding("no_ctrl VK_F1");
        assert!(!can_fire_together(&no_ctrl, &ctrl));
        assert!(can_fire_together(&no_ctrl, &parse_binding("alt VK_F1")));
    }

    #[test]
    fn different_keys_never_conflict() {
        assert!(!can_fire_together(
            &parse_binding("ctrl VK_F1"),
            &parse_binding("ctrl VK_F2")
        ));
        assert!(!can_fire_together(
            &parse_binding(""),
            &parse_binding("ctrl VK_F2")
        ));
    }

    #[test]
    fn unconditional_bindings_overlap_with_anything() {
        assert!(conditions_can_overlap(None, None));
        assert!(conditions_can_overlap(Some("$active == 1"), None));
        assert!(conditions_can_overlap(None, Some("$active == 1")));
        assert!(conditions_can_overlap(Some(""), Some("$active == 1")));
        assert!(can_fire_together(
            &parse_binding("VK_F1"),
            &parse_binding("VK_F1").with_condition(Some("$active == 1".to_string()))
        ));
    }

    #[test]
    fn mutually_exclusive_constants_do_not_conflict() {
        let a = parse_binding("VK_F1").with_condition(Some("$active == 1".to_string()));
        let b = parse_binding("VK_F1").with_condition(Some("$active == 2".to_string()));
        assert!(!can_fire_together(&a, &b));

        let c = parse_binding("VK_F1").with_condition(Some("$active == 0".to_string()));
        assert!(!can_fire_together(&a, &c));

        let same = parse_binding("VK_F1").with_condition(Some("$active == 1".to_string()));
        assert!(can_fire_together(&a, &same));
    }

    #[test]
    fn understands_inequalities_and_negations() {
        assert!(!conditions_can_overlap(
            Some("$active == 1"),
            Some("$active != 1")
        ));
        assert!(conditions_can_overlap(
            Some("$active == 1"),
            Some("$active != 2")
        ));
        assert!(!conditions_can_overlap(Some("$active"), Some("!$active")));
        assert!(!conditions_can_overlap(
            Some("!$active"),
            Some("$active == 1")
        ));
        assert!(conditions_can_overlap(
            Some("!$active"),
            Some("$active == 0")
        ));
        assert!(conditions_can_overlap(
            Some("$active"),
            Some("$active == 1")
        ));
    }

    #[test]
    fn handles_different_variables_and_boolean_combinations() {
        // Independent variables can be satisfied simultaneously.
        assert!(conditions_can_overlap(
            Some("$char == 1"),
            Some("$mode == 2")
        ));

        // Conjunctions with a contradictory variable cannot fire together.
        assert!(!conditions_can_overlap(
            Some("$char == 1 && $mode == 0"),
            Some("$char == 2 && $mode == 0")
        ));
        assert!(conditions_can_overlap(
            Some("$char == 1 && $mode == 0"),
            Some("$char == 1 && $mode == 0")
        ));

        // Disjunctions overlap if any branch can satisfy both.
        assert!(!conditions_can_overlap(
            Some("$x == 1 || $x == 2"),
            Some("$x == 3")
        ));
        assert!(conditions_can_overlap(
            Some("$x == 1 || $x == 2"),
            Some("$x == 2")
        ));
    }

    #[test]
    fn handles_numerical_ranges() {
        assert!(!conditions_can_overlap(Some("$x > 5"), Some("$x < 3")));
        assert!(!conditions_can_overlap(Some("$x > 5"), Some("$x <= 5")));
        assert!(conditions_can_overlap(Some("$x >= 5"), Some("$x <= 5")));
        assert!(conditions_can_overlap(Some("$x > 5"), Some("$x < 10")));
    }

    #[test]
    fn handles_string_constants_and_single_equals() {
        assert!(!conditions_can_overlap(
            Some("$name == 'Jane'"),
            Some("$name == 'Ellen'")
        ));
        assert!(conditions_can_overlap(
            Some("$name == 'Jane'"),
            Some("$name == 'Jane'")
        ));
        assert!(!conditions_can_overlap(
            Some("$name == Jane"),
            Some("$name == Ellen")
        ));
        // Single '=' used in 3DMigoto
        assert!(!conditions_can_overlap(
            Some("$active = 1"),
            Some("$active = 2")
        ));
        assert!(conditions_can_overlap(
            Some("$active = 1"),
            Some("$active = 1")
        ));
    }

    #[test]
    fn fails_open_on_unparseable_syntax() {
        assert!(conditions_can_overlap(
            Some("invalid ?? expression"),
            Some("$active == 1")
        ));
    }

    #[test]
    fn fails_open_on_relational_string_comparisons() {
        assert!(conditions_can_overlap(
            Some("$name > 'a' && $name < 'z'"),
            Some("$name > 'b' && $name < 'y'")
        ));
    }
}
