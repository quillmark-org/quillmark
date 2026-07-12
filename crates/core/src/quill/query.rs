//! Quill file/query convenience methods.
use std::path::{Path, PathBuf};

use super::{FileTreeNode, Quill};

impl Quill {
    /// Get file contents by path (relative to quill root)
    pub fn get_file<P: AsRef<Path>>(&self, path: P) -> Option<&[u8]> {
        self.files.get_file(path)
    }

    /// Check if a file exists in memory
    pub fn file_exists<P: AsRef<Path>>(&self, path: P) -> bool {
        self.files.file_exists(path)
    }

    /// Check if a directory exists in memory
    pub fn dir_exists<P: AsRef<Path>>(&self, path: P) -> bool {
        self.files.dir_exists(path)
    }

    /// List files in a directory (non-recursive, returns file names only)
    pub fn list_files<P: AsRef<Path>>(&self, path: P) -> Vec<String> {
        self.files.list_files(path)
    }

    /// List subdirectories in a directory (non-recursive, returns directory names only)
    pub fn list_subdirectories<P: AsRef<Path>>(&self, path: P) -> Vec<String> {
        self.files.list_subdirectories(path)
    }

    /// List all directories in a directory (returns paths relative to quill root)
    pub fn list_directories<P: AsRef<Path>>(&self, dir_path: P) -> Vec<PathBuf> {
        let dir_path = dir_path.as_ref();
        let subdirs = self.files.list_subdirectories(dir_path);

        // Convert subdirectory names to full paths
        subdirs
            .iter()
            .map(|name| {
                if dir_path == Path::new("") {
                    PathBuf::from(name)
                } else {
                    dir_path.join(name)
                }
            })
            .collect()
    }

    /// Get all files matching a pattern (supports glob-style wildcards)
    pub fn find_files<P: AsRef<Path>>(&self, pattern: P) -> Vec<PathBuf> {
        let pattern_str = pattern.as_ref().to_string_lossy();
        let mut matches = Vec::new();

        // Compile the glob pattern
        let glob_pattern = match glob::Pattern::new(&pattern_str) {
            Ok(pat) => pat,
            Err(_) => return matches, // Invalid pattern returns empty results
        };

        // Recursively search the tree for matching files
        Self::find_files_recursive(&self.files, Path::new(""), &glob_pattern, &mut matches);

        matches.sort();
        matches
    }

    /// Helper method to recursively search for files matching a pattern
    fn find_files_recursive(
        node: &FileTreeNode,
        current_path: &Path,
        pattern: &glob::Pattern,
        matches: &mut Vec<PathBuf>,
    ) {
        match node {
            FileTreeNode::File { .. } => {
                let path_str = current_path.to_string_lossy();
                if pattern.matches(&path_str) {
                    matches.push(current_path.to_path_buf());
                }
            }
            FileTreeNode::Directory { files } => {
                for (name, child_node) in files {
                    let child_path = if current_path == Path::new("") {
                        PathBuf::from(name)
                    } else {
                        current_path.join(name)
                    };
                    Self::find_files_recursive(child_node, &child_path, pattern, matches);
                }
            }
        }
    }
}
