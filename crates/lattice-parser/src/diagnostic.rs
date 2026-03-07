//! Source map and diagnostic rendering utilities.
//!
//! Converts byte offsets to line/column pairs and renders caret-style error messages.

/// Converts byte offsets to (line, column) pairs for a given source string.
pub struct SourceMap<'a> {
    source: &'a str,
    /// Byte offset of the start of each line.
    line_starts: Vec<usize>,
}

/// A line/column location (1-indexed).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Location {
    pub line: usize,   // 1-indexed
    pub column: usize, // 1-indexed
}

impl<'a> SourceMap<'a> {
    pub fn new(source: &'a str) -> Self {
        let mut line_starts = vec![0];
        for (i, ch) in source.char_indices() {
            if ch == '\n' {
                line_starts.push(i + 1);
            }
        }
        Self {
            source,
            line_starts,
        }
    }

    /// Convert a byte offset to a (line, column) location.
    pub fn location(&self, offset: usize) -> Location {
        let line = self
            .line_starts
            .partition_point(|&start| start <= offset)
            .saturating_sub(1);
        let col = offset - self.line_starts[line];
        Location {
            line: line + 1,
            column: col + 1,
        }
    }

    /// Get the source text for a given 1-indexed line number.
    pub fn line_text(&self, line: usize) -> &str {
        let idx = line - 1;
        if idx >= self.line_starts.len() {
            return "";
        }
        let start = self.line_starts[idx];
        let end = if idx + 1 < self.line_starts.len() {
            self.line_starts[idx + 1]
        } else {
            self.source.len()
        };
        self.source[start..end].trim_end_matches('\n')
    }

    pub fn total_lines(&self) -> usize {
        self.line_starts.len()
    }
}

/// Severity level of a diagnostic.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Error,
    Warning,
}

/// A structured diagnostic message with source location.
#[derive(Debug, Clone)]
pub struct Diagnostic {
    pub severity: Severity,
    pub message: String,
    pub offset: usize,
    pub filename: Option<String>,
}

impl Diagnostic {
    pub fn error(message: impl Into<String>, offset: usize) -> Self {
        Self {
            severity: Severity::Error,
            message: message.into(),
            offset,
            filename: None,
        }
    }

    pub fn warning(message: impl Into<String>, offset: usize) -> Self {
        Self {
            severity: Severity::Warning,
            message: message.into(),
            offset,
            filename: None,
        }
    }

    pub fn with_filename(mut self, filename: impl Into<String>) -> Self {
        self.filename = Some(filename.into());
        self
    }

    /// Render this diagnostic as a caret-style error string.
    pub fn render(&self, source_map: &SourceMap<'_>) -> String {
        let loc = source_map.location(self.offset);
        let file = self.filename.as_deref().unwrap_or("<input>");
        let severity_str = match self.severity {
            Severity::Error => "error",
            Severity::Warning => "warning",
        };

        let line_text = source_map.line_text(loc.line);
        let line_num = loc.line;
        let gutter_width = line_num.to_string().len();

        let caret_padding = " ".repeat(loc.column - 1);

        format!(
            "{severity_str}: {msg}\n \
             {pad}--> {file}:{line}:{col}\n \
             {pad} |\n\
            {line_num} | {line_text}\n \
             {pad} | {caret_padding}^\n",
            severity_str = severity_str,
            msg = self.message,
            pad = " ".repeat(gutter_width),
            file = file,
            line = loc.line,
            col = loc.column,
            line_num = line_num,
            line_text = line_text,
            caret_padding = caret_padding,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_map_single_line() {
        let sm = SourceMap::new("hello world");
        assert_eq!(sm.location(0), Location { line: 1, column: 1 });
        assert_eq!(sm.location(5), Location { line: 1, column: 6 });
    }

    #[test]
    fn source_map_multi_line() {
        let src = "line one\nline two\nline three";
        let sm = SourceMap::new(src);
        assert_eq!(sm.location(0), Location { line: 1, column: 1 });
        assert_eq!(sm.location(9), Location { line: 2, column: 1 }); // 'l' of "line two"
        assert_eq!(sm.location(18), Location { line: 3, column: 1 }); // 'l' of "line three"
        assert_eq!(sm.total_lines(), 3);
    }

    #[test]
    fn source_map_line_text() {
        let src = "first\nsecond\nthird";
        let sm = SourceMap::new(src);
        assert_eq!(sm.line_text(1), "first");
        assert_eq!(sm.line_text(2), "second");
        assert_eq!(sm.line_text(3), "third");
    }

    #[test]
    fn diagnostic_render_error() {
        let src = "let x = \"hello\" + 42";
        let sm = SourceMap::new(src);
        let diag = Diagnostic::error("type mismatch: expected String, found Int", 16)
            .with_filename("example.lattice");
        let rendered = diag.render(&sm);
        assert!(rendered.contains("error: type mismatch"));
        assert!(rendered.contains("--> example.lattice:1:17"));
        assert!(rendered.contains("^"));
    }

    #[test]
    fn diagnostic_render_multiline() {
        let src = "let a = 1\nlet b = true\nlet c = a + b";
        let sm = SourceMap::new(src);
        // offset 34 = the 'b' in "a + b" on line 3
        let diag = Diagnostic::error("type mismatch", 34).with_filename("test.lattice");
        let rendered = diag.render(&sm);
        assert!(rendered.contains("--> test.lattice:3:12"));
        assert!(rendered.contains("let c = a + b"));
    }

    #[test]
    fn diagnostic_render_warning() {
        let src = "let x = 42";
        let sm = SourceMap::new(src);
        let diag = Diagnostic::warning("unused variable", 4);
        let rendered = diag.render(&sm);
        assert!(rendered.contains("warning: unused variable"));
        assert!(rendered.contains("--> <input>:1:5"));
    }
}
