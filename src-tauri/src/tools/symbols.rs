use async_trait::async_trait;
use serde_json::Value;
use std::path::{Path, PathBuf};
use tree_sitter::{Language, Node, Parser};

use super::{validate_read_path, Tool, ToolCategory, ToolResult, ToolSpec};

/// Max output size in bytes.
const MAX_OUTPUT: usize = 16 * 1024;
/// Max symbols per file.
const MAX_SYMBOLS_PER_FILE: usize = 200;
/// Max total symbols across a directory walk.
const MAX_SYMBOLS_TOTAL: usize = 500;
/// Skip files larger than this (parsing huge files wastes time).
const MAX_FILE_SIZE: u64 = 512 * 1024;
/// Max files to process in directory mode.
const MAX_FILES: usize = 200;

/// (node_kind, display_label)
type SymbolKinds = &'static [(&'static str, &'static str)];

const RUST_SYMBOLS: SymbolKinds = &[
    ("function_item", "fn"),
    ("struct_item", "struct"),
    ("enum_item", "enum"),
    ("trait_item", "trait"),
    ("impl_item", "impl"),
    ("type_item", "type"),
    ("mod_item", "mod"),
    ("const_item", "const"),
    ("static_item", "static"),
    ("macro_definition", "macro"),
];

const PYTHON_SYMBOLS: SymbolKinds = &[
    ("function_definition", "def"),
    ("class_definition", "class"),
];

const TS_SYMBOLS: SymbolKinds = &[
    ("function_declaration", "function"),
    ("class_declaration", "class"),
    ("interface_declaration", "interface"),
    ("type_alias_declaration", "type"),
    ("enum_declaration", "enum"),
    ("method_definition", "method"),
    ("internal_module", "namespace"),
];

const JS_SYMBOLS: SymbolKinds = &[
    ("function_declaration", "function"),
    ("class_declaration", "class"),
    ("method_definition", "method"),
];

const GO_SYMBOLS: SymbolKinds = &[
    ("function_declaration", "func"),
    ("method_declaration", "method"),
    ("type_spec", "type"),
];

const JAVA_SYMBOLS: SymbolKinds = &[
    ("method_declaration", "method"),
    ("class_declaration", "class"),
    ("interface_declaration", "interface"),
    ("enum_declaration", "enum"),
    ("constructor_declaration", "constructor"),
    ("record_declaration", "record"),
];

const C_SYMBOLS: SymbolKinds = &[
    ("function_definition", "fn"),
    ("struct_specifier", "struct"),
    ("enum_specifier", "enum"),
    ("union_specifier", "union"),
    ("type_definition", "typedef"),
];

const CPP_SYMBOLS: SymbolKinds = &[
    ("function_definition", "fn"),
    ("function_declaration", "fn"),
    ("class_specifier", "class"),
    ("struct_specifier", "struct"),
    ("enum_specifier", "enum"),
    ("namespace_definition", "namespace"),
];

const RUBY_SYMBOLS: SymbolKinds = &[
    ("method", "def"),
    ("singleton_method", "def"),
    ("class", "class"),
    ("module", "module"),
];

const PHP_SYMBOLS: SymbolKinds = &[
    ("function_definition", "function"),
    ("class_declaration", "class"),
    ("method_declaration", "method"),
    ("interface_declaration", "interface"),
];

const BASH_SYMBOLS: SymbolKinds = &[("function_definition", "function")];

/// Map a file extension to its tree-sitter language and symbol-kind table.
fn lang_for_ext(ext: &str) -> Option<(Language, SymbolKinds)> {
    match ext {
        "rs" => Some((tree_sitter_rust::LANGUAGE.into(), RUST_SYMBOLS)),
        "py" | "pyi" => Some((tree_sitter_python::LANGUAGE.into(), PYTHON_SYMBOLS)),
        "ts" => Some((
            tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
            TS_SYMBOLS,
        )),
        "tsx" => Some((tree_sitter_typescript::LANGUAGE_TSX.into(), TS_SYMBOLS)),
        "js" | "mjs" | "cjs" | "jsx" => Some((tree_sitter_javascript::LANGUAGE.into(), JS_SYMBOLS)),
        "go" => Some((tree_sitter_go::LANGUAGE.into(), GO_SYMBOLS)),
        "java" => Some((tree_sitter_java::LANGUAGE.into(), JAVA_SYMBOLS)),
        "c" | "h" => Some((tree_sitter_c::LANGUAGE.into(), C_SYMBOLS)),
        "cpp" | "cc" | "cxx" | "hpp" | "hxx" | "hh" => {
            Some((tree_sitter_cpp::LANGUAGE.into(), CPP_SYMBOLS))
        }
        "rb" => Some((tree_sitter_ruby::LANGUAGE.into(), RUBY_SYMBOLS)),
        "php" => Some((tree_sitter_php::LANGUAGE_PHP.into(), PHP_SYMBOLS)),
        "sh" | "bash" => Some((tree_sitter_bash::LANGUAGE.into(), BASH_SYMBOLS)),
        _ => None,
    }
}

