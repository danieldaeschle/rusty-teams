use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};
use std::hash::{Hash, Hasher};
use std::ops::Range;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Token {
    Keyword,
    String,
    Number,
    Comment,
    Tag,
    Attribute,
    Heading,
    Strong,
    Emphasis,
    Link,
    /// Built-in types, functions and values: `bool`, `print`, `self`.
    Type,
    /// The name a definition gives: `f` in `def f`, `A` in `class A`.
    Title,
}

/// A lexer for code: comments, strings, numbers and keywords.
struct Grammar {
    line_comments: &'static [&'static str],
    block_comment: Option<(&'static str, &'static str)>,
    keywords: &'static [&'static str],
    /// `'x'` is a character; other single quotes (Rust lifetimes) are left alone.
    char_quotes: bool,
    case_insensitive: bool,
    /// Words after this prefix are keywords too, as `\section` in LaTeX.
    command_prefix: Option<char>,
    types: &'static [&'static str],
    /// A word or string right before `:` is a key, as in JSON, YAML and CSS.
    keys: bool,
    /// `-` joins words, as in `font-size` and `Get-ChildItem`; such a word is a built-in.
    dashed_words: bool,
}

const CODE: Grammar = Grammar {
    line_comments: &[],
    block_comment: None,
    keywords: &[],
    char_quotes: false,
    case_insensitive: false,
    command_prefix: None,
    types: &[],
    keys: false,
    dashed_words: false,
};

const fn c_style(keywords: &'static [&'static str]) -> Grammar {
    Grammar {
        line_comments: &["//"],
        block_comment: Some(("/*", "*/")),
        keywords,
        ..CODE
    }
}

const fn hash_style(keywords: &'static [&'static str]) -> Grammar {
    Grammar {
        line_comments: &["#"],
        keywords,
        ..CODE
    }
}

