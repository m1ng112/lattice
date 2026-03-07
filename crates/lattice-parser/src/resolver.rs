//! Module resolver for Lattice import statements.
//!
//! Resolves module paths to source files, parses them, and returns
//! the exported items. Handles circular dependency detection.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use crate::ast::{Import, ImportName, Item, Program, Spanned};
use crate::parser;

/// Errors produced during module resolution.
#[derive(Debug, Clone)]
pub enum ResolveError {
    /// Module file not found.
    NotFound { path: Vec<String>, searched: Vec<PathBuf> },
    /// Circular dependency detected.
    Circular { path: Vec<String> },
    /// Parse error in imported module.
    ParseError { path: Vec<String>, errors: Vec<String> },
    /// Imported name not found in module.
    NameNotFound { module: Vec<String>, name: String },
}

impl std::fmt::Display for ResolveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ResolveError::NotFound { path, searched } => {
                write!(f, "module `{}` not found", path.join("."))?;
                if !searched.is_empty() {
                    write!(f, " (searched: ")?;
                    for (i, p) in searched.iter().enumerate() {
                        if i > 0 { write!(f, ", ")?; }
                        write!(f, "{}", p.display())?;
                    }
                    write!(f, ")")?;
                }
                Ok(())
            }
            ResolveError::Circular { path } => {
                write!(f, "circular dependency detected: {}", path.join("."))
            }
            ResolveError::ParseError { path, errors } => {
                write!(f, "parse error in module `{}`: {}", path.join("."), errors.join("; "))
            }
            ResolveError::NameNotFound { module, name } => {
                write!(f, "name `{}` not found in module `{}`", name, module.join("."))
            }
        }
    }
}

impl std::error::Error for ResolveError {}

/// A resolved module with its exported items.
#[derive(Debug, Clone)]
pub struct ResolvedModule {
    /// The module path (e.g., `["std", "math"]`).
    pub path: Vec<String>,
    /// The source file path.
    pub source_path: PathBuf,
    /// Parsed and resolved items.
    pub items: Program,
}

/// Module resolver that finds, parses, and caches modules.
pub struct ModuleResolver {
    /// Search paths for module files.
    search_paths: Vec<PathBuf>,
    /// Cache of already-resolved modules.
    cache: HashMap<String, ResolvedModule>,
    /// Currently resolving (for circular dependency detection).
    resolving: HashSet<String>,
}

impl ModuleResolver {
    pub fn new() -> Self {
        Self {
            search_paths: Vec::new(),
            cache: HashMap::new(),
            resolving: HashSet::new(),
        }
    }

    /// Add a directory to the module search path.
    pub fn add_search_path(&mut self, path: PathBuf) {
        self.search_paths.push(path);
    }

    /// Resolve an import statement and return the exported items.
    pub fn resolve_import(&mut self, import: &Import) -> Result<Vec<Spanned<Item>>, ResolveError> {
        let module_key = import.path.join(".");

        // Check for circular dependency.
        if self.resolving.contains(&module_key) {
            return Err(ResolveError::Circular { path: import.path.clone() });
        }

        // Return cached module if available.
        if let Some(cached) = self.cache.get(&module_key) {
            return filter_imports(&cached.items, import);
        }

        // Find the module file.
        let source_path = self.find_module_file(&import.path)?;

        // Read and parse.
        let source = std::fs::read_to_string(&source_path).map_err(|_| {
            ResolveError::NotFound {
                path: import.path.clone(),
                searched: vec![source_path.clone()],
            }
        })?;

        self.resolving.insert(module_key.clone());
        let items = parser::parse(&source).map_err(|errs| {
            ResolveError::ParseError {
                path: import.path.clone(),
                errors: errs.iter().map(|e| e.message.clone()).collect(),
            }
        })?;
        self.resolving.remove(&module_key);

        let resolved = ResolvedModule {
            path: import.path.clone(),
            source_path,
            items,
        };

        let result = filter_imports(&resolved.items, import);
        self.cache.insert(module_key, resolved);
        result
    }

