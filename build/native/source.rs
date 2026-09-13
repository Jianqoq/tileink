//! Load a closed, literal include graph and compile the exact expanded bytes.
use std::{
    collections::BTreeMap,
    fs, io,
    path::{Path, PathBuf},
};

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
        graph.expanded = graph.expand(&root, &root.join(entry), &mut Vec::new())?;
        Ok(graph)
    }

    fn expand(&mut self, root: &Path, path: &Path, stack: &mut Vec<PathBuf>) -> io::Result<String> {
        let path = path.canonicalize()?;
        let name = path
            .strip_prefix(root)
            .map_err(|_| io::Error::other("shader include escapes source root"))?
            .to_string_lossy()
            .replace('\\', "/");
        if stack.contains(&path) {
            return Err(io::Error::other(format!("cyclic shader include: {name}")));
        }
        let source = fs::read_to_string(&path)?;
        self.files.insert(name.clone(), source.clone());
        stack.push(path.clone());
        let mut expanded = format!("#line 1 \"{name}\"\n");
        let mut block_comment = false;
        for (index, line) in source.lines().enumerate() {
            let visible = visible_line(line, &mut block_comment);
            let directive = visible.trim();
            // This first native source format deliberately supports literal
            // includes only. Never pass an untracked preprocessor directive or
            // line splice through to a compiler with a different lexer.
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
                // The Metal standard library belongs to the pinned SDK, not the
                // project graph; target-specific tool identity covers that SDK.
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
                )?);
                expanded.push_str(&format!("#line {} \"{name}\"\n", index + 2));
            } else {
                expanded.push_str(&visible);
                expanded.push('\n');
            }
        }
        if block_comment {
            return Err(io::Error::other(format!(
                "unterminated shader comment: {name}"
            )));
        }
        stack.pop();
        Ok(expanded)
    }
}

fn visible_line(line: &str, block: &mut bool) -> String {
    let mut out = String::new();
    let mut chars = line.chars().peekable();
    let mut quoted = false;
    while let Some(c) = chars.next() {
        if *block {
            if c == '*' && chars.peek() == Some(&'/') {
                chars.next();
                *block = false;
            }
        } else if quoted {
            out.push(c);
            if c == '\\' {
                if let Some(next) = chars.next() {
                    out.push(next);
                }
            } else if c == '"' {
                quoted = false;
            }
        } else if c == '/' && chars.peek() == Some(&'/') {
            break;
        } else if c == '/' && chars.peek() == Some(&'*') {
            chars.next();
            *block = true;
            out.push(' ');
        } else {
            out.push(c);
            if c == '"' {
                quoted = true;
            }
        }
    }
    out
}
