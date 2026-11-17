//! The TB resource text format: a tree of named nodes with values.
//!
//! TB: parser/tb_parser.cpp, tb_node_tree.cpp, tb_node_ref_tree.cpp.
//!
//! ```text
//! # A comment
//! window                              <- a node with no value
//! 	title My window                   <- "title" = "My window" (the rest of the line)
//! 	size 300 200                      <- number lists become arrays
//! 	TBButton: text: "Ok", id: ok      <- compact form: children separated by commas
//! 	TBLayout: axis: y                 <- the first value after "Name:" is optional
//! 		text: "Line 1\n" \              <- a trailing backslash continues the value
//! 		      "Line 2"
//! 	@include defines>button           <- copy the children of a node read earlier
//! 	@file other.tb.txt                <- add the nodes of another file
//! 	@if @settings>landscape           <- conditional branches, resolved after parsing
//! 		axis x
//! 	@else
//! 		axis y
//! 	skin @>defines>skin               <- a reference, followed when the value is read
//! ```
//!
//! Indentation is by tabs. Values are typed with [`Value::parse_auto`](crate::Value::parse_auto)
//! (int, float, number array, otherwise string); quoted values are always strings.

// The example above is indented with tabs because the format is.
#![allow(clippy::tabs_in_doc_comments)]

mod node;
mod parser;
mod refs;

pub use node::{Lookup, ResNode};
pub use parser::{
    MAX_DEPTH, MAX_INCLUDE_DEPTH, MAX_INCLUDES, MAX_NODES, ParseCtx, ParseError, parse, parse_file, unescape,
};
pub use refs::{MAX_REF_HOPS, RefTrees};
