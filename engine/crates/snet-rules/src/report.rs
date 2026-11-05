use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rule {
    FileLength,
    ForbiddenWord,
    Manifest,
    ModulePath,
    CommitObject,
}

impl fmt::Display for Rule {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self {
            Rule::FileLength => "file-length",
            Rule::ForbiddenWord => "forbidden-word",
            Rule::Manifest => "manifest",
            Rule::ModulePath => "module-path",
            Rule::CommitObject => "commit-object",
        };
        formatter.write_str(name)
    }
}

/// `path` is already masked by `ForbiddenTerms::display_path`, so printing a
/// finding never prints a listed word.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    pub path: String,
    pub line: usize,
    pub rule: Rule,
    pub detail: String,
}

impl fmt::Display for Finding {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{}:{}: {}: {}",
            self.path, self.line, self.rule, self.detail
        )
    }
}
