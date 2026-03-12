//! HancomEQN → LaTeX converter.
//!
//! Converts HancomEQN equation scripts (used in HWP/HWPX documents) into
//! LaTeX math expressions suitable for markdown rendering.

mod lexer;
mod parser;

pub(crate) use parser::eqn_to_latex;
