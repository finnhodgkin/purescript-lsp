use crate::ide_server::RebuildError;
use lsp_types::{Diagnostic, DiagnosticSeverity, Position, Range, Url};

/// Convert a rebuild error to an LSP diagnostic
pub fn rebuild_error_to_diagnostic(error: &RebuildError, _uri: &Url) -> Option<Diagnostic> {
    let position = &error.position;

    let range = Range {
        start: Position {
            line: position.start_line.saturating_sub(1),
            character: position.start_column.saturating_sub(1),
        },
        end: Position {
            line: position.end_line.saturating_sub(1),
            character: position.end_column.saturating_sub(1),
        },
    };

    let severity = match error.error_code.as_str() {
        // Type declaration warnings - these should be warnings, not errors
        "MissingTypeDeclaration"
        | "ImplicitImport"
        | "DeprecatedQualifiedSyntax"
        | "RedundantUnqualifiedImport"
        | "RedundantEmptyHidingImport"
        | "DuplicateImport"
        | "UnusedImport"
        | "UnusedExplicitImport"
        | "ShadowedName"
        | "UnusedTypeVar"
        | "Deprecated" => DiagnosticSeverity::WARNING,

        // Default to error for everything else
        _ => DiagnosticSeverity::ERROR,
    };

    Some(Diagnostic {
        range,
        severity: Some(severity),
        code: Some(lsp_types::NumberOrString::String(error.error_code.clone())),
        source: Some("purescript".to_string()),
        message: error.message.clone(),
        related_information: None,
        tags: None,
        code_description: None,
        data: None,
    })
}

/// Convert rebuild errors to LSP diagnostics
pub fn convert_rebuild_errors(errors: &[RebuildError], uri: &Url) -> Vec<Diagnostic> {
    errors
        .iter()
        .filter_map(|error| rebuild_error_to_diagnostic(error, uri))
        .collect()
}
