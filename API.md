# ra-lsp HTTP JSON API

`ra-lsp` wraps `rust-analyzer` behind plain HTTP endpoints and returns `application/json`.

## How To Run

Prerequisites:

- `rust-analyzer` is installed and available on `PATH`
- start `ra-lsp` from the Rust workspace root you want to analyze

Install `ra-lsp` from the packaged folder:

```powershell
./install.ps1
```

Or:

```cmd
install.cmd
```

After installation, open a new terminal and start the server with:

```powershell
ra-lsp
```

Defaults:

- `workspace root`: current directory
- `port`: `3000`
- `host`: `127.0.0.1`

Examples:

```powershell
cd C:\project
ra-lsp
ra-lsp --port 3001 --host 0.0.0.0
```

You can still override:

- `--port`: HTTP port, for example `3000`
- `--host`: bind address, for example `127.0.0.1`

Optional flags:

```powershell
ra-lsp --ra-binary rust-analyzer --log-response
```

**Base URL:** `http://127.0.0.1:3000`  
**Health:** `GET /health`

Coordinate format:

- `Position` -> `"line:col"`
- `TextRange` -> `"startLine:startCol,endLine:endCol"`

## Endpoints

### `POST /document-symbols`

Request:

```json
{
  "file": "C:/project/src/main.rs"
}
```

### `POST /hovers-in-code`

Request:

```json
{
  "file": "C:/project/src/main.rs",
  "range": "10:0,20:1",
  "code": "some_call"
}
```

Response item:

```json
{
  "name": "some_call",
  "hover": "fn some_call(x: i32) -> i32"
}
```

### `POST /definitions-in-code`

Request:

```json
{
  "file": "C:/project/src/main.rs",
  "range": "10:0,20:1",
  "code": "some_call"
}
```

Response item:

```json
{
  "name": "some_call",
  "definition": {
    "path": "C:/project/src/lib.rs",
    "range": "42:4,42:13",
    "code": "some_call"
  }
}
```

### `POST /declarations-in-code`

Request:

```json
{
  "file": "C:/project/src/main.rs",
  "range": "10:0,20:1",
  "code": "SomeTrait"
}
```

Response item:

```json
{
  "name": "SomeTrait",
  "declaration": {
    "path": "C:/project/src/traits.rs",
    "range": "8:0,8:9",
    "code": "trait SomeTrait"
  }
}
```

### `POST /type-definitions-in-code`

Request:

```json
{
  "file": "C:/project/src/main.rs",
  "range": "10:0,20:1",
  "code": "value"
}
```

Response item:

```json
{
  "name": "value",
  "type_definition": {
    "path": "C:/project/src/types.rs",
    "range": "15:0,15:8",
    "code": "struct Value"
  }
}
```

### `POST /implementations-in-code`

Request:

```json
{
  "file": "C:/Users/JianJian Li/ra-lsp/ra-lsp/src/lsp_client.rs",
  "range": "22:4,22:38",
  "code": "implementation"
}
```

Response item:

```json
{
  "name": "implementation",
  "implementations": [
    {
      "path": "C:/project/src/foo.rs",
      "range": "12:0,18:1",
      "code": "impl Serialize for Foo"
    }
  ]
}
```

### `POST /workspace-search`

Request:

```json
{
  "query": "fetch_document_symbols",
  "limit": 20
}
```

### `POST /workspace-grep`

Request:

```json
{
  "pattern": "impl\\s+\\w+\\s+for\\s+\\w+",
  "maxResults": 100,
  "caseInsensitive": false,
  "includeHidden": false
}
```

Response item:

```json
{
  "path": "C:/project/src/foo.rs",
  "line_number": 12,
  "line": "impl Serialize for Foo {"
}
```

## Notes

- `file` must be an absolute path
- `line` and `character` are 0-based
- all responses are JSON
- `code` is the symbol text to locate inside the provided `range`
- `workspace-grep` uses the `grep` regex engine syntax, not shell glob syntax
