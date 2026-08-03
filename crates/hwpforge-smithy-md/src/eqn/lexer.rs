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
    // Hancom font/style switches
    "rm",
    "it",
    "bold",
    "box",
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
    "le",
    "ge",
    "ne",
    "lt",
    "gt",
    "sim",
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

/// Uppercase commands commonly emitted by Hancom equation editors.
///
/// HancomEQN permits commands to be attached to following operands when a
/// case transition provides a boundary (`THEREFOREk`). A preceding operand
/// requires a real token boundary (`P LEFT`) so ordinary identifiers remain
/// indivisible.
const UPPERCASE_COMMANDS: &[(&str, &str)] = &[
    ("THEREFORE", "therefore"),
    ("BECAUSE", "because"),
    ("TIMES", "times"),
    ("CDOTS", "cdots"),
    ("RIGHT", "right"),
    ("LEFT", "left"),
    ("LEQ", "leq"),
    ("GEQ", "geq"),
    ("NEQ", "neq"),
    ("SIM", "sim"),
];

fn push_identifier_tokens(word: &str, tokens: &mut Vec<Token>) {
    if word.is_empty() {
        return;
    }
    if is_keyword(word) {
        tokens.push(Token::Keyword(word.to_string()));
        return;
    }
    if let Some((_, canonical)) = UPPERCASE_COMMANDS.iter().find(|(source, _)| word == *source) {
        tokens.push(Token::Keyword((*canonical).to_string()));
        return;
    }

    let attached_operand = UPPERCASE_COMMANDS.iter().find_map(|(source, canonical)| {
        let attached_operand = word
            .strip_prefix(source)
            .filter(|suffix| suffix.chars().next().is_some_and(|next| !next.is_ascii_uppercase()));
        attached_operand.map(|_| (*source, *canonical))
    });
    if let Some((source, canonical)) = attached_operand {
        tokens.push(Token::Keyword(canonical.to_string()));
        push_identifier_tokens(&word[source.len()..], tokens);
        return;
    }

    tokens.push(Token::Text(word.to_string()));
}

/// Tokenizes a HancomEQN script into a flat token stream.
pub(crate) fn tokenize(input: &str) -> Vec<Token> {
    let mut tokens = Vec::new();
    let chars: Vec<char> = input.chars().collect();
    let mut i = 0;

    while i < chars.len() {
        match chars[i] {
            // Hancom uses backticks as visual spacing markers. LaTeX spacing is
            // reconstructed by the parser, so they must not leak into output.
            ' ' | '\t' | '\n' | '\r' | '`' => {
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
            '\\' => {
                if i + 1 >= chars.len() {
                    tokens.push(Token::Text("\\".to_string()));
                    i += 1;
                    continue;
                }

                if chars[i + 1].is_ascii_alphabetic() {
                    let mut command_end = i + 1;
                    while command_end < chars.len() && chars[command_end].is_ascii_alphabetic() {
                        command_end += 1;
                    }
                    if command_end == i + 2 {
                        // HancomEQN uses `\A`-style escapes for literal Latin
                        // labels. Skip only the escape marker and let the
                        // ordinary identifier path consume the operand.
                        i += 1;
                    } else {
                        // Preserve multi-letter LaTeX commands as one token so
                        // known Hancom keywords do not add a second backslash.
                        tokens.push(Token::Text(chars[i..command_end].iter().collect()));
                        i = command_end;
                    }
                    continue;
                }

                // Keep symbolic LaTeX escapes (`\{`, `\_`, `\\`, and so on)
                // together so their second character is not parsed as
                // HancomEQN structure.
                tokens.push(Token::Text(chars[i..=i + 1].iter().collect()));
                i += 2;
            }
            // Identifier or keyword
            c if c.is_alphabetic() => {
                let start = i;
                while i < chars.len() && chars[i].is_alphanumeric() {
                    i += 1;
                }
                let word: String = chars[start..i].iter().collect();
                push_identifier_tokens(&word, &mut tokens);
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

    #[test]
    fn tokenize_hancom_spacing_markers_as_whitespace() {
        assert_eq!(
            tokenize("a,````b`"),
            vec![Token::Text("a".into()), Token::Text(",".into()), Token::Text("b".into()),]
        );
    }

    #[test]
    fn tokenize_uppercase_and_attached_hancom_commands() {
        assert_eq!(
            tokenize("P LEFT(x RIGHT),~THEREFOREk LEQ 1 TIMES 2 CDOTS"),
            vec![
                Token::Text("P".into()),
                Token::Keyword("left".into()),
                Token::Text("(".into()),
                Token::Text("x".into()),
                Token::Keyword("right".into()),
                Token::Text(")".into()),
                Token::Text(",".into()),
                Token::Text("~".into()),
                Token::Keyword("therefore".into()),
                Token::Text("k".into()),
                Token::Keyword("leq".into()),
                Token::Text("1".into()),
                Token::Keyword("times".into()),
                Token::Text("2".into()),
                Token::Keyword("cdots".into()),
            ]
        );
    }

    #[test]
    fn ordinary_uppercase_identifiers_do_not_match_command_substrings() {
        assert_eq!(
            tokenize("SIMPLE+BRIGHT+CLEFT"),
            vec![
                Token::Text("SIMPLE".into()),
                Token::Text("+".into()),
                Token::Text("BRIGHT".into()),
                Token::Text("+".into()),
                Token::Text("CLEFT".into()),
            ]
        );
    }

    #[test]
    fn ordinary_lowercase_identifiers_do_not_match_command_prefixes() {
        assert_eq!(
            tokenize("item+barometer+bars+its"),
            vec![
                Token::Text("item".into()),
                Token::Text("+".into()),
                Token::Text("barometer".into()),
                Token::Text("+".into()),
                Token::Text("bars".into()),
                Token::Text("+".into()),
                Token::Text("its".into()),
            ]
        );
    }

    #[test]
    fn tokenize_root_accent_fraction_and_style_commands_at_boundaries() {
        assert_eq!(
            tokenize("sqrt a+bar z+3 over k+rm O+it f"),
            vec![
                Token::Keyword("sqrt".into()),
                Token::Text("a".into()),
                Token::Text("+".into()),
                Token::Keyword("bar".into()),
                Token::Text("z".into()),
                Token::Text("+".into()),
                Token::Text("3".into()),
                Token::Keyword("over".into()),
                Token::Text("k".into()),
                Token::Text("+".into()),
                Token::Keyword("rm".into()),
                Token::Text("O".into()),
                Token::Text("+".into()),
                Token::Keyword("it".into()),
                Token::Text("f".into()),
            ]
        );
        assert_eq!(
            tokenize("sqrta+barz+overk+rmO+itf"),
            vec![
                Token::Text("sqrta".into()),
                Token::Text("+".into()),
                Token::Text("barz".into()),
                Token::Text("+".into()),
                Token::Text("overk".into()),
                Token::Text("+".into()),
                Token::Text("rmO".into()),
                Token::Text("+".into()),
                Token::Text("itf".into()),
            ]
        );
    }
}
