use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DiagnosticReport {
    pub protocol_version: String,
    pub timestamp: String,
    pub status: DiagnosticStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<DiagnosticSummary>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub compilation_metrics: Option<CompilationMetrics>,
    pub diagnostics: Vec<Diagnostic>,
}

impl DiagnosticReport {
    pub fn new(status: DiagnosticStatus, timestamp: String, diagnostics: Vec<Diagnostic>) -> Self {
        let total_errors = diagnostics
            .iter()
            .filter(|d| d.severity == DiagnosticSeverity::Error)
            .count();
        let total_warnings = diagnostics
            .iter()
            .filter(|d| d.severity == DiagnosticSeverity::Warning)
            .count();
        let auto_fixable = diagnostics
            .iter()
            .filter(|d| d.suggested_patch.is_some())
            .count();

        Self {
            protocol_version: "1.0.0".to_string(),
            timestamp,
            status,
            summary: Some(DiagnosticSummary {
                total_errors,
                total_warnings,
                auto_fixable,
            }),
            compilation_metrics: None,
            diagnostics,
        }
    }

    pub fn with_metrics(mut self, metrics: CompilationMetrics) -> Self {
        self.compilation_metrics = Some(metrics);
        self
    }
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
pub struct CompilationMetrics {
    pub duration_ms: f64,
    pub lines_parsed: usize,
    pub tokens_evaluated: usize,
    pub target: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum DiagnosticCategory {
    SyntaxError,
    TypeMismatch,
    UndefinedIdentifier,
    UncheckedNullability,
    SecurityViolation,
    DatabaseConstraint,
    RoutingConflict,
    UnusedDeclaration,
    RuntimePanic,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Diagnostic {
    pub id: String,
    pub code: String,
    pub category: DiagnosticCategory,
    pub severity: DiagnosticSeverity,
    pub message: String,
    pub primary_location: Location,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub related_locations: Vec<RelatedLocation>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub type_info: Option<TypeInfo>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context_slice: Option<ContextSlice>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub suggested_patch: Option<SuggestedPatch>,
}

impl Diagnostic {
    pub fn new(
        id: String,
        code: String,
        category: DiagnosticCategory,
        severity: DiagnosticSeverity,
        message: String,
        primary_location: Location,
    ) -> Self {
        Self {
            id,
            code,
            category,
            severity,
            message,
            primary_location,
            related_locations: Vec::new(),
            type_info: None,
            context_slice: None,
            suggested_patch: None,
        }
    }

    pub fn with_patch(mut self, patch: SuggestedPatch) -> Self {
        self.suggested_patch = Some(patch);
        self
    }

    pub fn with_type_info(mut self, info: TypeInfo) -> Self {
        self.type_info = Some(info);
        self
    }

    pub fn with_context_slice(mut self, slice: ContextSlice) -> Self {
        self.context_slice = Some(slice);
        self
    }

    pub fn with_related_location(mut self, location: Location, message: impl Into<String>) -> Self {
        self.related_locations.push(RelatedLocation {
            location,
            message: message.into(),
        });
        self
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RelatedLocation {
    pub location: Location,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TypeInfo {
    pub expected: String,
    pub actual: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub coercible: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ContextSlice {
    pub focus_line: usize,
    pub lines: Vec<ContextLine>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ContextLine {
    pub line_number: usize,
    pub content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_focus: Option<bool>,
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
    pub confidence: f64,
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

// ----------------------------------------------------------------------------
// Deterministic Diagnostic Helpers & Patch Utilities
// ----------------------------------------------------------------------------

pub fn byte_to_line_col(source: &str, byte_offset: usize) -> (usize, usize) {
    let mut line = 1;
    let mut col = 1;
    for (idx, ch) in source.char_indices() {
        if idx >= byte_offset {
            break;
        }
        if ch == '\n' {
            line += 1;
            col = 1;
        } else {
            col += 1;
        }
    }
    (line, col)
}

pub fn generate_diagnostic_id(file: &str, byte_range: &ByteRange, code: &str) -> String {
    let normalized_file = file.replace('\\', "/");
    let mut hash = 0xcbf29ce484222325u64;
    for &b in normalized_file.as_bytes() {
        hash = (hash ^ (b as u64)).wrapping_mul(0x100000001b3u64);
    }
    for &b in code.as_bytes() {
        hash = (hash ^ (b as u64)).wrapping_mul(0x100000001b3u64);
    }
    for b in byte_range.start.to_le_bytes() {
        hash = (hash ^ (b as u64)).wrapping_mul(0x100000001b3u64);
    }
    for b in byte_range.end.to_le_bytes() {
        hash = (hash ^ (b as u64)).wrapping_mul(0x100000001b3u64);
    }
    format!("diag_{:012x}", hash & 0xffffffffffffu64)
}

pub fn compute_context_slice(
    source: &str,
    focus_line: usize,
    context_window: usize,
) -> ContextSlice {
    let source_lines: Vec<&str> = source.lines().collect();
    let total_lines = source_lines.len();

    if total_lines == 0 {
        return ContextSlice {
            focus_line: 1,
            lines: Vec::new(),
        };
    }

    let clamped_focus = focus_line.clamp(1, total_lines);
    let start_line = clamped_focus.saturating_sub(context_window).max(1);
    let end_line = (clamped_focus + context_window).min(total_lines);

    let mut lines = Vec::new();
    for l_num in start_line..=end_line {
        let content = source_lines[l_num - 1].to_string();
        lines.push(ContextLine {
            line_number: l_num,
            content,
            is_focus: Some(l_num == clamped_focus),
        });
    }

    ContextSlice {
        focus_line: clamped_focus,
        lines,
    }
}

pub fn generate_unified_diff(
    file: &str,
    original_source: &str,
    byte_range: &ByteRange,
    new_text: &str,
) -> String {
    let start = byte_range.start.min(original_source.len());
    let end = byte_range.end.min(original_source.len()).max(start);

    // Find the start line and start-of-line byte index
    let before = &original_source[..start];
    let start_line = before.chars().filter(|&c| c == '\n').count() + 1;
    let line_start_idx = before.rfind('\n').map(|idx| idx + 1).unwrap_or(0);

    // Find the end line and end-of-line byte index
    let after = &original_source[end..];
    let line_end_idx = match after.find('\n') {
        Some(idx) => end + idx,
        None => original_source.len(),
    };

    let old_block = &original_source[line_start_idx..line_end_idx];
    let mut new_block = String::new();
    new_block.push_str(&original_source[line_start_idx..start]);
    new_block.push_str(new_text);
    new_block.push_str(&original_source[end..line_end_idx]);

    let old_lines: Vec<&str> = old_block.split('\n').collect();
    let new_lines: Vec<&str> = new_block.split('\n').collect();

    let old_count = old_lines.len();
    let new_count = new_lines.len();

    let mut diff = format!("--- {}\n+++ {}\n", file, file);
    diff.push_str(&format!(
        "@@ -{},{} +{},{} @@\n",
        start_line, old_count, start_line, new_count
    ));

    for line in old_lines {
        diff.push('-');
        diff.push_str(line.trim_end_matches('\r'));
        diff.push('\n');
    }
    for line in new_lines {
        diff.push('+');
        diff.push_str(line.trim_end_matches('\r'));
        diff.push('\n');
    }

    diff.trim_end().to_string()
}

pub fn apply_replacement(source: &str, replacement: &Replacement) -> Result<String, String> {
    let start = replacement.byte_range.start;
    let end = replacement.byte_range.end;
    if start > end || end > source.len() {
        return Err(format!(
            "Invalid byte range [{}..{}] for source of length {}",
            start,
            end,
            source.len()
        ));
    }
    if !source.is_char_boundary(start) || !source.is_char_boundary(end) {
        return Err(format!(
            "Byte range [{}..{}] does not align with UTF-8 character boundaries",
            start, end
        ));
    }
    let mut new_source =
        String::with_capacity(source.len() + replacement.new_text.len() - (end - start));
    new_source.push_str(&source[..start]);
    new_source.push_str(&replacement.new_text);
    new_source.push_str(&source[end..]);
    Ok(new_source)
}

pub fn apply_replacements_batch(
    source: &str,
    replacements: &[Replacement],
) -> Result<String, String> {
    if replacements.is_empty() {
        return Ok(source.to_string());
    }

    let mut sorted = replacements.to_vec();
    // Sort descending by start offset, then descending by end offset for deterministic collision detection
    sorted.sort_by_key(|b| {
        (
            std::cmp::Reverse(b.byte_range.start),
            std::cmp::Reverse(b.byte_range.end),
        )
    });

    // Check for overlapping byte ranges or identical start points
    for i in 0..sorted.len().saturating_sub(1) {
        if sorted[i + 1].byte_range.end > sorted[i].byte_range.start
            || sorted[i + 1].byte_range.start == sorted[i].byte_range.start
        {
            return Err(format!(
                "Overlapping replacements detected: [{}..{}] and [{}..{}]",
                sorted[i + 1].byte_range.start,
                sorted[i + 1].byte_range.end,
                sorted[i].byte_range.start,
                sorted[i].byte_range.end
            ));
        }
    }

    let mut current = source.to_string();
    for rep in &sorted {
        current = apply_replacement(&current, rep)?;
    }
    Ok(current)
}
