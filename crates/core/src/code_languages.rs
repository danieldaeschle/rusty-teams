/// A code block language as Teams offers it: the id its `language-*` class carries and the name shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CodeLanguage {
    pub id: &'static str,
    pub name: &'static str,
}

pub const PLAIN_LANGUAGE: &str = "plaintext";

const fn language(id: &'static str, name: &'static str) -> CodeLanguage {
    CodeLanguage { id, name }
}

/// Teams' code block picker, in its order, from the class map of its code block editor
/// (Teams web). Its Shell row sends `language-bash` like Bash, so it is left out here.
/// A new language is one row here, and a grammar in the app's highlighter if it gets colours.
pub const CODE_LANGUAGES: &[CodeLanguage] = &[
    language(PLAIN_LANGUAGE, "Plain Text"),
    language("bash", "Bash"),
    language("c", "C"),
    language("cpp", "C++"),
    language("csharp", "C#"),
    language("css", "CSS"),
    language("dart", "Dart"),
    language("dockerFile", "DockerFile"),
    language("dos", "DOS"),
    language("go", "Go"),
    language("graphql", "GraphQL"),
    language("html", "HTML"),
    language("http", "HTTP"),
    language("java", "Java"),
    language("javascript", "JavaScript"),
    language("json", "JSON"),
    language("jsp", "JSP"),
    language("jsx", "JSX"),
    language("kotlin", "Kotlin"),
    language("kql", "KQL"),
    language("latex", "LaTeX"),
    language("lisp", "Lisp"),
    language("markdown", "Markdown"),
    language("objectivec", "Objective-C"),
    language("octave", "Octave"),
    language("perl", "Perl"),
    language("php", "PHP"),
    language("powershell", "PowerShell"),
    language("python", "Python"),
    language("r", "R"),
    language("ruby", "Ruby"),
    language("rust", "Rust"),
    language("scala", "Scala"),
    language("scss", "SCSS"),
    language("sql", "SQL"),
    language("swift", "Swift"),
    language("typescript", "TypeScript"),
    language("vbnet", "VB.NET"),
    language("vbscript", "VBScript"),
    language("verilog", "Verilog"),
    language("vhdl", "VHDL"),
    language("xml", "XML"),
    language("yaml", "YAML"),
];

/// Short names people type after ``` that mean one of Teams' languages.
const ALIASES: &[(&str, &str)] = &[
    ("text", PLAIN_LANGUAGE),
    ("txt", PLAIN_LANGUAGE),
    ("sh", "bash"),
    ("shell", "bash"),
    ("zsh", "bash"),
    ("c++", "cpp"),
    ("cs", "csharp"),
    ("c#", "csharp"),
    ("docker", "dockerFile"),
    ("bat", "dos"),
    ("cmd", "dos"),
    ("golang", "go"),
    ("js", "javascript"),
    ("kt", "kotlin"),
    ("tex", "latex"),
    ("md", "markdown"),
    ("objc", "objectivec"),
    ("matlab", "octave"),
    ("ps1", "powershell"),
    ("py", "python"),
    ("rb", "ruby"),
    ("rs", "rust"),
    ("ts", "typescript"),
    ("tsx", "typescript"),
    ("vb", "vbnet"),
    ("svg", "xml"),
    ("yml", "yaml"),
];

/// Teams' language for an id or a common short name, in any case.
pub fn code_language(id: &str) -> Option<&'static CodeLanguage> {
    let id = ALIASES
        .iter()
        .find(|(alias, _)| alias.eq_ignore_ascii_case(id))
        .map_or(id, |(_, target)| target);
    CODE_LANGUAGES
        .iter()
        .find(|language| language.id.eq_ignore_ascii_case(id))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_names_and_short_names_find_teams_languages() {
        assert_eq!(
            code_language("dockerfile").map(|l| l.id),
            Some("dockerFile")
        );
        assert_eq!(code_language("py").map(|l| l.name), Some("Python"));
        assert_eq!(code_language("SHELL").map(|l| l.name), Some("Bash"));
        assert_eq!(code_language("brainfuck"), None);
        assert!(ALIASES.iter().all(|(_, id)| code_language(id).is_some()));
    }
}
