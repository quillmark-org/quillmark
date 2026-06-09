//! In-memory file tree representation for quill bundles.
use std::collections::HashMap;
use std::error::Error as StdError;
use std::path::Path;
/// A node in the file tree structure
#[derive(Debug, Clone)]
pub enum FileTreeNode {
    /// A file with its contents
    File {
        /// The file contents as bytes or UTF-8 string
        contents: Vec<u8>,
    },
    /// A directory containing other files and directories
    Directory {
        /// The files and subdirectories in this directory
        files: HashMap<String, FileTreeNode>,
    },
}

impl FileTreeNode {
    /// Get a file or directory node by path
    pub fn get_node<P: AsRef<Path>>(&self, path: P) -> Option<&FileTreeNode> {
        let path = path.as_ref();

        // Handle root path
        if path == Path::new("") {
            return Some(self);
        }

        // Split path into components
        let components: Vec<_> = path
            .components()
            .filter_map(|c| {
                if let std::path::Component::Normal(s) = c {
                    s.to_str()
                } else {
                    None
                }
            })
            .collect();

        if components.is_empty() {
            return Some(self);
        }

        // Navigate through the tree
        let mut current_node = self;
        for component in components {
            match current_node {
                FileTreeNode::Directory { files } => {
                    current_node = files.get(component)?;
                }
                FileTreeNode::File { .. } => {
                    return None; // Can't traverse into a file
                }
            }
        }

        Some(current_node)
    }

    /// Get file contents by path
    pub fn get_file<P: AsRef<Path>>(&self, path: P) -> Option<&[u8]> {
        match self.get_node(path)? {
            FileTreeNode::File { contents } => Some(contents.as_slice()),
            FileTreeNode::Directory { .. } => None,
        }
    }

    /// Check if a file exists at the given path
    pub fn file_exists<P: AsRef<Path>>(&self, path: P) -> bool {
        matches!(self.get_node(path), Some(FileTreeNode::File { .. }))
    }

    /// Check if a directory exists at the given path
    pub fn dir_exists<P: AsRef<Path>>(&self, path: P) -> bool {
        matches!(self.get_node(path), Some(FileTreeNode::Directory { .. }))
    }

    /// List all files in a directory (non-recursive)
    pub fn list_files<P: AsRef<Path>>(&self, dir_path: P) -> Vec<String> {
        match self.get_node(dir_path) {
            Some(FileTreeNode::Directory { files }) => files
                .iter()
                .filter_map(|(name, node)| {
                    if matches!(node, FileTreeNode::File { .. }) {
                        Some(name.clone())
                    } else {
                        None
                    }
                })
                .collect(),
            _ => Vec::new(),
        }
    }

    /// List all subdirectories in a directory (non-recursive)
    pub fn list_subdirectories<P: AsRef<Path>>(&self, dir_path: P) -> Vec<String> {
        match self.get_node(dir_path) {
            Some(FileTreeNode::Directory { files }) => files
                .iter()
                .filter_map(|(name, node)| {
                    if matches!(node, FileTreeNode::Directory { .. }) {
                        Some(name.clone())
                    } else {
                        None
                    }
                })
                .collect(),
            _ => Vec::new(),
        }
    }