    /// Find a module file on disk. Searches `<search_path>/<path_segments>.lattice`.
    fn find_module_file(&self, path: &[String]) -> Result<PathBuf, ResolveError> {
        let mut searched = Vec::new();

        for base in &self.search_paths {
            // Try `base/path/to/module.lattice`
            let mut file_path = base.clone();
            for segment in path {
                file_path.push(segment);
            }
            file_path.set_extension("lattice");
            searched.push(file_path.clone());
            if file_path.exists() {
                return Ok(file_path);
            }

            // Try `base/path/to/module/mod.lattice`
            let mut dir_path = base.clone();
            for segment in path {
                dir_path.push(segment);
            }
            dir_path.push("mod.lattice");
            searched.push(dir_path.clone());
            if dir_path.exists() {
                return Ok(dir_path);
            }
        }

        Err(ResolveError::NotFound {
            path: path.to_vec(),
            searched,
        })
    }

    /// Get a reference to the resolved module cache.
    pub fn resolved_modules(&self) -> &HashMap<String, ResolvedModule> {
        &self.cache
    }
}

impl Default for ModuleResolver {
    fn default() -> Self {
        Self::new()
    }
}

/// Filter module items based on selective imports.
fn filter_imports(items: &[Spanned<Item>], import: &Import) -> Result<Vec<Spanned<Item>>, ResolveError> {
    match &import.names {
        None => {
            // Import all items.
            Ok(items.to_vec())
        }
        Some(names) => {
            let mut result = Vec::new();
            for imp_name in names {
                let found = items.iter().find(|item| {
                    item_name(&item.node) == Some(&imp_name.name)
                });
                match found {
                    Some(item) => {
                        if let Some(alias) = &imp_name.alias {
                            // Clone and rename the item.
                            let mut aliased = item.clone();
                            rename_item(&mut aliased.node, alias);
                            result.push(aliased);
                        } else {
                            result.push(item.clone());
                        }
                    }
                    None => {
                        return Err(ResolveError::NameNotFound {
                            module: import.path.clone(),
                            name: imp_name.name.clone(),
                        });
                    }
                }
            }
            Ok(result)
        }
    }
}

/// Get the name of a top-level item, if it has one.
fn item_name(item: &Item) -> Option<&str> {
    match item {
        Item::Function(f) => Some(&f.name),
        Item::TypeDef(td) => Some(&td.name),
        Item::LetBinding(lb) => Some(&lb.name),
        Item::Module(m) => Some(&m.name),
        Item::Model(m) => Some(&m.name),
        Item::Graph(g) => Some(&g.name),
        Item::Meta(m) => Some(&m.name),
        Item::Import(_) => None,
    }
}

