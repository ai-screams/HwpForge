//! HancomEQN → LaTeX parser.

use super::lexer::{tokenize, Token};

/// Maps a HancomEQN keyword or text token to its LaTeX equivalent.
fn keyword_to_latex(kw: &str) -> &str {
    match kw {
        // Two-char operators
        "<=" => r"\le",
        ">=" => r"\ge",
        "!=" => r"\ne",
        // Arithmetic operators
        "times" => r"\times",
        "div" => r"\div",
        "cdot" => r"\cdot",
        "cdots" => r"\cdots",
        "ldots" => r"\ldots",
        "vdots" => r"\vdots",
        "ddots" => r"\ddots",
        "pm" => r"\pm",
        "mp" => r"\mp",
        // Relations
        "approx" => r"\approx",
        "equiv" => r"\equiv",
        "leq" => r"\le",
        "geq" => r"\ge",
        "neq" => r"\ne",
        "therefore" => r"\therefore",
        "because" => r"\because",
        // Arrows
        "rightarrow" => r"\rightarrow",
        "leftarrow" => r"\leftarrow",
        "Rightarrow" => r"\Rightarrow",
        "Leftarrow" => r"\Leftarrow",
        "uparrow" => r"\uparrow",
        "downarrow" => r"\downarrow",
        // Calculus / logic
        "partial" => r"\partial",
        "nabla" => r"\nabla",
        "forall" => r"\forall",
        "exists" => r"\exists",
        "inf" => r"\infty",
        // Sets
        "in" => r"\in",
        "notin" => r"\notin",
        "subset" => r"\subset",
        "supset" => r"\supset",
        "subseteq" => r"\subseteq",
        "supseteq" => r"\supseteq",
        "cap" => r"\cap",
        "cup" => r"\cup",
        "emptyset" => r"\emptyset",
        // Delimiters
        "lfloor" => r"\lfloor",
        "rfloor" => r"\rfloor",
        "lceil" => r"\lceil",
        "rceil" => r"\rceil",
        "langle" => r"\langle",
        "rangle" => r"\rangle",
        // Functions
        "log" => r"\log",
        "sin" => r"\sin",
        "cos" => r"\cos",
        "tan" => r"\tan",
        "exp" => r"\exp",
        "mod" => r"\bmod",
        "prime" => r"'",
        // Greek lowercase
        "alpha" => r"\alpha",
        "beta" => r"\beta",
        "gamma" => r"\gamma",
        "delta" => r"\delta",
        "epsilon" => r"\epsilon",
        "varepsilon" => r"\varepsilon",
        "zeta" => r"\zeta",
        "eta" => r"\eta",
        "theta" => r"\theta",
        "vartheta" => r"\vartheta",
        "iota" => r"\iota",
        "kappa" => r"\kappa",
        "lambda" => r"\lambda",
        "mu" => r"\mu",
        "nu" => r"\nu",
        "xi" => r"\xi",
        "pi" => r"\pi",
        "varpi" => r"\varpi",
        "rho" => r"\rho",
        "varrho" => r"\varrho",
        "sigma" => r"\sigma",
        "varsigma" => r"\varsigma",
        "tau" => r"\tau",
        "upsilon" => r"\upsilon",
        "phi" => r"\phi",
        "varphi" => r"\varphi",
        "chi" => r"\chi",
        "psi" => r"\psi",
        "omega" => r"\omega",
        // Greek uppercase
        "ALPHA" => r"\Alpha",
        "BETA" => r"\Beta",
        "GAMMA" => r"\Gamma",
        "DELTA" => r"\Delta",
        "EPSILON" => r"\Epsilon",
        "ZETA" => r"\Zeta",
        "ETA" => r"\Eta",
        "THETA" => r"\Theta",
        "IOTA" => r"\Iota",
        "KAPPA" => r"\Kappa",
        "LAMBDA" => r"\Lambda",
        "MU" => r"\Mu",
        "NU" => r"\Nu",
        "XI" => r"\Xi",
        "PI" => r"\Pi",
        "RHO" => r"\Rho",
        "SIGMA" => r"\Sigma",
        "TAU" => r"\Tau",
        "UPSILON" => r"\Upsilon",
        "PHI" => r"\Phi",
        "CHI" => r"\Chi",
        "PSI" => r"\Psi",
        "OMEGA" => r"\Omega",
        // Pass-through for unknown
        other => other,
    }
}

