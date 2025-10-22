use crate::ide_server::RebuildError;
use lsp_types::{
    CodeAction, CodeActionKind, CodeActionParams, Position, Range, TextEdit, WorkspaceEdit,
};
use std::collections::HashMap;

/// Check if two ranges overlap
fn ranges_overlap(range1: &Range, range2: &Range) -> bool {
    range1.start.line <= range2.end.line && range2.start.line <= range1.end.line
}

/// Get a concise title for a code action based on the error code
fn get_code_action_title(error_code: &str) -> &str {
    match error_code {
        "UnusedImport" => "Remove import",
        "RedundantEmptyHidingImport" => "Remove import",
        "DuplicateImport" => "Remove import",
        "RedundantUnqualifiedImport" => "Remove import",
        "DeprecatedQualifiedSyntax" => "Remove qualified keyword",
        "ImplicitImport" => "Make import explicit",
        "UnusedExplicitImport" => "Remove unused references",
        _ => "Apply suggestion",
    }
}

/// Check if an error has a fixable suggestion
pub fn has_fixable_suggestion(error: &RebuildError) -> bool {
    // Trust the IDE server - if it provides a suggestion, we can fix it
    error.suggestion.is_some()
}

/// Convert a rebuild error with suggestion to a code action
pub fn error_to_code_action(error: &RebuildError, uri: &lsp_types::Url) -> Option<CodeAction> {
    let suggestion = error.suggestion.as_ref()?;
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

    // Use the suggestion's replace_range if available, otherwise use the error range
    let replacement_range = if let Some(pos) = &suggestion.replace_range {
        Range {
            start: Position {
                line: pos.start_line.saturating_sub(1),
                character: pos.start_column.saturating_sub(1),
            },
            end: Position {
                line: pos.end_line.saturating_sub(1),
                character: pos.end_column.saturating_sub(1),
            },
        }
    } else {
        // If no specific replace range, use the error range
        range
    };

    // Special handling for type annotations
    let (final_range, final_text) = if suggestion.replacement.contains("::")
        && suggestion.replacement.trim().starts_with("main")
    {
        // This looks like a type annotation suggestion
        // Insert it at the beginning of the line, not replace the function
        let line_start = Position {
            line: range.start.line,
            character: 0,
        };
        let line_end = Position {
            line: range.start.line,
            character: 0,
        };
        let insert_range = Range {
            start: line_start,
            end: line_end,
        };
        let text_with_newline = format!("{}\n", suggestion.replacement.trim());
        (insert_range, text_with_newline)
    } else {
        (
            replacement_range,
            suggestion.replacement.trim_end().to_string(),
        )
    };

    let text_edit = TextEdit {
        range: final_range,
        new_text: final_text,
    };

    let workspace_edit = WorkspaceEdit {
        changes: Some(std::collections::HashMap::from([(
            uri.clone(),
            vec![text_edit],
        )])),
        document_changes: None,
        change_annotations: None,
    };

    Some(CodeAction {
        title: get_code_action_title(&error.error_code).to_string(),
        kind: Some(CodeActionKind::QUICKFIX),
        diagnostics: None,
        is_preferred: Some(true),
        disabled: None,
        edit: Some(workspace_edit),
        command: None,
        data: None,
    })
}

/// Generate code actions for a document
pub fn generate_code_actions(
    params: &CodeActionParams,
    errors: &[RebuildError],
) -> Vec<CodeAction> {
    let fixable_errors: Vec<_> = errors
        .iter()
        .filter(|error| has_fixable_suggestion(error))
        .collect();

    // Filter errors to only include those whose position overlaps with the requested range
    let overlapping_errors: Vec<_> = fixable_errors
        .iter()
        .filter(|error| {
            let error_range = Range {
                start: Position {
                    line: error.position.start_line.saturating_sub(1),
                    character: error.position.start_column.saturating_sub(1),
                },
                end: Position {
                    line: error.position.end_line.saturating_sub(1),
                    character: error.position.end_column.saturating_sub(1),
                },
            };
            ranges_overlap(&error_range, &params.range)
        })
        .collect();

    overlapping_errors
        .iter()
        .filter_map(|error| error_to_code_action(error, &params.text_document.uri))
        .collect()
}

