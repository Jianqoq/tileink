use std::{
    collections::BTreeMap,
    io::{BufRead, BufReader, Read, Write},
    path::Path,
    process::{Child, ChildStdout, Command, Stdio},
};

use crate::Result;

pub fn command(root: &Path, args: &[&str]) -> Result<Vec<u8>> {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()?;
    if !output.status.success() {
        return Err(format!("git {args:?}: {}", String::from_utf8_lossy(&output.stderr)).into());
    }
    Ok(output.stdout)
}

fn is_png(path: &str) -> bool {
    Path::new(path)
        .extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("png"))
}

pub fn files(root: &Path, revision: &str) -> Result<BTreeMap<String, Option<String>>> {
    let mut files = BTreeMap::new();
    // NUL-delimited names and blob IDs avoid shell quoting, line-ending conversion and
    // ambiguous revision:path expressions when filenames contain spaces or newlines.
    let tree = command(root, &["ls-tree", "-r", "-z", "--full-tree", revision])?;
    for entry in tree
        .split(|byte| *byte == 0)
        .filter(|entry| !entry.is_empty())
    {
        let entry = std::str::from_utf8(entry)?;
        let (metadata, path) = entry.split_once('\t').ok_or("invalid git tree entry")?;
        if is_png(path) {
            let oid = metadata
                .split_whitespace()
                .nth(2)
                .ok_or("missing blob ID")?;
            files.insert(path.to_owned(), Some(oid.to_owned()));
        }
    }
    let working = command(
        root,
        &[
            "ls-files",
            "-z",
            "--cached",
            "--others",
            "--exclude-standard",
        ],
    )?;
    for path in working
        .split(|byte| *byte == 0)
        .filter(|path| !path.is_empty())
    {
        let path = std::str::from_utf8(path)?;
        if is_png(path) {
            files.entry(path.to_owned()).or_insert(None);
        }
    }
    Ok(files)
}

pub struct Blobs {
    child: Child,
    output: BufReader<ChildStdout>,
}

impl Blobs {
    pub fn new(root: &Path) -> Result<Self> {
        let mut child = Command::new("git")
            .arg("-C")
            .arg(root)
            .args(["cat-file", "--batch"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()?;
        let output = BufReader::new(child.stdout.take().ok_or("missing git stdout")?);
        Ok(Self { child, output })
    }

    pub fn read(&mut self, oid: &str) -> Result<Vec<u8>> {
        let input = self.child.stdin.as_mut().ok_or("missing git stdin")?;
        writeln!(input, "{oid}")?;
        input.flush()?;
        let mut header = String::new();
        self.output.read_line(&mut header)?;
        let parts: Vec<_> = header.split_whitespace().collect();
        if parts.len() != 3 || parts[0] != oid || parts[1] != "blob" {
            return Err(format!("unexpected git blob response: {header:?}").into());
        }
        let mut bytes = vec![0; parts[2].parse()?];
        self.output.read_exact(&mut bytes)?;
        let mut newline = [0];
        self.output.read_exact(&mut newline)?;
        if newline[0] != b'\n' {
            return Err("invalid git blob terminator".into());
        }
        Ok(bytes)
    }
}

impl Drop for Blobs {
    fn drop(&mut self) {
        // Also reap the child on a broken response or an early report-writing error.
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