/// Strips one layer of outer `{...}` if present and balanced.
fn strip_braces(s: &str) -> &str {
    // Use strip_prefix/strip_suffix for inherent UTF-8 safety.
    if let Some(inner) = s.strip_prefix('{').and_then(|s| s.strip_suffix('}')) {
        // Verify inner braces are balanced
        let mut depth = 0i32;
        for c in inner.chars() {
            match c {
                '{' => depth += 1,
                '}' => {
                    depth -= 1;
                    if depth < 0 {
                        return s;
                    }
                }
                _ => {}
            }
        }
        if depth == 0 {
            inner
        } else {
            s
        }
    } else {
        s
    }
}

/// Appends `piece` to `out`, inserting a space when two alphanumeric
/// sequences would otherwise merge (e.g. `\le` followed by `b`).
fn append_spaced(out: &mut String, piece: &str) {
    if !piece.is_empty() && !out.is_empty() {
        let last = out.as_bytes().last().copied().unwrap_or(0);
        let first = piece.as_bytes().first().copied().unwrap_or(0);
        if last.is_ascii_alphanumeric() && first.is_ascii_alphanumeric() {
            out.push(' ');
        }
    }
    out.push_str(piece);
}

/// Parser state: token slice + cursor.
struct Parser<'a> {
    tokens: &'a [Token],
    pos: usize,
}

