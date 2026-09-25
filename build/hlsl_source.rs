//! Shared lexical handling for source expansion and HLSL constant extraction.
use std::io;

/// Strip comments and a whole-file conventional include guard, preserving line numbers.
/// Other directives remain visible for the caller to reject or process explicitly.
pub fn parse(source: &str) -> io::Result<(Vec<String>, Option<String>)> {
    let mut block_comment = false;
    let mut lines: Vec<_> = source
        .lines()
        .map(|line| visible_line(line, &mut block_comment))
        .collect();
    if block_comment {
        return Err(io::Error::other("unterminated shader comment"));
    }
    let visible: Vec<_> = lines
        .iter()
        .enumerate()
        .filter(|(_, line)| !line.trim().is_empty())
        .map(|(i, _)| i)
        .collect();
    let Some(&first) = visible.first() else {
        return Ok((lines, None));
    };
    let Some(guard) = lines[first].trim().strip_prefix("#ifndef ") else {
        return Ok((lines, None));
    };
    let guard = guard.trim().to_owned();
    let valid = !guard.is_empty()
        && guard
            .bytes()
            .next()
            .is_some_and(|c| c.is_ascii_uppercase() || c == b'_')
        && guard
            .bytes()
            .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == b'_');
    if !valid
        || visible.len() < 3
        || lines[visible[1]].trim() != format!("#define {guard}")
        || lines[*visible.last().unwrap()].trim() != "#endif"
    {
        return Err(io::Error::other(
            "expected matching whole-file #ifndef/#define/#endif guard",
        ));
    }
    lines[first].clear();
    lines[visible[1]].clear();
    lines[*visible.last().unwrap()].clear();
    Ok((lines, Some(guard)))
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