/// Rename an item (for aliased imports like `import math.{sin as sine}`).
fn rename_item(item: &mut Item, new_name: &str) {
    match item {
        Item::Function(f) => f.name = new_name.to_string(),
        Item::TypeDef(td) => td.name = new_name.to_string(),
        Item::LetBinding(lb) => lb.name = new_name.to_string(),
        Item::Module(m) => m.name = new_name.to_string(),
        Item::Model(m) => m.name = new_name.to_string(),
        Item::Graph(g) => g.name = new_name.to_string(),
        Item::Meta(m) => m.name = new_name.to_string(),
        Item::Import(_) => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn write_module(dir: &Path, path: &str, content: &str) {
        let file_path = dir.join(path);
        if let Some(parent) = file_path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(file_path, content).unwrap();
    }

    #[test]
    fn resolve_simple_module() {
        let tmp = TempDir::new().unwrap();
        write_module(tmp.path(), "math.lattice", r#"
let pi = 3
let e = 2
"#);

        let mut resolver = ModuleResolver::new();
        resolver.add_search_path(tmp.path().to_path_buf());

        let import = Import {
            path: vec!["math".into()],
            names: None,
        };
        let items = resolver.resolve_import(&import).unwrap();
        assert_eq!(items.len(), 2);
    }

    #[test]
    fn resolve_selective_import() {
        let tmp = TempDir::new().unwrap();
        write_module(tmp.path(), "math.lattice", r#"
let pi = 3
let e = 2
let tau = 6
"#);

        let mut resolver = ModuleResolver::new();
        resolver.add_search_path(tmp.path().to_path_buf());

        let import = Import {
            path: vec!["math".into()],
            names: Some(vec![
                ImportName { name: "pi".into(), alias: None },
                ImportName { name: "e".into(), alias: None },
            ]),
        };
        let items = resolver.resolve_import(&import).unwrap();
        assert_eq!(items.len(), 2);
    }

    #[test]
    fn resolve_aliased_import() {
        let tmp = TempDir::new().unwrap();
        write_module(tmp.path(), "math.lattice", "let pi = 3\n");

        let mut resolver = ModuleResolver::new();
        resolver.add_search_path(tmp.path().to_path_buf());

        let import = Import {
            path: vec!["math".into()],
            names: Some(vec![
                ImportName { name: "pi".into(), alias: Some("PI".into()) },
            ]),
        };
        let items = resolver.resolve_import(&import).unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(item_name(&items[0].node), Some("PI"));
    }

    #[test]
    fn resolve_nested_path() {
        let tmp = TempDir::new().unwrap();
        write_module(tmp.path(), "std/math.lattice", "let sqrt = 1\n");

        let mut resolver = ModuleResolver::new();
        resolver.add_search_path(tmp.path().to_path_buf());

        let import = Import {
            path: vec!["std".into(), "math".into()],
            names: None,
        };
        let items = resolver.resolve_import(&import).unwrap();
        assert_eq!(items.len(), 1);
    }

    #[test]
    fn resolve_mod_file() {
        let tmp = TempDir::new().unwrap();
        write_module(tmp.path(), "collections/mod.lattice", "let list = 1\n");

        let mut resolver = ModuleResolver::new();
        resolver.add_search_path(tmp.path().to_path_buf());

        let import = Import {
            path: vec!["collections".into()],
            names: None,
        };
        let items = resolver.resolve_import(&import).unwrap();
        assert_eq!(items.len(), 1);
    }

    #[test]
    fn detect_circular_dependency() {
        let tmp = TempDir::new().unwrap();
        write_module(tmp.path(), "a.lattice", "import b\nlet x = 1\n");
        write_module(tmp.path(), "b.lattice", "import a\nlet y = 2\n");

        let mut resolver = ModuleResolver::new();
        resolver.add_search_path(tmp.path().to_path_buf());

        // Manually simulate: start resolving "a", then "b" tries to import "a"
        resolver.resolving.insert("a".into());
        let import = Import {
            path: vec!["a".into()],
            names: None,
        };
        let result = resolver.resolve_import(&import);
        assert!(result.is_err());
        match result.unwrap_err() {
            ResolveError::Circular { path } => assert_eq!(path, vec!["a"]),
            other => panic!("expected Circular, got: {}", other),
        }
    }

    #[test]
    fn module_not_found() {
        let tmp = TempDir::new().unwrap();
        let mut resolver = ModuleResolver::new();
        resolver.add_search_path(tmp.path().to_path_buf());

        let import = Import {
            path: vec!["nonexistent".into()],
            names: None,
        };
        let result = resolver.resolve_import(&import);
        assert!(matches!(result, Err(ResolveError::NotFound { .. })));
    }

    #[test]
    fn name_not_found_in_module() {
        let tmp = TempDir::new().unwrap();
        write_module(tmp.path(), "math.lattice", "let pi = 3\n");

        let mut resolver = ModuleResolver::new();
        resolver.add_search_path(tmp.path().to_path_buf());

        let import = Import {
            path: vec!["math".into()],
            names: Some(vec![
                ImportName { name: "nonexistent".into(), alias: None },
            ]),
        };
        let result = resolver.resolve_import(&import);
        assert!(matches!(result, Err(ResolveError::NameNotFound { .. })));
    }

    #[test]
    fn caching_resolved_modules() {
        let tmp = TempDir::new().unwrap();
        write_module(tmp.path(), "math.lattice", "let pi = 3\n");

        let mut resolver = ModuleResolver::new();
        resolver.add_search_path(tmp.path().to_path_buf());

        let import = Import {
            path: vec!["math".into()],
            names: None,
        };
        let _ = resolver.resolve_import(&import).unwrap();

        // Second resolve should use cache
        assert!(resolver.resolved_modules().contains_key("math"));
        let items = resolver.resolve_import(&import).unwrap();
        assert_eq!(items.len(), 1);
    }
}