impl<'a> Parser<'a> {
    fn new(tokens: &'a [Token]) -> Self {
        Self { tokens, pos: 0 }
    }

    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.pos)
    }

    fn advance(&mut self) -> Option<&Token> {
        let t = self.tokens.get(self.pos);
        if t.is_some() {
            self.pos += 1;
        }
        t
    }

    /// Consume one token if it matches the predicate.
    fn consume_if<F: Fn(&Token) -> bool>(&mut self, f: F) -> bool {
        if self.peek().map(f).unwrap_or(false) {
            self.pos += 1;
            true
        } else {
            false
        }
    }

    /// Parse a brace-delimited group `{ ... }` and return the inner LaTeX.
    /// If the next token is NOT `{`, parse a single atom instead.
    fn parse_group(&mut self) -> String {
        if matches!(self.peek(), Some(Token::LBrace)) {
            self.advance(); // consume `{`
            let inner = self.parse_expr_until_rbrace();
            self.consume_if(|t| matches!(t, Token::RBrace));
            inner
        } else {
            // Single-token atom
            self.parse_atom()
        }
    }

    /// Parse expression tokens until `}` or end-of-input, returning inner LaTeX.
    fn parse_expr_until_rbrace(&mut self) -> String {
        let mut out = String::new();
        loop {
            match self.peek() {
                None | Some(Token::RBrace) => break,
                _ => {
                    let piece = self.parse_one();
                    append_spaced(&mut out, &piece);
                }
            }
        }
        out
    }

    /// Parse expression tokens until `}`, `##` (two consecutive Hash), or end.
    /// Used inside matrix/cases rows.
    fn parse_expr_until_rbrace_or_rowsep(&mut self) -> String {
        let mut out = String::new();
        loop {
            // Peek ahead: two consecutive Hash = row separator
            if let Some(Token::Hash) = self.peek() {
                if self.pos + 1 < self.tokens.len() {
                    if let Token::Hash = &self.tokens[self.pos + 1] {
                        break; // stop at `##`
                    }
                }
                // Single `#` = column separator — stop and let caller handle
                break;
            }
            match self.peek() {
                None | Some(Token::RBrace) => break,
                _ => {
                    let piece = self.parse_one();
                    append_spaced(&mut out, &piece);
                }
            }
        }
        out
    }

    /// Parse a single atom (no operator handling).
    fn parse_atom(&mut self) -> String {
        match self.peek() {
            None => String::new(),
            Some(Token::LBrace) => {
                self.advance();
                let inner = self.parse_expr_until_rbrace();
                self.consume_if(|t| matches!(t, Token::RBrace));
                format!("{{{}}}", inner)
            }
            Some(Token::RBrace) => {
                // Stray `}` at top level — consume to avoid infinite loop.
                self.advance();
                String::new()
            }
            Some(Token::Underscore) => {
                self.advance();
                String::new()
            }
            Some(Token::Caret) => {
                self.advance();
                String::new()
            }
            Some(Token::Hash) => {
                self.advance();
                String::new()
            }
            Some(Token::Keyword(kw)) => {
                let kw = kw.clone();
                self.advance();
                keyword_to_latex(&kw).to_string()
            }
            Some(Token::Text(t)) => {
                let t = t.clone();
                self.advance();
                t
            }
        }
    }

    /// Parse `from { ... }` if present, returning the subscript content.
    fn try_parse_from(&mut self) -> Option<String> {
        if matches!(self.peek(), Some(Token::Keyword(k)) if k == "from") {
            self.advance();
            Some(self.parse_group())
        } else {
            None
        }
    }

    /// Parse `to { ... }` if present, returning the superscript content.
    fn try_parse_to(&mut self) -> Option<String> {
        if matches!(self.peek(), Some(Token::Keyword(k)) if k == "to") {
            self.advance();
            Some(self.parse_group())
        } else {
            None
        }
    }

    /// Parse one "statement" including postfix `over`, `_`, `^`.
    fn parse_one(&mut self) -> String {
        let base = self.parse_primary();

        // Check for postfix `over` (fraction).
        // Uses `parse_one()` for the denominator so that chained `a over b over c`
        // and postfix sub/superscripts `a over b^2` are handled correctly.
        if matches!(self.peek(), Some(Token::Keyword(k)) if k == "over") {
            self.advance(); // consume `over`
            let denom = self.parse_one();
            return format!(r"\frac{{{}}}{{{}}}", strip_braces(&base), strip_braces(&denom));
        }

        // Check for `_` and `^` postfix
        let mut result = base;
        loop {
            match self.peek() {
                Some(Token::Underscore) => {
                    self.advance();
                    let sub = self.parse_group();
                    result = format!("{}_{{{}}}", result, sub);
                }
                Some(Token::Caret) => {
                    self.advance();
                    let sup = self.parse_group();
                    result = format!("{}^{{{}}}", result, sup);
                }
                _ => break,
            }
        }

        result
    }

    /// Parse a primary expression (keyword handlers, atoms).
    fn parse_primary(&mut self) -> String {
        match self.peek().cloned() {
            None => String::new(),

            Some(Token::LBrace) => {
                // Grouped expression — parse as group, then check for `over`
                self.advance(); // consume `{`
                let inner = self.parse_expr_until_rbrace();
                self.consume_if(|t| matches!(t, Token::RBrace));
                // The outer caller (parse_one) will handle `over`
                format!("{{{}}}", inner)
            }

            Some(Token::Keyword(ref kw)) => {
                let kw = kw.clone();
                match kw.as_str() {
                    "sqrt" => {
                        self.advance();
                        let arg = self.parse_group();
                        format!(r"\sqrt{{{}}}", arg)
                    }
                    "sum" | "int" | "prod" | "lim" => {
                        self.advance();
                        let cmd = match kw.as_str() {
                            "sum" => r"\sum",
                            "int" => r"\int",
                            "prod" => r"\prod",
                            "lim" => r"\lim",
                            _ => unreachable!(),
                        };
                        let sub = self.try_parse_from();
                        let sup = self.try_parse_to();
                        let mut s = cmd.to_string();
                        if let Some(sub) = sub {
                            s.push_str(&format!("_{{{}}}", sub));
                        }
                        if let Some(sup) = sup {
                            s.push_str(&format!("^{{{}}}", sup));
                        }
                        s
                    }
                    "vec" | "hat" | "bar" | "dot" | "tilde" => {
                        self.advance();
                        let arg = self.parse_group();
                        let cmd = match kw.as_str() {
                            "vec" => r"\vec",
                            "hat" => r"\hat",
                            "bar" => r"\overline",
                            "dot" => r"\dot",
                            "tilde" => r"\tilde",
                            _ => unreachable!(),
                        };
                        format!("{}{{{}}}", cmd, arg)
                    }
                    "left" => {
                        self.advance();
                        // next token should be a delimiter character
                        let delim = self.parse_atom();
                        format!(r"\left{}", delim)
                    }
                    "right" => {
                        self.advance();
                        let delim = self.parse_atom();
                        format!(r"\right{}", delim)
                    }
                    "matrix" => {
                        self.advance();
                        self.parse_matrix_env("pmatrix")
                    }
                    "cases" => {
                        self.advance();
                        self.parse_cases_env()
                    }
                    // `from` and `to` without a preceding operator — pass as keyword
                    _ => {
                        self.advance();
                        keyword_to_latex(&kw).to_string()
                    }
                }
            }

            // Non-keyword text or other tokens
            _ => self.parse_atom(),
        }
    }

    /// Parse `{ cell # cell ## cell # cell }` → `\begin{pmatrix}...\end{pmatrix}`.
    fn parse_matrix_env(&mut self, env: &str) -> String {
        // Consume opening `{`
        if !self.consume_if(|t| matches!(t, Token::LBrace)) {
            return String::new();
        }
        let mut rows: Vec<Vec<String>> = vec![vec![]];

        loop {
            match self.peek() {
                None | Some(Token::RBrace) => {
                    // Flush current cell
                    break;
                }
                Some(Token::Hash) => {
                    // Check for `##` (row separator) vs `#` (col separator)
                    if self.pos + 1 < self.tokens.len() {
                        if let Token::Hash = &self.tokens[self.pos + 1] {
                            // Row separator `##`
                            self.advance();
                            self.advance();
                            rows.push(vec![]);
                            continue;
                        }
                    }
                    // Column separator `#` — just advance; the next cell will
                    // be parsed and pushed by the default branch.
                    self.advance();
                    continue;
                }
                _ => {
                    let cell = self.parse_expr_until_rbrace_or_rowsep();
                    // rows is always non-empty: initialized with vec![vec![]] and only grows.
                    rows.last_mut().unwrap().push(cell);
                }
            }
        }
        self.consume_if(|t| matches!(t, Token::RBrace));

        let body = rows.into_iter().map(|row| row.join(" & ")).collect::<Vec<_>>().join(r" \\ ");
        format!(r"\begin{{{env}}}{body}\end{{{env}}}")
    }

    /// Parse `{ expr ## expr }` → `\begin{cases}...\end{cases}`.
    fn parse_cases_env(&mut self) -> String {
        if !self.consume_if(|t| matches!(t, Token::LBrace)) {
            return String::new();
        }
        let mut rows: Vec<String> = Vec::new();

        loop {
            match self.peek() {
                None | Some(Token::RBrace) => break,
                Some(Token::Hash) => {
                    if self.pos + 1 < self.tokens.len() {
                        if let Token::Hash = &self.tokens[self.pos + 1] {
                            self.advance();
                            self.advance();
                            continue;
                        }
                    }
                    // Single `#` inside cases treated as column sep — just skip
                    self.advance();
                }
                _ => {
                    let expr = self.parse_expr_until_rbrace_or_rowsep();
                    rows.push(expr);
                }
            }
        }
        self.consume_if(|t| matches!(t, Token::RBrace));

        let body = rows.join(r" \\ ");
        format!(r"\begin{{cases}}{body}\end{{cases}}")
    }

    /// Parse the entire token stream.
    fn parse_all(&mut self) -> String {
        let mut out = String::new();
        while self.peek().is_some() {
            let piece = self.parse_one();
            append_spaced(&mut out, &piece);
        }
        out
    }
}

