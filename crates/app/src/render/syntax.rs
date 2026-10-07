use std::ops::Range;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Token {
    Keyword,
    String,
    Number,
    Comment,
}

struct Grammar {
    line_comments: &'static [&'static str],
    block_comment: Option<(&'static str, &'static str)>,
    keywords: &'static [&'static str],
    char_quotes: bool,
}

const C_LIKE_KEYWORDS: &[&str] = &[
    "abstract",
    "as",
    "async",
    "await",
    "break",
    "case",
    "catch",
    "class",
    "const",
    "continue",
    "default",
    "do",
    "else",
    "enum",
    "export",
    "extends",
    "false",
    "final",
    "finally",
    "fn",
    "for",
    "from",
    "func",
    "function",
    "go",
    "if",
    "impl",
    "import",
    "in",
    "interface",
    "let",
    "loop",
    "match",
    "mod",
    "mut",
    "namespace",
    "new",
    "null",
    "override",
    "package",
    "private",
    "protected",
    "pub",
    "public",
    "readonly",
    "return",
    "self",
    "static",
    "struct",
    "super",
    "switch",
    "this",
    "throw",
    "trait",
    "true",
    "try",
    "type",
    "typeof",
    "use",
    "using",
    "var",
    "void",
    "where",
    "while",
    "yield",
];
const PYTHON_KEYWORDS: &[&str] = &[
    "and", "as", "async", "await", "break", "class", "continue", "def", "del", "elif", "else",
    "except", "False", "finally", "for", "from", "if", "import", "in", "is", "lambda", "None",
    "not", "or", "pass", "raise", "return", "True", "try", "while", "with", "yield",
];
const SHELL_KEYWORDS: &[&str] = &[
    "case", "do", "done", "elif", "else", "esac", "export", "fi", "for", "function", "if", "in",
    "local", "return", "then", "while",
];
const SQL_KEYWORDS: &[&str] = &[
    "and", "as", "by", "create", "delete", "from", "group", "insert", "into", "join", "left",
    "not", "null", "on", "or", "order", "select", "set", "table", "update", "values", "where",
];

const C_LIKE: Grammar = Grammar {
    line_comments: &["//"],
    block_comment: Some(("/*", "*/")),
    keywords: C_LIKE_KEYWORDS,
    char_quotes: false,
};
const CHAR_LIKE: Grammar = Grammar {
    char_quotes: true,
    ..C_LIKE
};
const HASH: Grammar = Grammar {
    line_comments: &["#"],
    block_comment: None,
    keywords: PYTHON_KEYWORDS,
    char_quotes: false,
};
const SHELL: Grammar = Grammar {
    line_comments: &["#"],
    block_comment: None,
    keywords: SHELL_KEYWORDS,
    char_quotes: false,
};
const SQL: Grammar = Grammar {
    line_comments: &["--"],
    block_comment: Some(("/*", "*/")),
    keywords: SQL_KEYWORDS,
    char_quotes: false,
};
const MARKUP: Grammar = Grammar {
    line_comments: &[],
    block_comment: Some(("<!--", "-->")),
    keywords: &[],
    char_quotes: false,
};

fn grammar_for(language: &str) -> Option<&'static Grammar> {
    match language.to_ascii_lowercase().as_str() {
        "rust" | "rs" | "c" | "cpp" | "c++" | "csharp" | "cs" | "c#" | "java" | "go" | "kotlin"
        | "scala" => Some(&CHAR_LIKE),
        "javascript" | "js" | "jsx" | "typescript" | "ts" | "tsx" | "swift" | "php" | "json"
        | "css" | "scss" | "less" | "dart" => Some(&C_LIKE),
        "python" | "py" | "ruby" | "rb" | "yaml" | "yml" | "toml" | "r" | "perl" | "dockerfile"
        | "makefile" => Some(&HASH),
        "bash" | "sh" | "shell" | "zsh" | "powershell" | "ps1" => Some(&SHELL),
        "sql" | "tsql" | "plsql" | "lua" => Some(&SQL),
        "html" | "xml" | "svg" | "markdown" | "md" => Some(&MARKUP),
        _ => None,
    }
}

