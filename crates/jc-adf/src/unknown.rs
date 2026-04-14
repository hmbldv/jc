//! Escape hatch for ADF nodes with no clean markdown representation.
//!
//! Unknown / exotic nodes (panel, status, expand, layout, decisionList, etc.)
//! are rendered as fenced code blocks with a type marker:
//!
//! ```adf:panel:info
//! { ...raw ADF JSON... }
//! ```
//!
//! On the return trip, the converter detects the marker and re-emits the raw
//! ADF verbatim. Nothing is ever silently dropped.
