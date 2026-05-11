//! .quillignore parsing and path matching.
use std::path::Path;

/// Simple gitignore-style pattern matcher for .quillignore
#[derive(Debug, Clone)]
pub struct QuillIgnore {
    pub(crate) patterns: Vec<String>,
}

impl Default for QuillIgnore {
    /// Built-in ignore set used when a quill directory has no `.quillignore`
    /// file. Skips VCS metadata, build artifacts, and dependency caches.
    fn default() -> Self {
        Self::new(vec![
            ".git/".to_string(),
            ".gitignore".to_string(),
            ".quillignore".to_string(),
            "target/".to_string(),
            "node_modules/".to_string(),
        ])
    }
}

impl QuillIgnore {
    /// Create a new QuillIgnore from pattern strings
    pub fn new(patterns: Vec<String>) -> Self {
        Self { patterns }
    }

    /// Parse .quillignore content into patterns
    pub fn from_content(content: &str) -> Self {
        let patterns = content
            .lines()
            .map(|line| line.trim())
            .filter(|line| !line.is_empty() && !line.starts_with('#'))
            .map(|line| line.to_string())
            .collect();
        Self::new(patterns)
    }

    /// Check if a path should be ignored
    pub fn is_ignored<P: AsRef<Path>>(&self, path: P) -> bool {
        let path = path.as_ref();
        let path_str = path.to_string_lossy();

        for pattern in &self.patterns {
            if self.matches_pattern(pattern, &path_str) {
                return true;
            }
        }
        false
    }

    /// Simple pattern matching (supports * wildcard and directory patterns)
    fn matches_pattern(&self, pattern: &str, path: &str) -> bool {
        // Handle directory patterns
        if let Some(pattern_prefix) = pattern.strip_suffix('/') {
            return path.starts_with(pattern_prefix)
                && (path.len() == pattern_prefix.len()
                    || path.chars().nth(pattern_prefix.len()) == Some('/'));
        }

        // Handle exact matches
        if !pattern.contains('*') {
            return path == pattern || path.ends_with(&format!("/{}", pattern));
        }

        // Simple wildcard matching
        if pattern == "*" {
            return true;
        }

        // Handle patterns with wildcards
        let pattern_parts: Vec<&str> = pattern.split('*').collect();
        if pattern_parts.len() == 2 {
            let (prefix, suffix) = (pattern_parts[0], pattern_parts[1]);
            if prefix.is_empty() {
                return path.ends_with(suffix);
            } else if suffix.is_empty() {
                return path.starts_with(prefix);
            } else {
                return path.starts_with(prefix) && path.ends_with(suffix);
            }
        }

        false
    }
}
