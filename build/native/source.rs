use std::{
    collections::BTreeMap,
    fs, io,
    path::{Path, PathBuf},
};

use crate::gpu_constants::syntax;

pub struct SourceGraph {
    pub expanded: String,
    pub files: BTreeMap<String, String>,
}

impl SourceGraph {
    pub fn load(root: &Path, entry: &str) -> io::Result<Self> {
        let root = root.canonicalize()?;
        let mut graph = Self {
            expanded: String::new(),
            files: BTreeMap::new(),
        };
        graph.expanded = graph.expand(
            &root,
            &root.join(entry),
            &mut Vec::new(),
            &mut BTreeMap::new(),
        )?;
        Ok(graph)
    }

    fn expand(
        &mut self,
        root: &Path,
        path: &Path,
        stack: &mut Vec<PathBuf>,
        guards: &mut BTreeMap<String, PathBuf>,
    ) -> io::Result<String> {
        let path = path.canonicalize()?;
        let name = path
            .strip_prefix(root)
            .map_err(|_| io::Error::other("shader include escapes source root"))?
            .to_string_lossy()
            .replace('\\', "/");
        let source = fs::read_to_string(&path)?;
        let (lines, guard) =
            syntax::parse(&source).map_err(|error| io::Error::other(format!("{name}: {error}")))?;
        if let Some(guard) = guard {
            if let Some(previous) = guards.get(&guard) {
                if previous == &path {
                    return Ok(String::new());
                }
                // Reusing a macro in another header would silently hide that header in DXC.
                return Err(io::Error::other(format!(
                    "duplicate include guard {guard}: {} and {name}",
                    previous.display()
                )));
            }
            guards.insert(guard, path.clone());
        }
        if stack.contains(&path) {
            return Err(io::Error::other(format!("cyclic shader include: {name}")));
        }
        self.files.insert(name.clone(), source);
        stack.push(path.clone());
        let mut expanded = format!("#line 1 \"{name}\"\n");
        for (index, visible) in lines.iter().enumerate() {
            let directive = visible.trim();
            // Only literal includes and the validated whole-file guard are supported.
            if directive.ends_with('\\')
                || (directive.starts_with('#') && !directive.starts_with("#include "))
            {
                return Err(io::Error::other(format!(
                    "unsupported shader preprocessor syntax: {name}:{}",
                    index + 1
                )));
            }
            if let Some(include) = directive.strip_prefix("#include ") {
                let include = include.trim();
                // The pinned Metal SDK owns this standard-library dependency.
                if include == "<metal_stdlib>" {
                    expanded.push_str(directive);
                    expanded.push('\n');
                    continue;
                }
                let relative = include
                    .strip_prefix('"')
                    .and_then(|s| s.strip_suffix('"'))
                    .filter(|s| !s.contains('"'))
                    .ok_or_else(|| {
                        io::Error::other(format!(
                            "only literal local shader includes are supported: {name}:{}",
                            index + 1
                        ))
                    })?;
                expanded.push_str(&self.expand(
                    root,
                    &path.parent().unwrap().join(relative),
                    stack,
                    guards,
                )?);
                expanded.push_str(&format!("#line {} \"{name}\"\n", index + 2));
            } else {
                expanded.push_str(visible);
                expanded.push('\n');
            }
        }
        stack.pop();
        Ok(expanded)
    }
}
