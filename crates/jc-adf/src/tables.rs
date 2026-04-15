//! GFM tables are now handled inline inside [`crate::to_adf`] and
//! [`crate::from_adf`] rather than in a dedicated module. This file is
//! retained as a placeholder so the module path stays stable for the
//! 0.1 release; a future refactor can move the table-specific helpers
//! (`render_table`, `render_table_row`, `render_cell_inline`,
//! `escape_table_cell`) here if the logic grows.
//!
//! ## Fidelity notes
//!
//! - GFM → ADF: header row cells become `tableHeader`, body cells become
//!   `tableCell`. Inline marks (bold, italic, code, strike, link) inside
//!   cells are preserved. Column alignment from the GFM separator row is
//!   discarded because ADF doesn't model per-column alignment.
//! - ADF → GFM: rows whose cells are all `tableHeader` become the GFM
//!   header row; if no header row is present, a blank header is
//!   synthesized so the output is still valid GFM. Pipes inside cell
//!   content are escaped as `\|`; newlines within a cell are collapsed
//!   to spaces.