/// Converts a HancomEQN script string to LaTeX.
///
/// Returns the LaTeX expression wrapped in `$...$` for inline math.
/// Unknown constructs are passed through as-is.
pub fn eqn_to_latex(script: &str) -> String {
    let tokens = tokenize(script);
    let mut parser = Parser::new(&tokens);
    let latex = parser.parse_all();
    format!("${}$", latex)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn simple_fraction() {
        assert_eq!(eqn_to_latex("{a+b} over {c+d}"), "$\\frac{a+b}{c+d}$");
    }

    #[test]
    fn square_root() {
        assert_eq!(eqn_to_latex("sqrt {x+1}"), "$\\sqrt{x+1}$");
    }

    #[test]
    fn summation() {
        assert_eq!(eqn_to_latex("sum from {i=1} to {n} a_i"), "$\\sum_{i=1}^{n}a_{i}$");
    }

    #[test]
    fn integral() {
        assert_eq!(eqn_to_latex("int from {0} to {1} f(x)dx"), "$\\int_{0}^{1}f(x)dx$");
    }

    #[test]
    fn subscript_superscript() {
        assert_eq!(eqn_to_latex("x^2 + y_i"), "$x^{2}+y_{i}$");
    }

    #[test]
    fn greek_letters() {
        assert_eq!(eqn_to_latex("alpha + beta"), "$\\alpha+\\beta$");
    }

    #[test]
    fn comparison_operators() {
        assert_eq!(eqn_to_latex("a <= b"), "$a\\le b$");
    }

    #[test]
    fn empty_input() {
        assert_eq!(eqn_to_latex(""), "$$");
    }

    #[test]
    fn plain_text_passthrough() {
        assert_eq!(eqn_to_latex("x + y"), "$x+y$");
    }

    #[test]
    fn nested_fraction() {
        assert_eq!(eqn_to_latex("{1} over {{a} over {b}}"), "$\\frac{1}{\\frac{a}{b}}$");
    }

    #[test]
    fn vector_accent() {
        assert_eq!(eqn_to_latex("vec {a}"), "$\\vec{a}$");
    }

    #[test]
    fn limit() {
        assert_eq!(eqn_to_latex("lim from {x rightarrow 0}"), "$\\lim_{x\\rightarrow 0}$");
    }

    #[test]
    fn chained_fraction() {
        assert_eq!(eqn_to_latex("a over b over c"), "$\\frac{a}{\\frac{b}{c}}$");
    }

    #[test]
    fn fraction_with_superscript_denom() {
        assert_eq!(eqn_to_latex("a over b^2"), "$\\frac{a}{b^{2}}$");
    }

    #[test]
    fn matrix_2x2() {
        assert_eq!(
            eqn_to_latex("matrix {a # b ## c # d}"),
            "$\\begin{pmatrix}a & b \\\\ c & d\\end{pmatrix}$"
        );
    }

    #[test]
    fn cases_env() {
        assert_eq!(eqn_to_latex("cases {x ## -x}"), "$\\begin{cases}x \\\\ -x\\end{cases}$");
    }

    #[test]
    fn left_right_delimiters() {
        assert_eq!(eqn_to_latex("left ( a + b right )"), "$\\left(a+b\\right)$");
    }

    #[test]
    fn floor_ceil_delimiters() {
        assert_eq!(eqn_to_latex("lfloor x rfloor"), "$\\lfloor x\\rfloor$");
        assert_eq!(eqn_to_latex("lceil x rceil"), "$\\lceil x\\rceil$");
    }

    #[test]
    fn trig_functions() {
        assert_eq!(eqn_to_latex("sin theta"), "$\\sin\\theta$");
        assert_eq!(eqn_to_latex("cos alpha"), "$\\cos\\alpha$");
        assert_eq!(eqn_to_latex("log x"), "$\\log x$");
    }

    #[test]
    fn prime_and_mod() {
        assert_eq!(eqn_to_latex("f prime"), "$f'$");
        assert_eq!(eqn_to_latex("a mod b"), "$a\\bmod b$");
    }
}
