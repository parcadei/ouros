//! Multi-session MCP server support for Ouros REPL sessions.
//!
//! This crate exposes a handler layer (`handler::McpHandler`) that manages
//! multiple named `ReplSession` instances and maps MCP tool calls to them.
//! A "default" session is always present and used when no `session_id` is
//! specified, maintaining backward compatibility with the single-session API.
//!
//! Session management tools allow creating, destroying, listing, and forking
//! sessions. Heap introspection tools provide statistics, snapshots, and diffs
//! for monitoring memory behavior across sessions.

pub mod handler;
