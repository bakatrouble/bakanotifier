#[derive(Debug)]
pub enum TemplateError {
    NotFound,
    Io(String),
    Render(String),
    Yaml(String),
    Invalid(String),
    Values(String),
}

impl std::fmt::Display for TemplateError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotFound => write!(f, "template not found"),
            Self::Io(message) => write!(f, "template read error: {message}"),
            Self::Render(message) => write!(f, "template render error: {message}"),
            Self::Yaml(message) => write!(f, "template is not valid YAML: {message}"),
            Self::Invalid(message) => write!(f, "invalid rich template: {message}"),
            Self::Values(message) => write!(f, "values validation failed: {message}"),
        }
    }
}

impl std::error::Error for TemplateError {}
