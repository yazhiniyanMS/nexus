//! Language extractors: pull symbol definitions and import statements out of source text.
//! These are line-oriented and deliberately simple. They sit behind `extract()` so a real
//! parser can replace them without touching storage, commands or the UI.

use regex::Regex;
use std::sync::OnceLock;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Lang {
    Rust,
    Script, // TypeScript and JavaScript
    Python,
    Go,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Symbol {
    pub name: String,
    pub kind: &'static str,
    pub line: u32,
    pub signature: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RawImport {
    pub spec: String,
    pub line: u32,
}

pub fn lang_of(path: &str) -> Option<(Lang, &'static str)> {
    let ext = path.rsplit('.').next()?.to_lowercase();
    Some(match ext.as_str() {
        "rs" => (Lang::Rust, "Rust"),
        "ts" | "tsx" | "mts" | "cts" => (Lang::Script, "TypeScript"),
        "js" | "jsx" | "mjs" | "cjs" => (Lang::Script, "JavaScript"),
        "py" => (Lang::Python, "Python"),
        "go" => (Lang::Go, "Go"),
        _ => return None,
    })
}

macro_rules! re {
    ($pat:expr) => {{
        static R: OnceLock<Regex> = OnceLock::new();
        R.get_or_init(|| Regex::new($pat).unwrap())
    }};
}

fn line_of(text: &str, offset: usize) -> u32 {
    text.as_bytes()[..offset].iter().filter(|b| **b == b'\n').count() as u32 + 1
}

fn sig(line: &str) -> String {
    line.trim().trim_end_matches('{').trim_end().chars().take(160).collect()
}

pub fn extract(lang: Lang, text: &str) -> (Vec<Symbol>, Vec<RawImport>) {
    match lang {
        Lang::Rust => rust(text),
        Lang::Script => script(text),
        Lang::Python => python(text),
        Lang::Go => go(text),
    }
}

fn rust(text: &str) -> (Vec<Symbol>, Vec<RawImport>) {
    let def = re!(
        r#"^\s*(?:pub(?:\([^)]*\))?\s+)?(?:(?:async|const|unsafe|default)\s+|extern\s+"[^"]*"\s+)*(fn|struct|enum|trait|type|const|static|mod|union)\s+(?:mut\s+)?([A-Za-z_][A-Za-z0-9_]*)"#
    );
    let mac = re!(r"^\s*macro_rules!\s+([A-Za-z_][A-Za-z0-9_]*)");
    let modline = re!(r"^\s*(?:pub(?:\([^)]*\))?\s+)?mod\s+([A-Za-z_][A-Za-z0-9_]*)\s*;");
    let mut symbols = vec![];
    let mut imports = vec![];
    for (i, line) in text.lines().enumerate() {
        let n = i as u32 + 1;
        if line.trim_start().starts_with("//") {
            continue;
        }
        if let Some(c) = modline.captures(line) {
            // `mod x;` points at another file; it is an import, not a definition here.
            imports.push(RawImport { spec: format!("mod:{}", &c[1]), line: n });
        } else if let Some(c) = def.captures(line) {
            let kind = match &c[1] {
                "fn" => "function",
                "struct" | "union" => "struct",
                "enum" => "enum",
                "trait" => "trait",
                "type" => "type",
                "const" | "static" => "constant",
                _ => "module",
            };
            symbols.push(Symbol { name: c[2].to_string(), kind, line: n, signature: sig(line) });
        } else if let Some(c) = mac.captures(line) {
            symbols.push(Symbol { name: c[1].to_string(), kind: "macro", line: n, signature: sig(line) });
        }
    }
    // `use` statements can span lines, so match them against the whole text.
    let use_stmt = re!(r"(?m)^\s*(?:pub(?:\([^)]*\))?\s+)?use\s+([^;]+);");
    for c in use_stmt.captures_iter(text) {
        let line = line_of(text, c.get(1).unwrap().start());
        for spec in expand_use(&c[1]) {
            imports.push(RawImport { spec, line });
        }
    }
    (symbols, imports)
}

/// `crate::{a, b::C}` -> ["crate::a", "crate::b::C"]. Only the first brace level is expanded.
fn expand_use(body: &str) -> Vec<String> {
    let body: String = body.split_whitespace().collect::<Vec<_>>().join(" ");
    let clean = |s: &str| s.split(" as ").next().unwrap_or("").trim().trim_end_matches("::*").trim_end_matches("::").to_string();
    let Some(open) = body.find('{') else { return vec![clean(&body)] };
    let prefix = body[..open].trim().to_string();
    let inner = &body[open + 1..body.rfind('}').unwrap_or(body.len())];
    let mut items = vec![];
    let (mut depth, mut start) = (0i32, 0usize);
    for (i, ch) in inner.char_indices() {
        match ch {
            '{' => depth += 1,
            '}' => depth -= 1,
            ',' if depth == 0 => {
                items.push(&inner[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    items.push(&inner[start..]);
    items
        .into_iter()
        .map(|it| it.split('{').next().unwrap_or("").trim())
        .filter(|it| !it.is_empty())
        .map(|it| if it == "self" { clean(&prefix) } else { clean(&format!("{prefix}{it}")) })
        .collect()
}

fn script(text: &str) -> (Vec<Symbol>, Vec<RawImport>) {
    let func = re!(r"^\s*(?:export\s+)?(?:default\s+)?(?:async\s+)?function\s*\*?\s*([A-Za-z_$][\w$]*)");
    let class = re!(r"^\s*(?:export\s+)?(?:default\s+)?(?:abstract\s+)?class\s+([A-Za-z_$][\w$]*)");
    let typ = re!(r"^\s*(?:export\s+)?(?:declare\s+)?(interface|type|enum)\s+([A-Za-z_$][\w$]*)");
    let arrow = re!(
        r"^(?:export\s+)?(?:const|let|var)\s+([A-Za-z_$][\w$]*)\s*(?::[^=]+)?=\s*(?:async\s+)?(?:<[^>]*>\s*)?(?:\([^)]*\)|[A-Za-z_$][\w$]*)\s*(?::[^=]+?)?=>"
    );
    let konst = re!(r"^export\s+(?:const|let|var)\s+([A-Za-z_$][\w$]*)");
    let mut symbols = vec![];
    for (i, line) in text.lines().enumerate() {
        let n = i as u32 + 1;
        let mut add = |name: &str, kind: &'static str| symbols.push(Symbol { name: name.to_string(), kind, line: n, signature: sig(line) });
        if let Some(c) = func.captures(line) {
            add(&c[1], "function");
        } else if let Some(c) = class.captures(line) {
            add(&c[1], "class");
        } else if let Some(c) = typ.captures(line) {
            add(
                &c[2],
                match &c[1] {
                    "interface" => "interface",
                    "enum" => "enum",
                    _ => "type",
                },
            );
        } else if let Some(c) = arrow.captures(line) {
            add(&c[1], "function");
        } else if let Some(c) = konst.captures(line) {
            add(&c[1], "constant");
        }
    }
    let mut imports = vec![];
    let patterns = [
        re!(r#"(?m)^\s*(?:import|export)\s+(?:[^'";]*?\s+from\s+)?['"]([^'"\n]+)['"]"#),
        re!(r#"\brequire\(\s*['"]([^'"\n]+)['"]\s*\)"#),
        re!(r#"\bimport\(\s*['"]([^'"\n]+)['"]\s*\)"#),
    ];
    for p in patterns {
        for c in p.captures_iter(text) {
            let m = c.get(1).unwrap();
            imports.push(RawImport { spec: m.as_str().to_string(), line: line_of(text, m.start()) });
        }
    }
    imports.sort_by_key(|i| i.line);
    (symbols, imports)
}

fn python(text: &str) -> (Vec<Symbol>, Vec<RawImport>) {
    let def = re!(r"^(\s*)(?:async\s+)?def\s+([A-Za-z_]\w*)");
    let class = re!(r"^(\s*)class\s+([A-Za-z_]\w*)");
    let konst = re!(r"^([A-Z][A-Z0-9_]{2,})\s*(?::[^=]+)?=");
    let import = re!(r"^\s*import\s+(.+)$");
    let from = re!(r"^\s*from\s+(\.*[\w.]*)\s+import\s+(.+)$");
    let mut symbols = vec![];
    let mut imports = vec![];
    for (i, line) in text.lines().enumerate() {
        let n = i as u32 + 1;
        if let Some(c) = def.captures(line) {
            let kind = if c[1].is_empty() { "function" } else { "method" };
            symbols.push(Symbol { name: c[2].to_string(), kind, line: n, signature: sig(line).trim_end_matches(':').to_string() });
        } else if let Some(c) = class.captures(line) {
            symbols.push(Symbol { name: c[2].to_string(), kind: "class", line: n, signature: sig(line).trim_end_matches(':').to_string() });
        } else if let Some(c) = konst.captures(line) {
            symbols.push(Symbol { name: c[1].to_string(), kind: "constant", line: n, signature: sig(line) });
        } else if let Some(c) = from.captures(line) {
            let module = &c[1];
            if module.chars().all(|ch| ch == '.') {
                // `from . import a, b` names sibling modules.
                for item in c[2].trim_matches(|ch| ch == '(' || ch == ')').split(',') {
                    let item = item.split(" as ").next().unwrap_or("").trim();
                    if !item.is_empty() && item != "*" {
                        imports.push(RawImport { spec: format!("{module}{item}"), line: n });
                    }
                }
            } else {
                imports.push(RawImport { spec: module.to_string(), line: n });
            }
        } else if let Some(c) = import.captures(line) {
            for item in c[1].split('#').next().unwrap_or("").split(',') {
                let item = item.split(" as ").next().unwrap_or("").trim();
                if !item.is_empty() {
                    imports.push(RawImport { spec: item.to_string(), line: n });
                }
            }
        }
    }
    (symbols, imports)
}

fn go(text: &str) -> (Vec<Symbol>, Vec<RawImport>) {
    let func = re!(r"^func\s+(?:\([^)]*\)\s*)?([A-Za-z_]\w*)");
    let typ = re!(r"^type\s+([A-Za-z_]\w*)\s+(struct|interface)?");
    let konst = re!(r"^(?:const|var)\s+([A-Za-z_]\w*)");
    let mut symbols = vec![];
    for (i, line) in text.lines().enumerate() {
        let n = i as u32 + 1;
        if let Some(c) = func.captures(line) {
            let kind = if line.trim_start().starts_with("func (") { "method" } else { "function" };
            symbols.push(Symbol { name: c[1].to_string(), kind, line: n, signature: sig(line) });
        } else if let Some(c) = typ.captures(line) {
            let kind = match c.get(2).map(|m| m.as_str()) {
                Some("struct") => "struct",
                Some("interface") => "interface",
                _ => "type",
            };
            symbols.push(Symbol { name: c[1].to_string(), kind, line: n, signature: sig(line) });
        } else if let Some(c) = konst.captures(line) {
            symbols.push(Symbol { name: c[1].to_string(), kind: "constant", line: n, signature: sig(line) });
        }
    }
    let mut imports = vec![];
    let single = re!(r#"(?m)^import\s+(?:[\w.]+\s+)?"([^"]+)""#);
    let block = re!(r"(?s)\bimport\s*\((.*?)\)");
    let quoted = re!(r#""([^"]+)""#);
    for c in single.captures_iter(text) {
        let m = c.get(1).unwrap();
        imports.push(RawImport { spec: m.as_str().to_string(), line: line_of(text, m.start()) });
    }
    for b in block.captures_iter(text) {
        let body = b.get(1).unwrap();
        for c in quoted.captures_iter(body.as_str()) {
            let m = c.get(1).unwrap();
            imports.push(RawImport { spec: m.as_str().to_string(), line: line_of(text, body.start() + m.start()) });
        }
    }
    (symbols, imports)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(s: &[Symbol]) -> Vec<(&str, &str)> {
        s.iter().map(|x| (x.name.as_str(), x.kind)).collect()
    }
    fn specs(i: &[RawImport]) -> Vec<&str> {
        i.iter().map(|x| x.spec.as_str()).collect()
    }

    #[test]
    fn rust_symbols_and_imports() {
        let (s, i) = extract(Lang::Rust, "mod db;\npub mod tasks;\nuse crate::error::{AppError, Result};\nuse crate::{activity, db::Db};\nuse std::fs;\nuse super::thing as t;\n\npub struct Engine {\n}\npub(crate) async fn run_task() {}\nconst MAX: usize = 1;\npub const fn tiny() {}\nstatic mut COUNT: u8 = 0;\nenum E { A }\ntrait T {}\n// fn commented() {}\nmacro_rules! re { () => {} }\n");
        assert_eq!(
            names(&s),
            [
                ("Engine", "struct"),
                ("run_task", "function"),
                ("MAX", "constant"),
                ("tiny", "function"),
                ("COUNT", "constant"),
                ("E", "enum"),
                ("T", "trait"),
                ("re", "macro")
            ]
        );
        assert_eq!(s[1].line, 10);
        assert_eq!(
            specs(&i),
            [
                "mod:db",
                "mod:tasks",
                "crate::error::AppError",
                "crate::error::Result",
                "crate::activity",
                "crate::db::Db",
                "std::fs",
                "super::thing"
            ]
        );
    }

    #[test]
    fn script_symbols_and_imports() {
        let (s, i) = extract(Lang::Script, "import React, { useState } from \"react\";\nimport {\n  a,\n  b,\n} from './lib/api';\nimport './index.css';\nexport * from \"../shared\";\nconst x = require('node:fs');\nconst lazy = () => import('./views/Projects');\n\nexport default function App() {}\nexport const useAsync = <T,>(fn: () => T) => fn;\nconst helper = async (a: number): Promise<void> => {};\nexport interface Workspace {}\nexport type ViewId = 'a';\nclass Store {}\nexport const TASK_CHANGED = 'x';\n  function nested() {}\n");
        assert_eq!(
            names(&s),
            [
                ("lazy", "function"),
                ("App", "function"),
                ("useAsync", "function"),
                ("helper", "function"),
                ("Workspace", "interface"),
                ("ViewId", "type"),
                ("Store", "class"),
                ("TASK_CHANGED", "constant"),
                ("nested", "function")
            ]
        );
        assert_eq!(specs(&i), ["react", "./lib/api", "./index.css", "../shared", "node:fs", "./views/Projects"]);
        assert_eq!(i[1].line, 5);
    }

    #[test]
    fn python_symbols_and_imports() {
        let (s, i) = extract(Lang::Python, "import os, sys as system\nfrom . import models, utils\nfrom .config import Settings\nfrom app.core.db import connect\nMAX_RETRIES = 3\n\nclass Agent(Base):\n    async def run(self):\n        pass\n\ndef main():\n    pass\n");
        assert_eq!(names(&s), [("MAX_RETRIES", "constant"), ("Agent", "class"), ("run", "method"), ("main", "function")]);
        assert_eq!(specs(&i), ["os", "sys", ".models", ".utils", ".config", "app.core.db"]);
    }

    #[test]
    fn go_symbols_and_imports() {
        let (s, i) = extract(Lang::Go, "package main\n\nimport \"fmt\"\nimport (\n\t\"os\"\n\tapi \"example.com/app/internal/api\"\n)\n\ntype Server struct {}\ntype Handler interface {}\nfunc (s *Server) Start() {}\nfunc main() {}\nconst Version = \"1\"\n");
        assert_eq!(
            names(&s),
            [("Server", "struct"), ("Handler", "interface"), ("Start", "method"), ("main", "function"), ("Version", "constant")]
        );
        assert_eq!(specs(&i), ["fmt", "os", "example.com/app/internal/api"]);
    }
}