const C_KEYWORDS: &[&str] = &[
    "auto",
    "break",
    "case",
    "char",
    "const",
    "continue",
    "default",
    "do",
    "double",
    "else",
    "enum",
    "extern",
    "float",
    "for",
    "goto",
    "if",
    "inline",
    "int",
    "long",
    "register",
    "restrict",
    "return",
    "short",
    "signed",
    "sizeof",
    "static",
    "struct",
    "switch",
    "typedef",
    "union",
    "unsigned",
    "void",
    "volatile",
    "while",
    "NULL",
    "true",
    "false",
    "bool",
    "#include",
    "#define",
    "@interface",
    "@implementation",
    "@end",
    "@property",
    "self",
    "nil",
];
const CPP_KEYWORDS: &[&str] = &[
    "auto",
    "bool",
    "break",
    "case",
    "catch",
    "char",
    "class",
    "const",
    "constexpr",
    "continue",
    "default",
    "delete",
    "do",
    "double",
    "else",
    "enum",
    "explicit",
    "extern",
    "false",
    "float",
    "for",
    "friend",
    "goto",
    "if",
    "inline",
    "int",
    "long",
    "namespace",
    "new",
    "noexcept",
    "nullptr",
    "operator",
    "override",
    "private",
    "protected",
    "public",
    "return",
    "short",
    "signed",
    "sizeof",
    "static",
    "struct",
    "switch",
    "template",
    "this",
    "throw",
    "true",
    "try",
    "typedef",
    "typename",
    "union",
    "unsigned",
    "using",
    "virtual",
    "void",
    "volatile",
    "while",
];
const CSHARP_KEYWORDS: &[&str] = &[
    "abstract",
    "as",
    "async",
    "await",
    "base",
    "bool",
    "break",
    "case",
    "catch",
    "class",
    "const",
    "continue",
    "default",
    "delegate",
    "do",
    "else",
    "enum",
    "event",
    "false",
    "finally",
    "for",
    "foreach",
    "get",
    "if",
    "in",
    "int",
    "interface",
    "internal",
    "is",
    "new",
    "null",
    "object",
    "out",
    "override",
    "private",
    "protected",
    "public",
    "readonly",
    "ref",
    "return",
    "sealed",
    "set",
    "static",
    "string",
    "struct",
    "switch",
    "this",
    "throw",
    "true",
    "try",
    "using",
    "var",
    "virtual",
    "void",
    "while",
];
const JAVA_KEYWORDS: &[&str] = &[
    "abstract",
    "boolean",
    "break",
    "case",
    "catch",
    "char",
    "class",
    "const",
    "continue",
    "default",
    "do",
    "double",
    "else",
    "enum",
    "extends",
    "false",
    "final",
    "finally",
    "float",
    "for",
    "if",
    "implements",
    "import",
    "instanceof",
    "int",
    "interface",
    "long",
    "new",
    "null",
    "package",
    "private",
    "protected",
    "public",
    "return",
    "static",
    "super",
    "switch",
    "this",
    "throw",
    "throws",
    "true",
    "try",
    "var",
    "void",
    "while",
];
const JS_KEYWORDS: &[&str] = &[
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
    "delete",
    "do",
    "else",
    "enum",
    "export",
    "extends",
    "false",
    "finally",
    "for",
    "from",
    "function",
    "if",
    "implements",
    "import",
    "in",
    "instanceof",
    "interface",
    "let",
    "new",
    "null",
    "of",
    "private",
    "protected",
    "public",
    "readonly",
    "return",
    "static",
    "super",
    "switch",
    "this",
    "throw",
    "true",
    "try",
    "type",
    "typeof",
    "undefined",
    "var",
    "void",
    "while",
    "yield",
];
const DART_KEYWORDS: &[&str] = &[
    "abstract",
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
    "extends",
    "false",
    "final",
    "finally",
    "for",
    "if",
    "implements",
    "import",
    "in",
    "is",
    "late",
    "new",
    "null",
    "required",
    "return",
    "static",
    "super",
    "switch",
    "this",
    "throw",
    "true",
    "try",
    "var",
    "void",
    "while",
    "with",
    "yield",
];
const GO_KEYWORDS: &[&str] = &[
    "break",
    "case",
    "chan",
    "const",
    "continue",
    "default",
    "defer",
    "else",
    "fallthrough",
    "for",
    "func",
    "go",
    "goto",
    "if",
    "import",
    "interface",
    "map",
    "package",
    "range",
    "return",
    "select",
    "struct",
    "switch",
    "type",
    "var",
    "true",
    "false",
    "nil",
    "iota",
];
const KOTLIN_KEYWORDS: &[&str] = &[
    "as",
    "break",
    "class",
    "continue",
    "data",
    "do",
    "else",
    "false",
    "for",
    "fun",
    "if",
    "import",
    "in",
    "interface",
    "is",
    "null",
    "object",
    "override",
    "package",
    "private",
    "return",
    "super",
    "this",
    "throw",
    "true",
    "try",
    "val",
    "var",
    "when",
    "while",
];
const RUST_KEYWORDS: &[&str] = &[
    "as", "async", "await", "break", "const", "continue", "crate", "dyn", "else", "enum", "extern",
    "false", "fn", "for", "if", "impl", "in", "let", "loop", "match", "mod", "move", "mut", "pub",
    "ref", "return", "self", "Self", "static", "struct", "super", "trait", "true", "type",
    "unsafe", "use", "where", "while",
];
const SCALA_KEYWORDS: &[&str] = &[
    "abstract",
    "case",
    "catch",
    "class",
    "def",
    "do",
    "else",
    "extends",
    "false",
    "final",
    "finally",
    "for",
    "if",
    "implicit",
    "import",
    "lazy",
    "match",
    "new",
    "null",
    "object",
    "override",
    "package",
    "private",
    "protected",
    "return",
    "sealed",
    "super",
    "this",
    "throw",
    "trait",
    "true",
    "try",
    "type",
    "val",
    "var",
    "while",
    "with",
    "yield",
];
const SWIFT_KEYWORDS: &[&str] = &[
    "as",
    "break",
    "case",
    "catch",
    "class",
    "continue",
    "default",
    "defer",
    "do",
    "else",
    "enum",
    "extension",
    "false",
    "for",
    "func",
    "guard",
    "if",
    "import",
    "in",
    "init",
    "let",
    "nil",
    "private",
    "protocol",
    "public",
    "return",
    "self",
    "static",
    "struct",
    "switch",
    "throw",
    "throws",
    "true",
    "try",
    "var",
    "where",
    "while",
];
const PHP_KEYWORDS: &[&str] = &[
    "abstract",
    "array",
    "as",
    "break",
    "case",
    "catch",
    "class",
    "const",
    "continue",
    "default",
    "do",
    "echo",
    "else",
    "elseif",
    "extends",
    "false",
    "final",
    "finally",
    "for",
    "foreach",
    "function",
    "if",
    "implements",
    "include",
    "interface",
    "namespace",
    "new",
    "null",
    "private",
    "protected",
    "public",
    "require",
    "return",
    "static",
    "switch",
    "throw",
    "true",
    "try",
    "use",
    "while",
];
const PYTHON_KEYWORDS: &[&str] = &[
    "and", "as", "assert", "async", "await", "break", "class", "continue", "def", "del", "elif",
    "else", "except", "False", "finally", "for", "from", "global", "if", "import", "in", "is",
    "lambda", "None", "nonlocal", "not", "or", "pass", "raise", "return", "True", "try", "while",
    "with", "yield",
];
const RUBY_KEYWORDS: &[&str] = &[
    "and", "begin", "break", "case", "class", "def", "do", "else", "elsif", "end", "ensure",
    "false", "for", "if", "in", "module", "next", "nil", "not", "or", "require", "rescue",
    "return", "self", "super", "then", "true", "unless", "until", "when", "while", "yield",
];
const PERL_KEYWORDS: &[&str] = &[
    "else", "elsif", "for", "foreach", "if", "last", "local", "my", "next", "our", "package",
    "print", "require", "return", "sub", "unless", "until", "use", "while",
];
const R_KEYWORDS: &[&str] = &[
    "break", "else", "FALSE", "for", "function", "if", "in", "NA", "next", "NULL", "repeat",
    "return", "TRUE", "while", "library",
];
const SHELL_KEYWORDS: &[&str] = &[
    "case", "do", "done", "echo", "elif", "else", "esac", "exit", "export", "fi", "for",
    "function", "if", "in", "local", "return", "set", "then", "until", "while",
];
const POWERSHELL_KEYWORDS: &[&str] = &[
    "begin", "break", "catch", "class", "continue", "do", "else", "elseif", "end", "exit",
    "filter", "finally", "for", "foreach", "function", "if", "in", "param", "process", "return",
    "switch", "throw", "trap", "try", "until", "while", "$true", "$false", "$null",
];
const DOS_KEYWORDS: &[&str] = &[
    "call", "cd", "copy", "defined", "del", "do", "echo", "else", "endlocal", "equ", "exist",
    "exit", "for", "goto", "if", "in", "mkdir", "neq", "not", "pause", "set", "setlocal", "shift",
    "start",
];
const DOCKER_KEYWORDS: &[&str] = &[
    "add",
    "arg",
    "as",
    "cmd",
    "copy",
    "entrypoint",
    "env",
    "expose",
    "from",
    "healthcheck",
    "label",
    "onbuild",
    "run",
    "shell",
    "stopsignal",
    "user",
    "volume",
    "workdir",
];
const SQL_KEYWORDS: &[&str] = &[
    "alter", "and", "as", "asc", "between", "by", "case", "create", "delete", "desc", "distinct",
    "drop", "else", "end", "exists", "from", "group", "having", "in", "inner", "insert", "into",
    "is", "join", "left", "like", "limit", "not", "null", "on", "or", "order", "outer", "right",
    "select", "set", "table", "then", "union", "update", "values", "when", "where", "with",
];
const KQL_KEYWORDS: &[&str] = &[
    "and",
    "asc",
    "by",
    "count",
    "desc",
    "distinct",
    "extend",
    "join",
    "let",
    "limit",
    "or",
    "order",
    "project",
    "render",
    "sort",
    "summarize",
    "take",
    "top",
    "union",
    "where",
];
const GRAPHQL_KEYWORDS: &[&str] = &[
    "enum",
    "extend",
    "false",
    "fragment",
    "implements",
    "input",
    "interface",
    "mutation",
    "null",
    "on",
    "query",
    "scalar",
    "schema",
    "subscription",
    "true",
    "type",
    "union",
];
const HTTP_KEYWORDS: &[&str] = &[
    "CONNECT", "DELETE", "GET", "HEAD", "HTTP", "OPTIONS", "PATCH", "POST", "PUT", "TRACE",
];
const VB_KEYWORDS: &[&str] = &[
    "and", "as", "boolean", "byval", "byref", "call", "case", "class", "const", "dim", "do",
    "each", "else", "elseif", "end", "exit", "false", "for", "function", "if", "imports", "in",
    "integer", "is", "loop", "module", "new", "next", "not", "nothing", "or", "private", "public",
    "return", "select", "set", "string", "sub", "then", "to", "true", "while", "with",
];
const VERILOG_KEYWORDS: &[&str] = &[
    "always",
    "assign",
    "begin",
    "case",
    "else",
    "end",
    "endcase",
    "endmodule",
    "for",
    "if",
    "initial",
    "input",
    "integer",
    "module",
    "negedge",
    "output",
    "parameter",
    "posedge",
    "reg",
    "wire",
];
const VHDL_KEYWORDS: &[&str] = &[
    "architecture",
    "begin",
    "case",
    "downto",
    "else",
    "elsif",
    "end",
    "entity",
    "for",
    "generic",
    "if",
    "in",
    "is",
    "library",
    "loop",
    "map",
    "of",
    "others",
    "out",
    "port",
    "process",
    "signal",
    "then",
    "to",
    "use",
    "variable",
    "when",
    "while",
];
const OCTAVE_KEYWORDS: &[&str] = &[
    "break",
    "case",
    "catch",
    "continue",
    "else",
    "elseif",
    "end",
    "for",
    "function",
    "if",
    "otherwise",
    "return",
    "switch",
    "try",
    "while",
];
const LISP_KEYWORDS: &[&str] = &[
    "cond",
    "defmacro",
    "defparameter",
    "defun",
    "defvar",
    "if",
    "lambda",
    "let",
    "nil",
    "progn",
    "quote",
    "setf",
    "setq",
    "t",
    "unless",
    "when",
];
const LITERALS: &[&str] = &["true", "false", "null", "yes", "no", "on", "off", "~"];