/// Create an "Apply all fixes" code action that safely applies multiple fixes
pub fn create_apply_all_action(
    params: &CodeActionParams,
    errors: &[RebuildError],
) -> Option<CodeAction> {
    let fixable_errors: Vec<_> = errors
        .iter()
        .filter(|error| has_fixable_suggestion(error))
        .collect();

    if fixable_errors.len() <= 1 {
        return None;
    }

    // Sort errors by position (end to start) to avoid range conflicts
    let mut sorted_errors: Vec<_> = fixable_errors.iter().collect();
    sorted_errors.sort_by(|a, b| {
        b.position
            .end_line
            .cmp(&a.position.end_line)
            .then(b.position.end_column.cmp(&a.position.end_column))
    });

    // Remove overlapping fixes by keeping only the first (highest priority) fix in each range
    let mut non_overlapping_errors = Vec::new();
    for error in sorted_errors {
        let error_range = Range {
            start: Position {
                line: error.position.start_line.saturating_sub(1),
                character: error.position.start_column.saturating_sub(1),
            },
            end: Position {
                line: error.position.end_line.saturating_sub(1),
                character: error.position.end_column.saturating_sub(1),
            },
        };

        // Check if this error overlaps with any already selected error
        let has_overlap = non_overlapping_errors
            .iter()
            .any(|existing_error: &&&RebuildError| {
                let existing_range = Range {
                    start: Position {
                        line: existing_error.position.start_line.saturating_sub(1),
                        character: existing_error.position.start_column.saturating_sub(1),
                    },
                    end: Position {
                        line: existing_error.position.end_line.saturating_sub(1),
                        character: existing_error.position.end_column.saturating_sub(1),
                    },
                };
                ranges_overlap(&error_range, &existing_range)
            });

        if !has_overlap {
            non_overlapping_errors.push(error);
        }
    }

    if non_overlapping_errors.is_empty() {
        return None;
    }

    let fix_count = non_overlapping_errors.len();
    // Create text edits for all non-overlapping fixes
    let mut text_edits = Vec::new();
    for error in non_overlapping_errors {
        if let Some(suggestion) = &error.suggestion {
            let replacement_range = suggestion
                .replace_range
                .as_ref()
                .map(|pos| Range {
                    start: Position {
                        line: pos.start_line.saturating_sub(1),
                        character: pos.start_column.saturating_sub(1),
                    },
                    end: Position {
                        line: pos.end_line.saturating_sub(1),
                        character: pos.end_column.saturating_sub(1),
                    },
                })
                .unwrap_or_else(|| Range {
                    start: Position {
                        line: error.position.start_line.saturating_sub(1),
                        character: error.position.start_column.saturating_sub(1),
                    },
                    end: Position {
                        line: error.position.end_line.saturating_sub(1),
                        character: error.position.end_column.saturating_sub(1),
                    },
                });

            text_edits.push(TextEdit {
                range: replacement_range,
                new_text: suggestion.replacement.trim_end().to_string(),
            });
        }
    }

    if text_edits.is_empty() {
        return None;
    }

    let workspace_edit = WorkspaceEdit {
        changes: Some(HashMap::from([(
            params.text_document.uri.clone(),
            text_edits,
        )])),
        document_changes: None,
        change_annotations: None,
    };

    Some(CodeAction {
        title: format!("Apply all fixes ({} fixes)", fix_count),
        kind: Some(CodeActionKind::QUICKFIX),
        diagnostics: None,
        is_preferred: Some(false),
        disabled: None,
        edit: Some(workspace_edit),
        command: None,
        data: None,
    })
}
