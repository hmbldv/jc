//! Small JQL builder used by the wrapper commands (`issue list`, `issue mine`,
//! `issue search`). Users can always drop down to `jc jira jql` for the raw
//! escape hatch; this module exists so the friendly wrappers produce correct,
//! safely-escaped queries.

/// Escape a user-supplied string for use as a JQL literal. Re-exported from
/// [`jc_core::literal::escape_string`] so JQL and CQL share the same
/// implementation (they have identical literal grammars).
pub use jc_core::literal::escape_string;

#[derive(Debug, Default)]
pub struct JqlBuilder {
    clauses: Vec<String>,
    order_by: Option<String>,
}

impl JqlBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    /// `field = "value"` — value is escape-quoted.
    pub fn eq(mut self, field: &str, value: &str) -> Self {
        self.clauses.push(format!("{field} = {}", escape_string(value)));
        self
    }

    /// `field ~ "value"` — full-text match, value is escape-quoted.
    pub fn contains(mut self, field: &str, value: &str) -> Self {
        self.clauses.push(format!("{field} ~ {}", escape_string(value)));
        self
    }

    /// Raw JQL fragment inserted verbatim. Use for function calls like
    /// `assignee = currentUser()` or time expressions like `updated >= -7d`
    /// where quoting would be wrong.
    pub fn raw(mut self, clause: impl Into<String>) -> Self {
        self.clauses.push(clause.into());
        self
    }

    pub fn order_by(mut self, s: impl Into<String>) -> Self {
        self.order_by = Some(s.into());
        self
    }

    pub fn build(self) -> String {
        let mut out = self.clauses.join(" AND ");
        if let Some(order) = self.order_by {
            if !out.is_empty() {
                out.push(' ');
            }
            out.push_str("ORDER BY ");
            out.push_str(&order);
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_empty() {
        assert_eq!(JqlBuilder::new().build(), "");
    }

    #[test]
    fn build_single_eq() {
        let q = JqlBuilder::new().eq("project", "FOO").build();
        assert_eq!(q, r#"project = "FOO""#);
    }

    #[test]
    fn build_multiple_clauses_and() {
        let q = JqlBuilder::new()
            .eq("project", "FOO")
            .eq("status", "In Progress")
            .build();
        assert_eq!(q, r#"project = "FOO" AND status = "In Progress""#);
    }

    #[test]
    fn build_with_raw_and_order() {
        let q = JqlBuilder::new()
            .raw("assignee = currentUser()")
            .raw("updated >= -7d")
            .order_by("updated DESC")
            .build();
        assert_eq!(
            q,
            "assignee = currentUser() AND updated >= -7d ORDER BY updated DESC"
        );
    }

    #[test]
    fn build_contains() {
        let q = JqlBuilder::new()
            .contains("summary", "webhook retry")
            .build();
        assert_eq!(q, r#"summary ~ "webhook retry""#);
    }

    #[test]
    fn build_order_only() {
        let q = JqlBuilder::new().order_by("updated DESC").build();
        assert_eq!(q, "ORDER BY updated DESC");
    }
}