const C: Grammar = Grammar {
    types: &[
        "size_t", "int8_t", "int16_t", "int32_t", "int64_t", "uint8_t", "uint16_t", "uint32_t",
        "uint64_t", "FILE", "printf", "scanf", "malloc", "free", "memcpy", "strlen", "BOOL", "YES",
        "NO", "id", "NSString", "NSLog",
    ],
    char_quotes: true,
    ..c_style(C_KEYWORDS)
};
const CPP: Grammar = Grammar {
    types: &[
        "std",
        "string",
        "vector",
        "map",
        "set",
        "size_t",
        "cout",
        "cin",
        "endl",
        "unique_ptr",
        "shared_ptr",
        "make_unique",
        "make_shared",
        "printf",
    ],
    char_quotes: true,
    ..c_style(CPP_KEYWORDS)
};
const CSHARP: Grammar = Grammar {
    types: &[
        "byte",
        "char",
        "decimal",
        "double",
        "float",
        "long",
        "Console",
        "List",
        "Dictionary",
        "Task",
        "DateTime",
        "Exception",
        "nameof",
    ],
    char_quotes: true,
    ..c_style(CSHARP_KEYWORDS)
};
const JAVA: Grammar = Grammar {
    types: &[
        "byte",
        "short",
        "String",
        "Integer",
        "Long",
        "Double",
        "Boolean",
        "Object",
        "List",
        "Map",
        "ArrayList",
        "HashMap",
        "System",
        "Exception",
        "Override",
    ],
    char_quotes: true,
    ..c_style(JAVA_KEYWORDS)
};
const RUST: Grammar = Grammar {
    types: &[
        "i8",
        "i16",
        "i32",
        "i64",
        "i128",
        "isize",
        "u8",
        "u16",
        "u32",
        "u64",
        "u128",
        "usize",
        "f32",
        "f64",
        "bool",
        "char",
        "str",
        "String",
        "Vec",
        "Option",
        "Some",
        "None",
        "Result",
        "Ok",
        "Err",
        "Box",
        "Rc",
        "Arc",
        "println",
        "print",
        "format",
        "vec",
        "panic",
        "assert",
        "assert_eq",
    ],
    char_quotes: true,
    ..c_style(RUST_KEYWORDS)
};
const KOTLIN: Grammar = Grammar {
    types: &[
        "Int", "Long", "Double", "Boolean", "String", "Unit", "Any", "List", "Map", "println",
        "listOf", "mapOf", "setOf",
    ],
    char_quotes: true,
    ..c_style(KOTLIN_KEYWORDS)
};
const SCALA: Grammar = Grammar {
    types: &[
        "Int", "Long", "Double", "Boolean", "String", "Unit", "Any", "List", "Map", "Option",
        "Some", "None", "println",
    ],
    char_quotes: true,
    ..c_style(SCALA_KEYWORDS)
};
const JS: Grammar = Grammar {
    types: &[
        "console", "document", "window", "Promise", "Array", "Object", "JSON", "Math", "Date",
        "Error", "Map", "Set", "string", "number", "boolean", "any", "unknown", "never", "require",
        "module",
    ],
    ..c_style(JS_KEYWORDS)
};
const DART: Grammar = Grammar {
    types: &[
        "int", "double", "num", "String", "bool", "List", "Map", "Set", "dynamic", "Future",
        "Stream", "print",
    ],
    ..c_style(DART_KEYWORDS)
};
const GO: Grammar = Grammar {
    types: &[
        "bool",
        "byte",
        "complex64",
        "complex128",
        "error",
        "float32",
        "float64",
        "int",
        "int8",
        "int16",
        "int32",
        "int64",
        "rune",
        "string",
        "uint",
        "uint8",
        "uint16",
        "uint32",
        "uint64",
        "uintptr",
        "any",
        "append",
        "cap",
        "close",
        "copy",
        "delete",
        "len",
        "make",
        "new",
        "panic",
        "print",
        "println",
        "recover",
    ],
    ..c_style(GO_KEYWORDS)
};
const SWIFT: Grammar = Grammar {
    types: &[
        "Int",
        "Double",
        "Float",
        "Bool",
        "String",
        "Character",
        "Array",
        "Dictionary",
        "Set",
        "Optional",
        "Any",
        "print",
    ],
    ..c_style(SWIFT_KEYWORDS)
};
const CSS: Grammar = Grammar {
    keys: true,
    dashed_words: true,
    ..c_style(&[])
};
const JSON: Grammar = Grammar {
    keys: true,
    ..c_style(LITERALS)
};
const VERILOG: Grammar = Grammar {
    types: &["$display", "$finish", "$time", "logic", "bit"],
    ..c_style(VERILOG_KEYWORDS)
};
const PHP: Grammar = Grammar {
    types: &[
        "$this",
        "array",
        "count",
        "isset",
        "empty",
        "strlen",
        "json_encode",
        "json_decode",
        "var_dump",
        "print_r",
    ],
    line_comments: &["//", "#"],
    ..c_style(PHP_KEYWORDS)
};
const KQL: Grammar = Grammar {
    types: &[
        "ago", "now", "datetime", "bin", "dcount", "tostring", "toint", "strcat", "count", "sum",
        "avg",
    ],
    block_comment: None,
    case_insensitive: true,
    ..c_style(KQL_KEYWORDS)
};
const PYTHON: Grammar = Grammar {
    types: &[
        "self",
        "cls",
        "print",
        "len",
        "range",
        "int",
        "str",
        "float",
        "bool",
        "bytes",
        "list",
        "dict",
        "set",
        "tuple",
        "object",
        "open",
        "super",
        "isinstance",
        "enumerate",
        "zip",
        "map",
        "filter",
        "sorted",
        "sum",
        "min",
        "max",
        "abs",
        "type",
        "Exception",
    ],
    ..hash_style(PYTHON_KEYWORDS)
};
const RUBY: Grammar = Grammar {
    types: &[
        "puts",
        "print",
        "p",
        "attr_accessor",
        "attr_reader",
        "attr_writer",
        "lambda",
        "proc",
        "raise",
    ],
    ..hash_style(RUBY_KEYWORDS)
};
const PERL: Grammar = Grammar {
    types: &[
        "chomp", "die", "join", "keys", "push", "shift", "split", "values", "open", "close",
    ],
    ..hash_style(PERL_KEYWORDS)
};
const R: Grammar = Grammar {
    types: &[
        "c", "list", "print", "paste", "length", "vector", "matrix", "sum", "mean", "cat",
    ],
    ..hash_style(R_KEYWORDS)
};
const SHELL: Grammar = Grammar {
    types: &[
        "cd", "ls", "pwd", "grep", "sed", "awk", "cat", "cp", "mv", "rm", "mkdir", "touch",
        "chmod", "sudo", "git", "docker", "kubectl", "curl", "wget", "read", "printf", "source",
        "test", "true", "false",
    ],
    ..hash_style(SHELL_KEYWORDS)
};
const YAML: Grammar = Grammar {
    keys: true,
    ..hash_style(LITERALS)
};
const GRAPHQL: Grammar = Grammar {
    types: &["String", "Int", "Float", "Boolean", "ID"],
    ..hash_style(GRAPHQL_KEYWORDS)
};
const DOCKER: Grammar = Grammar {
    case_insensitive: true,
    ..hash_style(DOCKER_KEYWORDS)
};
const POWERSHELL: Grammar = Grammar {
    dashed_words: true,
    block_comment: Some(("<#", "#>")),
    case_insensitive: true,
    ..hash_style(POWERSHELL_KEYWORDS)
};
const DOS: Grammar = Grammar {
    line_comments: &["::", "rem ", "REM ", "@rem "],
    case_insensitive: true,
    keywords: DOS_KEYWORDS,
    ..CODE
};
const SQL: Grammar = Grammar {
    types: &[
        "int",
        "integer",
        "bigint",
        "varchar",
        "char",
        "text",
        "date",
        "timestamp",
        "boolean",
        "decimal",
        "count",
        "sum",
        "avg",
        "min",
        "max",
        "coalesce",
        "now",
    ],
    line_comments: &["--"],
    block_comment: Some(("/*", "*/")),
    keywords: SQL_KEYWORDS,
    case_insensitive: true,
    ..CODE
};
const VHDL: Grammar = Grammar {
    types: &[
        "std_logic",
        "std_logic_vector",
        "integer",
        "boolean",
        "unsigned",
        "signed",
        "rising_edge",
    ],
    line_comments: &["--"],
    keywords: VHDL_KEYWORDS,
    case_insensitive: true,
    ..CODE
};
const VB: Grammar = Grammar {
    line_comments: &["'", "rem ", "REM "],
    keywords: VB_KEYWORDS,
    case_insensitive: true,
    ..CODE
};
const OCTAVE: Grammar = Grammar {
    types: &[
        "disp", "zeros", "ones", "size", "numel", "length", "plot", "printf", "fprintf", "true",
        "false",
    ],
    line_comments: &["%", "#"],
    block_comment: Some(("%{", "%}")),
    keywords: OCTAVE_KEYWORDS,
    ..CODE
};
const LISP: Grammar = Grammar {
    types: &[
        "car", "cdr", "cons", "list", "format", "print", "append", "mapcar", "funcall", "apply",
    ],
    line_comments: &[";"],
    block_comment: Some(("#|", "|#")),
    keywords: LISP_KEYWORDS,
    ..CODE
};
const HTTP: Grammar = Grammar {
    keys: true,
    dashed_words: true,
    keywords: HTTP_KEYWORDS,
    ..CODE
};
const LATEX: Grammar = Grammar {
    line_comments: &["%"],
    command_prefix: Some('\\'),
    ..CODE
};

