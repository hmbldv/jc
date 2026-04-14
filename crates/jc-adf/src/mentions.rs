//! User mention resolution. The converter needs an async lookup to turn
//! `@username` into an ADF mention node keyed by accountId. The resolver is
//! a trait so jc-jira and jc-conf can share one implementation.