fn supported_extensions() -> Vec<&'static str> {
    vec![
        "rs", "py", "pyi", "ts", "tsx", "js", "mjs", "cjs", "jsx", "go", "java", "c", "h", "cpp",
        "cc", "cxx", "hpp", "hxx", "hh", "rb", "php", "sh", "bash",
    ]
}

struct Symbol {
    kind: &'static str,
    name: String,
    start_line: usize,
    end_line: usize,
    depth: usize,
}

/// Extract the name text from a definition node. Tries the `name` field first,
/// then `type` (Rust impl blocks), then descends into C/C++ declarator chains,
/// and finally falls back to the first identifier-like child.
fn extract_name(node: Node, source: &[u8]) -> Option<String> {
    for field in &["name", "type"] {
        if let Some(child) = node.child_by_field_name(field) {
            if let Ok(text) = child.utf8_text(source) {
                let text = text.trim();
                if !text.is_empty() {
                    return Some(text.to_string());
                }
            }
        }
    }

    // C/C++: declarator → (pointer_declarator|function_declarator) → declarator → identifier
    if let Some(decl) = node.child_by_field_name("declarator") {
        let mut current = decl;
        for _ in 0..5 {
            if let Ok(text) = current.utf8_text(source) {
                let t = text.trim();
                if !t.is_empty()
                    && t.chars()
                        .all(|c| c.is_alphanumeric() || c == '_' || c == ':')
                {
                    return Some(t.to_string());
                }
            }
            if let Some(next) = current.child_by_field_name("declarator") {
                current = next;
            } else {
                break;
            }
        }
    }

    // Fallback: first identifier-like child node.
    let name_kinds = [
        "identifier",
        "type_identifier",
        "property_identifier",
        "field_identifier",
        "constant",
        "scoped_identifier",
        "scoped_type_name",
    ];
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if name_kinds.contains(&child.kind()) {
            if let Ok(text) = child.utf8_text(source) {
                let t = text.trim();
                if !t.is_empty() {
                    return Some(t.to_string());
                }
            }
        }
    }

    None
}

/// Recursively walk the AST, collecting definition nodes.
fn walk_tree(
    node: Node,
    source: &[u8],
    kinds: SymbolKinds,
    depth: usize,
    out: &mut Vec<Symbol>,
    cap: usize,
) {
    if out.len() >= cap {
        return;
    }

    let symbol_entry = kinds.iter().find(|(k, _)| *k == node.kind());
    let is_symbol = symbol_entry.is_some();
    let child_depth = if is_symbol { depth + 1 } else { depth };

    if let Some(&(_node_kind, kind_label)) = symbol_entry {
        let name = extract_name(node, source).unwrap_or_default();
        out.push(Symbol {
            kind: kind_label,
            name,
            start_line: node.start_position().row + 1,
            end_line: node.end_position().row + 1,
            depth,
        });
    }

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        walk_tree(child, source, kinds, child_depth, out, cap);
        if out.len() >= cap {
            return;
        }
    }
}

/// Parse a single source file and return its symbol outline.
fn parse_file_symbols(
    source: &str,
    language: Language,
    kinds: SymbolKinds,
) -> Result<Vec<Symbol>, String> {
    let mut parser = Parser::new();
    parser
        .set_language(&language)
        .map_err(|e| format!("parser setup failed: {e}"))?;

    let tree = parser
        .parse(source, None)
        .ok_or_else(|| "parse returned None".to_string())?;

    let mut symbols = Vec::new();
    walk_tree(
        tree.root_node(),
        source.as_bytes(),
        kinds,
        0,
        &mut symbols,
        MAX_SYMBOLS_PER_FILE,
    );

    Ok(symbols)
}