enum Rules {
    Code(&'static Grammar),
    Markup,
    Markdown,
}

/// The rules for a code block language: one arm per Teams language id (see
/// `teams_core::CODE_LANGUAGES`); a language without an arm stays plain.
fn rules_for(language: &str) -> Option<Rules> {
    let id = teams_core::code_language(language).map_or_else(
        || language.to_ascii_lowercase(),
        |known| known.id.to_ascii_lowercase(),
    );
    let grammar = match id.as_str() {
        "html" | "xml" | "jsp" => return Some(Rules::Markup),
        "markdown" => return Some(Rules::Markdown),
        "bash" => &SHELL,
        "c" | "objectivec" => &C,
        "cpp" => &CPP,
        "csharp" => &CSHARP,
        "css" | "scss" | "less" => &CSS,
        "dart" => &DART,
        "dockerfile" => &DOCKER,
        "dos" => &DOS,
        "go" => &GO,
        "graphql" => &GRAPHQL,
        "http" => &HTTP,
        "java" => &JAVA,
        "javascript" | "jsx" | "typescript" => &JS,
        "json" => &JSON,
        "kotlin" => &KOTLIN,
        "kql" => &KQL,
        "latex" => &LATEX,
        "lisp" => &LISP,
        "octave" => &OCTAVE,
        "perl" => &PERL,
        "php" => &PHP,
        "powershell" => &POWERSHELL,
        "python" => &PYTHON,
        "r" => &R,
        "ruby" => &RUBY,
        "rust" => &RUST,
        "scala" => &SCALA,
        "sql" | "tsql" | "plsql" => &SQL,
        "swift" => &SWIFT,
        "vbnet" | "vbscript" => &VB,
        "verilog" => &VERILOG,
        "vhdl" => &VHDL,
        "yaml" | "toml" => &YAML,
        _ => return None,
    };
    Some(Rules::Code(grammar))
}

const HIGHLIGHT_CACHE: usize = 64;

thread_local! {
    static HIGHLIGHTS: RefCell<(
        HashMap<(u64, usize, u64), Option<Vec<(Range<usize>, Token)>>>,
        VecDeque<(u64, usize, u64)>,
    )> = RefCell::new((HashMap::new(), VecDeque::new()));
}

fn highlight_key(language: &str, code: &str) -> (u64, usize, u64) {
    let mut language_hash = std::collections::hash_map::DefaultHasher::new();
    language.hash(&mut language_hash);
    let mut code_hash = std::collections::hash_map::DefaultHasher::new();
    code.hash(&mut code_hash);
    (language_hash.finish(), code.len(), code_hash.finish())
}

pub fn highlight(code: &str, language: &str) -> Option<Vec<(Range<usize>, Token)>> {
    let key = highlight_key(language, code);
    if let Some(hit) = HIGHLIGHTS.with(|cache| cache.borrow().0.get(&key).cloned()) {
        return hit;
    }
    let value = highlight_uncached(code, language);
    HIGHLIGHTS.with(|cache| {
        let mut cache = cache.borrow_mut();
        cache.0.insert(key, value.clone());
        cache.1.push_back(key);
        while cache.1.len() > HIGHLIGHT_CACHE {
            if let Some(old) = cache.1.pop_front() {
                cache.0.remove(&old);
            }
        }
    });
    value
}

fn highlight_uncached(code: &str, language: &str) -> Option<Vec<(Range<usize>, Token)>> {
    Some(match rules_for(language)? {
        Rules::Code(grammar) => highlight_code(code, grammar),
        Rules::Markup => highlight_markup(code),
        Rules::Markdown => highlight_markdown(code),
    })
}

/// Keywords whose next word is the name being defined.
const DEFINITIONS: &[&str] = &[
    "class",
    "def",
    "enum",
    "fn",
    "fun",
    "func",
    "function",
    "interface",
    "module",
    "object",
    "protocol",
    "struct",
    "sub",
    "trait",
    "type",
];

/// Only spaces, or a YAML list dash, before `at` on its line.
fn starts_line(code: &str, at: usize) -> bool {
    let line = &code[code[..at].rfind('\n').map_or(0, |end| end + 1)..at];
    matches!(line.trim(), "" | "-")
}

/// Followed by `:` (not `::`) after optional spaces.
fn before_colon(code: &str, end: usize) -> bool {
    let rest = code[end..].trim_start_matches([' ', '\t']);
    rest.starts_with(':') && !rest.starts_with("::")
}

fn highlight_code(code: &str, grammar: &Grammar) -> Vec<(Range<usize>, Token)> {
    let mut tokens = Vec::new();
    let mut index = 0;
    let mut defines = false;
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
        let line_start = index == 0 || code.as_bytes()[index - 1] == b'\n';
        if grammar.line_comments.iter().any(|marker| {
            rest.starts_with(marker)
                // A word comment such as `rem ` only counts at the start of a line.
                && (!marker.ends_with(' ') || line_start || code[..index].ends_with(' '))
        }) {
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
            let key = grammar.keys && before_colon(code, end);
            tokens.push((
                index..end,
                if key { Token::Attribute } else { Token::String },
            ));
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
        let command = grammar.command_prefix == Some(character);
        if character.is_alphabetic() || matches!(character, '_' | '$' | '@' | '#') || command {
            let dashed = grammar.dashed_words;
            let length = character.len_utf8()
                + rest[character.len_utf8()..]
                    .find(|next: char| {
                        !(next.is_alphanumeric() || next == '_' || dashed && next == '-')
                    })
                    .unwrap_or(rest.len() - character.len_utf8());
            let length = rest[..length].trim_end_matches('-').len();
            let word = &rest[..length];
            let same = |known: &&str| {
                if grammar.case_insensitive {
                    known.eq_ignore_ascii_case(word)
                } else {
                    *known == word
                }
            };
            let end = index + length;
            let token = if command && length > 1 || grammar.keywords.iter().any(same) {
                Some(Token::Keyword)
            } else if grammar.keys && starts_line(code, index) && before_colon(code, end) {
                Some(Token::Attribute)
            } else if defines {
                Some(Token::Title)
            } else if grammar.types.iter().any(same) || dashed && word.contains('-') {
                Some(Token::Type)
            } else {
                None
            };
            defines = token == Some(Token::Keyword) && DEFINITIONS.iter().any(same);
            tokens.extend(token.map(|token| (index..end, token)));
            index = end;
            continue;
        }
        if !character.is_whitespace() {
            defines = false;
        }
        index += character.len_utf8();
    }
    tokens
}

/// HTML and XML: tag names, attribute names, quoted values and comments.
fn highlight_markup(code: &str) -> Vec<(Range<usize>, Token)> {
    let mut tokens = Vec::new();
    let mut index = 0;
    while let Some(offset) = code[index..].find('<') {
        let start = index + offset;
        let rest = &code[start..];
        if rest.starts_with("<!--") {
            let end = rest.find("-->").map_or(code.len(), |at| start + at + 3);
            tokens.push((start..end, Token::Comment));
            index = end;
            continue;
        }
        let name_start = start + 1 + usize::from(rest[1..].starts_with(['/', '!', '?']));
        let name_end = name_start
            + code[name_start..]
                .find(|next: char| {
                    !(next.is_alphanumeric() || matches!(next, '-' | ':' | '_' | '.'))
                })
                .unwrap_or(code.len() - name_start);
        if name_end == name_start {
            index = start + 1;
            continue;
        }
        tokens.push((name_start..name_end, Token::Tag));
        let close = code[name_end..]
            .find('>')
            .map_or(code.len(), |at| name_end + at);
        let mut at = name_end;
        while at < close {
            let character = code[at..].chars().next().unwrap_or_default();
            if matches!(character, '"' | '\'') {
                let end = code[at + 1..close]
                    .find(character)
                    .map_or(close, |end| at + 1 + end + 1);
                tokens.push((at..end, Token::String));
                at = end;
            } else if character.is_alphabetic() {
                let end = at
                    + code[at..close]
                        .find(|next: char| {
                            !(next.is_alphanumeric() || matches!(next, '-' | ':' | '_'))
                        })
                        .unwrap_or(close - at);
                tokens.push((at..end, Token::Attribute));
                at = end;
            } else {
                at += character.len_utf8();
            }
        }
        index = (close + 1).min(code.len());
    }
    tokens
}

/// Markdown by line: headings, quotes, list markers, fenced code, then inline code, bold,
/// italic and links.
fn highlight_markdown(code: &str) -> Vec<(Range<usize>, Token)> {
    let mut tokens = Vec::new();
    let mut fenced = false;
    let mut start = 0;
    for line in code.split('\n') {
        let range = start..start + line.len();
        start = range.end + 1;
        let trimmed = line.trim_start();
        let indent = range.start + line.len() - trimmed.len();
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            fenced = !fenced;
            tokens.push((range, Token::String));
            continue;
        }
        if fenced {
            tokens.push((range, Token::String));
            continue;
        }
        let hashes = trimmed.bytes().take_while(|byte| *byte == b'#').count();
        if (1..=6).contains(&hashes) && trimmed[hashes..].starts_with(' ') {
            tokens.push((range, Token::Heading));
            continue;
        }
        if trimmed.starts_with('>') {
            tokens.push((range, Token::Comment));
            continue;
        }
        let marker = list_marker(trimmed);
        if marker > 0 {
            tokens.push((indent..indent + marker, Token::Keyword));
        }
        inline_markdown(code, indent + marker..range.end, &mut tokens);
    }
    tokens
}

