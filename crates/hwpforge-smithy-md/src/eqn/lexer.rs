//! HancomEQN tokenizer.

/// Tokens produced by the HancomEQN lexer.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Token {
    /// Keyword like `over`, `sqrt`, `sum`, `from`, `to`, etc.
    Keyword(String),
    /// Opening brace `{`
    LBrace,
    /// Closing brace `}`
    RBrace,
    /// Subscript `_`
    Underscore,
    /// Superscript `^`
    Caret,
    /// Hash separator `#` (for matrix/cases)
    Hash,
    /// Plain text or symbol
    Text(String),
}

/// Known HancomEQN keywords (operators, accents, environments, symbols).
static KEYWORDS: &[&str] = &[
    // Structural
    "over",
    "sqrt",
    "sum",
    "int",
    "prod",
    "lim",
    "from",
    "to",
    "left",
    "right",
    "matrix",
    "cases",
    // Accents
    "vec",
    "hat",
    "bar",
    "dot",
    "tilde",
    // Operators (multi-char text forms)
    "times",
    "div",
    "cdot",
    "cdots",
    "ldots",
    "vdots",
    "ddots",
    "pm",
    "mp",
    // Relations
    "approx",
    "equiv",
    "therefore",
    "because",
    "leq",
    "geq",
    "neq",
    // Arrows
    "rightarrow",
    "leftarrow",
    "Rightarrow",
    "Leftarrow",
    "uparrow",
    "downarrow",
    // Set / logic
    "partial",
    "nabla",
    "forall",
    "exists",
    "in",
    "notin",
    "subset",
    "supset",
    "subseteq",
    "supseteq",
    "cap",
    "cup",
    "emptyset",
    // Misc
    "inf",
    // Delimiters
    "lfloor",
    "rfloor",
    "lceil",
    "rceil",
    "langle",
    "rangle",
    // Functions
    "log",
    "sin",
    "cos",
    "tan",
    "exp",
    "mod",
    "prime",
    // Greek lowercase
    "alpha",
    "beta",
    "gamma",
    "delta",
    "epsilon",
    "varepsilon",
    "zeta",
    "eta",
    "theta",
    "vartheta",
    "iota",
    "kappa",
    "lambda",
    "mu",
    "nu",
    "xi",
    "pi",
    "varpi",
    "rho",
    "varrho",
    "sigma",
    "varsigma",
    "tau",
    "upsilon",
    "phi",
    "varphi",
    "chi",
    "psi",
    "omega",
    // Greek uppercase
    "ALPHA",
    "BETA",
    "GAMMA",
    "DELTA",
    "EPSILON",
    "ZETA",
    "ETA",
    "THETA",
    "IOTA",
    "KAPPA",
    "LAMBDA",
    "MU",
    "NU",
    "XI",
    "PI",
    "RHO",
    "SIGMA",
    "TAU",
    "UPSILON",
    "PHI",
    "CHI",
    "PSI",
    "OMEGA",
];

/// Returns true if the identifier is a known keyword.
fn is_keyword(s: &str) -> bool {
    KEYWORDS.contains(&s)
}

/// Tokenizes a HancomEQN script into a flat token stream.
pub(crate) fn tokenize(input: &str) -> Vec<Token> {
    let mut tokens = Vec::new();
    let chars: Vec<char> = input.chars().collect();
    let mut i = 0;

    while i < chars.len() {
        match chars[i] {
            // Skip plain whitespace — preserve one space as Text(" ")
            ' ' | '\t' | '\n' | '\r' => {
                i += 1;
            }
            '{' => {
                tokens.push(Token::LBrace);
                i += 1;
            }
            '}' => {
                tokens.push(Token::RBrace);
                i += 1;
            }
            '_' => {
                tokens.push(Token::Underscore);
                i += 1;
            }
            '^' => {
                tokens.push(Token::Caret);
                i += 1;
            }
            '#' => {
                // `##` → double hash (row separator in matrix/cases)
                if i + 1 < chars.len() && chars[i + 1] == '#' {
                    tokens.push(Token::Hash);
                    tokens.push(Token::Hash);
                    i += 2;
                } else {
                    tokens.push(Token::Hash);
                    i += 1;
                }
            }
            // Two-char operators
            '<' if i + 1 < chars.len() && chars[i + 1] == '=' => {
                tokens.push(Token::Keyword("<=".to_string()));
                i += 2;
            }
            '>' if i + 1 < chars.len() && chars[i + 1] == '=' => {
                tokens.push(Token::Keyword(">=".to_string()));
                i += 2;
            }
            '!' if i + 1 < chars.len() && chars[i + 1] == '=' => {
                tokens.push(Token::Keyword("!=".to_string()));
                i += 2;
            }
            // Identifier or keyword
            c if c.is_alphabetic() => {
                let start = i;
                while i < chars.len() && chars[i].is_alphanumeric() {
                    i += 1;
                }
                let word: String = chars[start..i].iter().collect();
                if is_keyword(&word) {
                    tokens.push(Token::Keyword(word));
                } else {
                    tokens.push(Token::Text(word));
                }
            }
            // Numbers
            c if c.is_ascii_digit() => {
                let start = i;
                while i < chars.len() && (chars[i].is_ascii_digit() || chars[i] == '.') {
                    i += 1;
                }
                let num: String = chars[start..i].iter().collect();
                tokens.push(Token::Text(num));
            }
            // Everything else passes through as single-char Text
            c => {
                tokens.push(Token::Text(c.to_string()));
                i += 1;
            }
        }
    }

    tokens
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokenize_over() {
        let toks = tokenize("{a} over {b}");
        assert_eq!(
            toks,
            vec![
                Token::LBrace,
                Token::Text("a".into()),
                Token::RBrace,
                Token::Keyword("over".into()),
                Token::LBrace,
                Token::Text("b".into()),
                Token::RBrace,
            ]
        );
    }

    #[test]
    fn tokenize_sub_sup() {
        let toks = tokenize("x_i^2");
        assert_eq!(
            toks,
            vec![
                Token::Text("x".into()),
                Token::Underscore,
                Token::Text("i".into()),
                Token::Caret,
                Token::Text("2".into()),
            ]
        );
    }

    #[test]
    fn tokenize_comparison() {
        let toks = tokenize("a <= b");
        assert_eq!(
            toks,
            vec![Token::Text("a".into()), Token::Keyword("<=".into()), Token::Text("b".into()),]
        );
    }

    #[test]
    fn tokenize_hash_double() {
        let toks = tokenize("a # b ## c");
        assert_eq!(
            toks,
            vec![
                Token::Text("a".into()),
                Token::Hash,
                Token::Text("b".into()),
                Token::Hash,
                Token::Hash,
                Token::Text("c".into()),
            ]
        );
    }
}