pub fn highlight(code: &str, language: &str) -> Option<Vec<(Range<usize>, Token)>> {
    let grammar = grammar_for(language)?;
    let sql = grammar.keywords == SQL_KEYWORDS;
    let mut tokens = Vec::new();
    let mut index = 0;
    while index < code.len() {
        let rest = &code[index..];
        if let Some((open, close)) = grammar.block_comment
            && rest.starts_with(open)
        {
            let end = rest[open.len()..].find(close).map_or(code.len(), |offset| {
                index + open.len() + offset + close.len()
            });
            tokens.push((index..end, Token::Comment));
            index = end;
            continue;
        }
        if grammar
            .line_comments
            .iter()
            .any(|marker| rest.starts_with(marker))
        {
            let end = rest.find('\n').map_or(code.len(), |offset| index + offset);
            tokens.push((index..end, Token::Comment));
            index = end;
            continue;
        }
        let character = rest.chars().next().unwrap_or_default();
        if character == '\'' && grammar.char_quotes && !is_char_literal(rest) {
            index += 1;
            continue;
        }
        if matches!(character, '"' | '\'' | '`') {
            let end = string_end(code, index, character);
            tokens.push((index..end, Token::String));
            index = end;
            continue;
        }
        if character.is_ascii_digit() {
            let length = rest
                .find(|next: char| !(next.is_ascii_alphanumeric() || next == '.' || next == '_'))
                .unwrap_or(rest.len());
            tokens.push((index..index + length, Token::Number));
            index += length;
            continue;
        }
        if character.is_alphabetic() || character == '_' {
            let length = rest
                .find(|next: char| !(next.is_alphanumeric() || next == '_'))
                .unwrap_or(rest.len());
            let word = &rest[..length];
            let is_keyword = if sql {
                grammar
                    .keywords
                    .iter()
                    .any(|keyword| keyword.eq_ignore_ascii_case(word))
            } else {
                grammar.keywords.contains(&word)
            };
            if is_keyword {
                tokens.push((index..index + length, Token::Keyword));
            }
            index += length;
            continue;
        }
        index += character.len_utf8();
    }
    Some(tokens)
}

fn is_char_literal(rest: &str) -> bool {
    let mut characters = rest.chars().skip(1);
    match characters.next() {
        Some('\\') => characters.take(9).any(|character| character == '\''),
        Some('\n') | None => false,
        Some(_) => characters.next() == Some('\''),
    }
}

fn string_end(code: &str, start: usize, quote: char) -> usize {
    let mut escaped = false;
    for (offset, character) in code[start + quote.len_utf8()..].char_indices() {
        if character == '\n' && quote != '`' {
            return start + quote.len_utf8() + offset;
        }
        if escaped {
            escaped = false;
        } else if character == '\\' {
            escaped = true;
        } else if character == quote {
            return start + quote.len_utf8() + offset + character.len_utf8();
        }
    }
    code.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds<'a>(code: &'a str, language: &str) -> Vec<(&'a str, Token)> {
        highlight(code, language)
            .unwrap()
            .into_iter()
            .map(|(range, token)| (&code[range], token))
            .collect()
    }

    #[test]
    fn rust_keywords_strings_and_comments() {
        assert_eq!(
            kinds("// x\nfn a() { \"b\" }", "rust"),
            vec![
                ("// x", Token::Comment),
                ("fn", Token::Keyword),
                ("\"b\"", Token::String),
            ]
        );
    }

    #[test]
    fn escaped_quote_stays_in_string() {
        assert_eq!(kinds(r#""a\"b" 1"#, "js")[0].0, r#""a\"b""#);
    }

    #[test]
    fn unterminated_string_stops_at_line_end() {
        assert_eq!(
            kinds("x = 'abc\nreturn", "python"),
            vec![("'abc", Token::String), ("return", Token::Keyword)]
        );
    }

    #[test]
    fn rust_lifetime_is_not_a_string() {
        assert_eq!(
            kinds("fn a<'b>(x: &'b str) -> char { 'c' }", "rust"),
            vec![("fn", Token::Keyword), ("'c'", Token::String),]
        );
        assert_eq!(kinds(r"'\n'", "rust"), vec![(r"'\n'", Token::String)]);
    }

    #[test]
    fn sql_keywords_ignore_case() {
        assert_eq!(kinds("SELECT 1", "sql")[0], ("SELECT", Token::Keyword));
    }

    #[test]
    fn unknown_language_has_no_highlight() {
        assert!(highlight("x", "brainfuck").is_none());
    }

    #[test]
    fn identifier_with_digits_is_not_a_number() {
        assert!(kinds("let x1 = 2;", "rust").contains(&("2", Token::Number)));
        assert!(
            !kinds("let x1 = 2;", "rust")
                .iter()
                .any(|(text, _)| *text == "1")
        );
    }
}
