use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DiagnosticReport {
    pub protocol_version: String,
    pub status: DiagnosticStatus,
    pub timestamp: String,
    pub summary: DiagnosticSummary,
    pub diagnostics: Vec<Diagnostic>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum DiagnosticStatus {
    #[serde(rename = "ok")]
    Success,
    Warning,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct DiagnosticSummary {
    pub total_errors: usize,
    pub total_warnings: usize,
    pub auto_fixable: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Diagnostic {
    pub id: String,
    pub code: String,
    pub category: String,
    pub severity: DiagnosticSeverity,
    pub message: String,
    pub primary_location: Location,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub suggested_patch: Option<SuggestedPatch>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum DiagnosticSeverity {
    Error,
    Warning,
    Info,
    Hint,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Location {
    pub file: String,
    pub span: Span,
    pub byte_range: ByteRange,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct Span {
    pub start_line: usize,
    pub start_col: usize,
    pub end_line: usize,
    pub end_col: usize,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct ByteRange {
    pub start: usize,
    pub end: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SuggestedPatch {
    pub patch_type: PatchType,
    pub confidence: f32,
    pub explanation: String,
    pub replacement: Replacement,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub patch_unified: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum PatchType {
    AutoApply,
    UserConfirm,
    AlternativeChoice,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Replacement {
    pub file: String,
    pub byte_range: ByteRange,
    pub new_text: String,
}
