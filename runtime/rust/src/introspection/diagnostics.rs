use super::facts;
use super::model::{IntrospectionDiagnostic, IntrospectionStatus};

pub(super) fn derive_diagnostics(status: &IntrospectionStatus) -> Vec<IntrospectionDiagnostic> {
    facts::derive_diagnostic_facts(status)
}
