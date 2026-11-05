//! Limited, safe markdown. [`parse`] is total: any input yields a bounded [`Document`].

mod ast;
mod blocks;
mod inline;
mod url;

pub use ast::{Block, CodeBlock, Document, Inline};
pub use blocks::parse;
pub use url::{HostInfo, HostWarning, Link, parse_http};