/// Byte length of a `- `, `* `, `+ ` or `1. ` list marker.
fn list_marker(line: &str) -> usize {
    if line.starts_with(['-', '*', '+']) && line[1..].starts_with(' ') {
        return 1;
    }
    let digits = line.bytes().take_while(u8::is_ascii_digit).count();
    if digits > 0 && line[digits..].starts_with(". ") {
        digits + 1
    } else {
        0
    }
}

fn inline_markdown(code: &str, range: Range<usize>, tokens: &mut Vec<(Range<usize>, Token)>) {
    let mut index = range.start;
    while index < range.end {
        let rest = &code[index..range.end];
        let span = |open: &str, close: &str| {
            rest.strip_prefix(open)?
                .find(close)
                .filter(|length| *length > 0)
                .map(|length| index + open.len() + length + close.len())
        };
        let found = if rest.starts_with('`') {
            span("`", "`").map(|end| (end, Token::String))
        } else if rest.starts_with("**") || rest.starts_with("__") {
            span(&rest[..2], &rest[..2]).map(|end| (end, Token::Strong))
        } else if rest.starts_with(['*', '_']) {
            span(&rest[..1], &rest[..1]).map(|end| (end, Token::Emphasis))
        } else if rest.starts_with('[') {
            rest.find("](")
                .and_then(|label| {
                    rest[label..]
                        .find(')')
                        .map(|close| index + label + close + 1)
                })
                .map(|end| (end, Token::Link))
        } else {
            None
        };
        match found {
            Some((end, token)) => {
                tokens.push((index..end, token));
                index = end;
            }
            None => index += rest.chars().next().map_or(1, char::len_utf8),
        }
    }
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
                ("a", Token::Title),
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
            vec![
                ("fn", Token::Keyword),
                ("a", Token::Title),
                ("str", Token::Type),
                ("char", Token::Type),
                ("'c'", Token::String),
            ]
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
        assert!(highlight("x", "plaintext").is_none());
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

    #[test]
    fn every_teams_language_but_plain_text_has_rules() {
        let missing: Vec<_> = teams_core::CODE_LANGUAGES
            .iter()
            .filter(|language| language.id != teams_core::PLAIN_LANGUAGE)
            .filter(|language| rules_for(language.id).is_none())
            .map(|language| language.id)
            .collect();
        assert!(missing.is_empty(), "no highlighting for {missing:?}");
    }

    #[test]
    fn markup_colours_tags_attributes_and_values() {
        assert_eq!(
            kinds("<a href=\"x\">t</a><!-- c -->", "html"),
            vec![
                ("a", Token::Tag),
                ("href", Token::Attribute),
                ("\"x\"", Token::String),
                ("a", Token::Tag),
                ("<!-- c -->", Token::Comment),
            ]
        );
    }

    #[test]
    fn markdown_colours_its_blocks_and_inline_marks() {
        assert_eq!(
            kinds(
                "# Title\n- **b** and *i* `c` [l](u)\n> q\n```\nx\n```",
                "md"
            ),
            vec![
                ("# Title", Token::Heading),
                ("-", Token::Keyword),
                ("**b**", Token::Strong),
                ("*i*", Token::Emphasis),
                ("`c`", Token::String),
                ("[l](u)", Token::Link),
                ("> q", Token::Comment),
                ("```", Token::String),
                ("x", Token::String),
                ("```", Token::String),
            ]
        );
    }

    #[test]
    fn definitions_types_and_keys() {
        assert_eq!(
            kinds("def f(self): print(1)", "python"),
            vec![
                ("def", Token::Keyword),
                ("f", Token::Title),
                ("self", Token::Type),
                ("print", Token::Type),
                ("1", Token::Number),
            ]
        );
        assert_eq!(
            kinds("func ok() bool", "go"),
            vec![
                ("func", Token::Keyword),
                ("ok", Token::Title),
                ("bool", Token::Type)
            ]
        );
        assert_eq!(
            kinds("{\"a\": \"b\"}", "json"),
            vec![("\"a\"", Token::Attribute), ("\"b\"", Token::String)]
        );
        assert_eq!(
            kinds("name: x\nurl: http://y", "yaml"),
            vec![("name", Token::Attribute), ("url", Token::Attribute)]
        );
        assert_eq!(
            kinds("  font-size: 2px;", "css")[0],
            ("font-size", Token::Attribute)
        );
        assert_eq!(
            kinds("Get-ChildItem -Path .", "powershell")[0],
            ("Get-ChildItem", Token::Type)
        );
    }

    #[test]
    fn word_comments_and_latex_commands() {
        assert_eq!(
            kinds("rem x\necho", "dos"),
            vec![("rem x", Token::Comment), ("echo", Token::Keyword)]
        );
        assert_eq!(
            kinds("\\section{a} % c", "latex"),
            vec![("\\section", Token::Keyword), ("% c", Token::Comment)]
        );
        assert_eq!(kinds("FROM x", "dockerFile")[0], ("FROM", Token::Keyword));
    }
}
