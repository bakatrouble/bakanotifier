//! Rich template loading, rendering and Telegram delivery helpers.

mod error;
mod loader;
mod model;

pub use self::error::TemplateError;
pub use self::loader::{is_valid_template_name, load_and_render, load_raw, parse_template};
pub use self::model::{RenderedMessage, RichTemplate, TemplateMode};

#[cfg(test)]
mod tests;