    /// Insert a file or directory at the given path
    pub fn insert<P: AsRef<Path>>(
        &mut self,
        path: P,
        node: FileTreeNode,
    ) -> Result<(), Box<dyn StdError + Send + Sync>> {
        let path = path.as_ref();

        // Validate and collect path components, rejecting any non-Normal component
        // so that `..`, `.`, and absolute roots are errors rather than silent no-ops.
        let mut components: Vec<String> = Vec::new();
        for c in path.components() {
            match c {
                std::path::Component::Normal(s) => {
                    components.push(
                        s.to_str()
                            .ok_or("Path component is not valid UTF-8")?
                            .to_string(),
                    );
                }
                std::path::Component::ParentDir => {
                    return Err("Path traversal ('..') is not allowed".into());
                }
                std::path::Component::CurDir => {
                    return Err("Current-directory ('.') components are not allowed".into());
                }
                std::path::Component::RootDir | std::path::Component::Prefix(_) => {
                    return Err("Absolute paths are not allowed; use a relative path".into());
                }
            }
        }

        if components.is_empty() {
            return Err("Cannot insert at root path".into());
        }

        // Navigate to parent directory, creating directories as needed
        let mut current_node = self;
        for component in &components[..components.len() - 1] {
            match current_node {
                FileTreeNode::Directory { files } => {
                    current_node =
                        files
                            .entry(component.clone())
                            .or_insert_with(|| FileTreeNode::Directory {
                                files: HashMap::new(),
                            });
                }
                FileTreeNode::File { .. } => {
                    return Err("Cannot traverse into a file".into());
                }
            }
        }

        // Insert the new node
        let filename = &components[components.len() - 1];
        match current_node {
            FileTreeNode::Directory { files } => {
                files.insert(filename.clone(), node);
                Ok(())
            }
            FileTreeNode::File { .. } => Err("Cannot insert into a file".into()),
        }
    }

    /// Flatten the tree into `(path, contents)` pairs — the inverse of building
    /// a tree by `insert`-ing each path. Paths are `"/"`-joined and relative
    /// (no leading slash), exactly the key shape the WASM `Quill.fromTree`
    /// boundary consumes, so `from_tree(flatten(t))` round-trips every file.
    /// Output is sorted by path for deterministic ordering (the construction
    /// side stores children in a `HashMap`, which has no inherent order).
    ///
    /// Only files are emitted: an EMPTY directory yields no entry and so is not
    /// reconstructed by a `flatten` → `insert` round trip. This is intentional —
    /// quill bundles are file-addressed and nothing in load/render depends on
    /// empty directories — but it means the round trip preserves file contents,
    /// not exact directory structure.
    pub fn flatten(&self) -> Vec<(String, Vec<u8>)> {
        let mut out = Vec::new();
        self.flatten_into(String::new(), &mut out);
        out.sort_by(|(a, _), (b, _)| a.cmp(b));
        out
    }

    fn flatten_into(&self, prefix: String, out: &mut Vec<(String, Vec<u8>)>) {
        match self {
            FileTreeNode::File { contents } => {
                // A File only reaches here with a non-empty prefix: the root is
                // always a Directory, so every file is named by its parent.
                if !prefix.is_empty() {
                    out.push((prefix, contents.clone()));
                }
            }
            FileTreeNode::Directory { files } => {
                for (name, node) in files {
                    let path = if prefix.is_empty() {
                        name.clone()
                    } else {
                        format!("{}/{}", prefix, name)
                    };
                    node.flatten_into(path, out);
                }
            }
        }
    }

    pub fn print_tree(&self) -> String {
        self.print_tree_recursive("", "", true)
    }

    fn print_tree_recursive(&self, name: &str, prefix: &str, is_last: bool) -> String {
        let mut result = String::new();

        // Choose the appropriate tree characters
        let connector = if is_last { "└── " } else { "├── " };
        let extension = if is_last { "    " } else { "│   " };

        match self {
            FileTreeNode::File { .. } => {
                result.push_str(&format!("{}{}{}\n", prefix, connector, name));
            }
            FileTreeNode::Directory { files } => {
                // Add trailing slash for directories like `tree` does
                result.push_str(&format!("{}{}{}/\n", prefix, connector, name));

                let child_prefix = format!("{}{}", prefix, extension);
                let count = files.len();

                for (i, (child_name, node)) in files.iter().enumerate() {
                    let is_last_child = i == count - 1;
                    result.push_str(&node.print_tree_recursive(
                        child_name,
                        &child_prefix,
                        is_last_child,
                    ));
                }
            }
        }

        result
    }
}