/// Format a single file's symbols into a text outline.
fn format_symbols(rel_path: &str, line_count: usize, symbols: &[Symbol]) -> String {
    if symbols.is_empty() {
        return format!("{rel_path} ({line_count} lines, no symbols found)");
    }

    let mut out = format!(
        "{rel_path} ({line_count} lines, {} symbol{})\n",
        symbols.len(),
        if symbols.len() != 1 { "s" } else { "" }
    );

    for s in symbols {
        let indent = "  ".repeat(s.depth);
        let name_part = if s.name.is_empty() {
            String::new()
        } else {
            format!(" {}", s.name)
        };
        out.push_str(&format!(
            "{}{}{}  L{}-L{}\n",
            indent, s.kind, name_part, s.start_line, s.end_line
        ));
    }

    out
}

/// Directories never descended into (same set as grep/glob).
fn is_ignored_dir(name: &str) -> bool {
    matches!(
        name,
        ".git"
            | ".hg"
            | ".svn"
            | ".bzr"
            | "node_modules"
            | "target"
            | "dist"
            | "build"
            | "out"
            | "__pycache__"
            | ".venv"
            | "venv"
            | ".tox"
            | ".pytest_cache"
            | ".mypy_cache"
            | ".ruff_cache"
            | ".cache"
            | ".gradle"
            | ".idea"
            | ".next"
            | ".nuxt"
            | ".turbo"
            | ".svelte-kit"
            | ".parcel-cache"
    )
}

/// Collect source files from a directory, respecting .gitignore via the `ignore` crate.
fn collect_source_files(root: &Path) -> Vec<PathBuf> {
    let supported = supported_extensions();
    let mut files = Vec::new();

    let walker = ignore::WalkBuilder::new(root)
        .hidden(false)
        .git_ignore(true)
        .git_global(true)
        .git_exclude(true)
        .ignore(true)
        .parents(true)
        .build();

    for entry in walker.flatten() {
        if files.len() >= MAX_FILES {
            break;
        }
        let path = entry.path();
        if !path.is_file() {
            // Skip ignored directories during manual descent check.
            if path.is_dir() {
                if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                    if is_ignored_dir(name) {
                        // The ignore crate handles .gitignore, but we also
                        // manually skip known generated/cache dirs.
                    }
                }
            }
            continue;
        }
        let ext = match path.extension().and_then(|s| s.to_str()) {
            Some(e) => e,
            None => continue,
        };
        if !supported.contains(&ext) {
            continue;
        }
        files.push(path.to_path_buf());
    }

    files.sort();
    files
}

/// Process a single file: read, validate, parse, format.
fn process_file(abs: &Path, root: &Path, total_symbols: &mut usize) -> Option<String> {
    if *total_symbols >= MAX_SYMBOLS_TOTAL {
        return None;
    }

    let metadata = std::fs::metadata(abs).ok()?;
    if !metadata.is_file() {
        return None;
    }
    if metadata.len() > MAX_FILE_SIZE {
        // Compute relative path for the message.
        let rel = abs.strip_prefix(root).unwrap_or(abs).to_string_lossy();
        return Some(format!(
            "{} (skipped: file too large, {} bytes)\n",
            rel,
            metadata.len()
        ));
    }

    let bytes = match std::fs::read(abs) {
        Ok(b) => b,
        Err(_) => return None,
    };

    // Skip binary files (NUL byte heuristic).
    if bytes.contains(&0u8) {
        return None;
    }

    let source = match String::from_utf8(bytes) {
        Ok(s) => s,
        Err(_) => return None,
    };

    let ext = abs.extension().and_then(|s| s.to_str())?;
    let (language, kinds) = lang_for_ext(ext)?;

    let symbols = match parse_file_symbols(&source, language, kinds) {
        Ok(s) => s,
        Err(_) => return None,
    };

    *total_symbols += symbols.len();

    let rel = abs.strip_prefix(root).unwrap_or(abs).to_string_lossy();
    let line_count = source.lines().count();
    Some(format_symbols(&rel, line_count, &symbols))
}

/// `list_symbols` — extract a structural outline of symbols (functions, classes,
/// structs, etc.) from source files using tree-sitter.
///
/// Accepts a file path or a directory. For a directory, walks the tree
/// (respecting .gitignore), parses each supported source file, and returns a
/// combined outline. Supported languages: Rust, Python, TypeScript/TSX,
/// JavaScript, Go, Java, C, C++, Ruby, PHP, Bash.
pub struct ListSymbols;

#[async_trait]
impl Tool for ListSymbols {
    fn category(&self) -> ToolCategory {
        ToolCategory::Readonly
    }

    fn spec(&self) -> ToolSpec {
        let supported = supported_extensions()
            .iter()
            .map(|e| format!(".{e}"))
            .collect::<Vec<_>>()
            .join(", ");
        ToolSpec {
            name: "list_symbols".into(),
            description: format!(
                "Extract a structural outline of symbols (functions, classes, structs, methods, \
                 etc.) from a source file or directory using tree-sitter parsing. Returns symbol \
                 names with kind and line ranges (e.g. `fn parse  L10-L25`). For a directory, \
                 walks all supported source files (respecting .gitignore) and returns a combined \
                 outline — useful for understanding codebase structure without reading every file. \
                 Supported extensions: {supported}. Cap: ~500 symbols, files up to 512 KB. \
                 Symbols are nested (methods inside classes are indented). Use this before \
                 read_file on large files to find the right line range, or to get an overview of \
                 a directory."
            ),
            parameters: super::builtin::schema(&[("path", "string")], &["path"]),
        }
    }

    async fn execute(&self, args: Value) -> ToolResult {
        let Some(path) = super::builtin::arg_str(&args, "path") else {
            return ToolResult::err("missing 'path'");
        };

        let abs = match validate_read_path(path) {
            Ok(p) => p,
            Err(e) => return ToolResult::err(e),
        };

        let metadata = match tokio::fs::metadata(&abs).await {
            Ok(m) => m,
            Err(e) => return ToolResult::err(format!("failed to stat path: {e}")),
        };

        if metadata.is_file() {
            // Single-file mode.
            let ext = abs
                .extension()
                .and_then(|s| s.to_str())
                .unwrap_or("?")
                .to_string();
            let file_path = abs.clone();
            let result = tokio::task::spawn_blocking(move || {
                let mut total = 0usize;
                process_file(&file_path, &file_path, &mut total)
            })
            .await;

            match result {
                Ok(Some(out)) => ToolResult::ok(out),
                Ok(None) => {
                    if lang_for_ext(&ext).is_none() {
                        ToolResult::err(format!(
                            "unsupported file type: .{ext}. \
                             Supported: {}",
                            supported_extensions()
                                .iter()
                                .map(|e| format!(".{e}"))
                                .collect::<Vec<_>>()
                                .join(", ")
                        ))
                    } else {
                        ToolResult::ok("file has no symbols or could not be parsed")
                    }
                }
                Err(e) => ToolResult::err(format!("task failed: {e}")),
            }
        } else if metadata.is_dir() {
            // Directory mode.
            let root = abs.clone();
            let result = tokio::task::spawn_blocking(move || {
                let files = collect_source_files(&root);
                let mut total_symbols = 0usize;
                let mut output = String::new();
                let mut files_with_symbols = 0usize;

                for file_path in &files {
                    if total_symbols >= MAX_SYMBOLS_TOTAL {
                        output.push_str(&format!(
                            "\n…[stopped at {} symbols — narrow the path or use grep to find \
                             specific symbols]\n",
                            MAX_SYMBOLS_TOTAL
                        ));
                        break;
                    }
                    if let Some(file_out) = process_file(file_path, &root, &mut total_symbols) {
                        if !output.is_empty() {
                            output.push('\n');
                        }
                        output.push_str(&file_out);
                        files_with_symbols += 1;
                    }

                    if output.len() > MAX_OUTPUT {
                        output.truncate(MAX_OUTPUT);
                        output.push_str("\n…[output truncated]\n");
                        break;
                    }
                }

                if files_with_symbols == 0 {
                    String::from("no supported source files found in the given directory")
                } else {
                    format!(
                        "{} file{} parsed, {} symbol{}\n\n{}",
                        files_with_symbols,
                        if files_with_symbols != 1 { "s" } else { "" },
                        total_symbols,
                        if total_symbols != 1 { "s" } else { "" },
                        output
                    )
                }
            })
            .await;

            match result {
                Ok(out) => ToolResult::ok(out),
                Err(e) => ToolResult::err(format!("task failed: {e}")),
            }
        } else {
            ToolResult::err(format!(
                "path is not a regular file or directory: {}",
                abs.display()
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn symbols_text(source: &str, ext: &str) -> Vec<(String, usize, usize)> {
        let (language, kinds) = lang_for_ext(ext).expect("unsupported ext in test");
        let syms = parse_file_symbols(source, language, kinds).unwrap_or_default();
        syms.into_iter()
            .map(|s| {
                (
                    if s.name.is_empty() {
                        s.kind.to_string()
                    } else {
                        format!("{} {}", s.kind, s.name)
                    },
                    s.start_line,
                    s.end_line,
                )
            })
            .collect()
    }

    #[test]
    fn rust_basic_symbols() {
        let code = r#"
fn main() {
    let x = 42;
}

struct Config {
    name: String,
}

enum Mode {
    Read,
    Write,
}

impl Config {
    fn new() -> Self {
        Self { name: String::new() }
    }
}
"#;
        let syms = symbols_text(code, "rs");
        assert!(!syms.is_empty());
        assert!(syms.iter().any(|(n, _, _)| n == "fn main"));
        assert!(syms.iter().any(|(n, _, _)| n == "struct Config"));
        assert!(syms.iter().any(|(n, _, _)| n == "enum Mode"));
        assert!(syms.iter().any(|(n, _, _)| n == "impl Config"));
    }

    #[test]
    fn python_symbols() {
        let code = r#"
def hello():
    pass

class Foo:
    def bar(self):
        pass
"#;
        let syms = symbols_text(code, "py");
        assert!(syms.iter().any(|(n, _, _)| n == "def hello"));
        assert!(syms.iter().any(|(n, _, _)| n == "class Foo"));
        assert!(syms.iter().any(|(n, _, _)| n == "def bar"));
    }

    #[test]
    fn typescript_symbols() {
        let code = r#"
function add(a: number, b: number): number {
    return a + b;
}

interface Foo {
    bar(): void;
}

class Bar {
    method(): void {}
}

type Alias = string;
"#;
        let syms = symbols_text(code, "ts");
        assert!(syms.iter().any(|(n, _, _)| n == "function add"));
        assert!(syms.iter().any(|(n, _, _)| n == "interface Foo"));
        assert!(syms.iter().any(|(n, _, _)| n == "class Bar"));
        assert!(syms.iter().any(|(n, _, _)| n == "type Alias"));
    }

    #[test]
    fn go_symbols() {
        let code = r#"
package main

func main() {
    fmt.Println("hello")
}

func (s *Server) Start() {
    // ...
}

type Config struct {
    Port int
}
"#;
        let syms = symbols_text(code, "go");
        assert!(syms.iter().any(|(n, _, _)| n == "func main"));
        assert!(syms.iter().any(|(n, _, _)| n == "method Start"));
        assert!(syms.iter().any(|(n, _, _)| n == "type Config"));
    }

    #[test]
    fn unsupported_extension_returns_error() {
        let result = lang_for_ext("xyz");
        assert!(result.is_none());
    }

    #[test]
    fn nesting_depth() {
        let code = r#"
struct Foo {
    x: i32,
}

impl Foo {
    fn bar(&self) -> i32 {
        self.x
    }
}
"#;
        let (language, kinds) = lang_for_ext("rs").unwrap();
        let syms = parse_file_symbols(code, language, kinds).unwrap();
        let impl_sym = syms.iter().find(|s| s.kind == "impl").unwrap();
        assert_eq!(impl_sym.depth, 0);
        let method = syms.iter().find(|s| s.kind == "fn").unwrap();
        assert_eq!(method.depth, 1);
        assert_eq!(method.name, "bar");
    }

    #[test]
    fn format_output() {
        let syms = vec![
            Symbol {
                kind: "fn",
                name: "main".into(),
                start_line: 1,
                end_line: 5,
                depth: 0,
            },
            Symbol {
                kind: "struct",
                name: "Config".into(),
                start_line: 7,
                end_line: 10,
                depth: 0,
            },
            Symbol {
                kind: "fn",
                name: "new".into(),
                start_line: 12,
                end_line: 15,
                depth: 1,
            },
        ];
        let out = format_symbols("src/main.rs", 20, &syms);
        assert!(out.contains("src/main.rs (20 lines, 3 symbols)"));
        assert!(out.contains("fn main  L1-L5"));
        assert!(out.contains("struct Config  L7-L10"));
        assert!(out.contains("  fn new  L12-L15"));
    }
}
