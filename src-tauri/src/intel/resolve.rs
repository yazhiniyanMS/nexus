//! Turn a raw import string into a file inside the project, an external package, or nothing.

use super::extract::Lang;
use std::collections::BTreeSet;

#[derive(Debug, PartialEq)]
pub enum Target {
    Internal(String),
    External(String),
    Ignored,
}

pub struct Resolver<'a> {
    pub files: &'a BTreeSet<String>,
    /// Go module path from go.mod, when present.
    pub go_module: Option<String>,
}

const PY_STDLIB: &[&str] = &[
    "__future__",
    "abc",
    "argparse",
    "ast",
    "asyncio",
    "base64",
    "bisect",
    "collections",
    "concurrent",
    "contextlib",
    "copy",
    "csv",
    "dataclasses",
    "datetime",
    "decimal",
    "email",
    "enum",
    "fractions",
    "functools",
    "glob",
    "gzip",
    "hashlib",
    "heapq",
    "html",
    "http",
    "importlib",
    "inspect",
    "io",
    "itertools",
    "json",
    "logging",
    "math",
    "multiprocessing",
    "operator",
    "os",
    "pathlib",
    "pickle",
    "platform",
    "queue",
    "random",
    "re",
    "secrets",
    "shlex",
    "shutil",
    "signal",
    "socket",
    "sqlite3",
    "statistics",
    "string",
    "struct",
    "subprocess",
    "sys",
    "tarfile",
    "tempfile",
    "textwrap",
    "threading",
    "time",
    "traceback",
    "types",
    "typing",
    "unittest",
    "urllib",
    "uuid",
    "warnings",
    "xml",
    "zipfile",
];

fn dir_of(path: &str) -> &str {
    path.rfind('/').map(|i| &path[..i]).unwrap_or("")
}

/// Join and normalise `.` / `..` segments. Returns None if the path leaves the project.
fn join(base: &str, rel: &str) -> Option<String> {
    let mut parts: Vec<&str> = base.split('/').filter(|s| !s.is_empty()).collect();
    for seg in rel.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            s => parts.push(s),
        }
    }
    Some(parts.join("/"))
}

impl Resolver<'_> {
    fn has(&self, p: &str) -> bool {
        self.files.contains(p)
    }

    pub fn resolve(&self, lang: Lang, from: &str, spec: &str) -> Target {
        match lang {
            Lang::Script => self.script(from, spec),
            Lang::Rust => self.rust(from, spec),
            Lang::Python => self.python(from, spec),
            Lang::Go => self.go(spec),
        }
    }

    fn script(&self, from: &str, spec: &str) -> Target {
        let base = if spec.starts_with('.') {
            join(dir_of(from), spec)
        } else if let Some(rest) = spec.strip_prefix("@/") {
            Some(format!("src/{rest}"))
        } else {
            let mut parts = spec.split('/');
            let first = parts.next().unwrap_or(spec);
            let name = if first.starts_with('@') { format!("{first}/{}", parts.next().unwrap_or("")) } else { first.to_string() };
            return Target::External(name);
        };
        let Some(base) = base else { return Target::Ignored };
        const EXTS: &[&str] = &["ts", "tsx", "js", "jsx", "mjs", "cjs", "d.ts"];
        if self.has(&base) {
            return Target::Internal(base);
        }
        // `./x.js` in TypeScript sources refers to `./x.ts`.
        let stem = base.strip_suffix(".js").or(base.strip_suffix(".mjs")).unwrap_or(&base);
        for ext in EXTS {
            for candidate in [format!("{stem}.{ext}"), format!("{base}/index.{ext}")] {
                if self.has(&candidate) {
                    return Target::Internal(candidate);
                }
            }
        }
        Target::Ignored
    }

    /// Longest prefix of `segs` under `root` that names a module file.
    fn rust_module(&self, root: &str, segs: &[&str]) -> Target {
        for end in (1..=segs.len()).rev() {
            let Some(base) = join(root, &segs[..end].join("/")) else { continue };
            for candidate in [format!("{base}.rs"), format!("{base}/mod.rs")] {
                if self.has(&candidate) {
                    return Target::Internal(candidate);
                }
            }
        }
        Target::Ignored
    }

    /// Directory that holds a Rust file's child modules.
    fn rust_child_dir(from: &str) -> String {
        let dir = dir_of(from);
        let file = from.rsplit('/').next().unwrap_or(from);
        if matches!(file, "mod.rs" | "lib.rs" | "main.rs") {
            dir.to_string()
        } else {
            join(dir, file.trim_end_matches(".rs")).unwrap_or_default()
        }
    }

    fn rust(&self, from: &str, spec: &str) -> Target {
        if let Some(name) = spec.strip_prefix("mod:") {
            return self.rust_module(&Self::rust_child_dir(from), &[name]);
        }
        let segs: Vec<&str> = spec.split("::").map(str::trim).filter(|s| !s.is_empty()).collect();
        match segs.first().copied() {
            Some("crate") => {
                // The crate root is the nearest ancestor directory holding lib.rs or main.rs.
                let mut dir = dir_of(from).to_string();
                loop {
                    let prefix = if dir.is_empty() { String::new() } else { format!("{dir}/") };
                    let root_file = [format!("{prefix}lib.rs"), format!("{prefix}main.rs")].into_iter().find(|f| self.has(f));
                    if let Some(root_file) = root_file {
                        // An item with no module file of its own lives in the crate root.
                        return match self.rust_module(&dir, &segs[1..]) {
                            Target::Ignored if segs.len() > 1 => Target::Internal(root_file),
                            found => found,
                        };
                    }
                    if dir.is_empty() {
                        return Target::Ignored;
                    }
                    dir = dir_of(&dir).to_string();
                }
            }
            Some("super") => {
                let own = Self::rust_child_dir(from);
                let ups = segs.iter().take_while(|s| **s == "super").count();
                let mut dir = own;
                for _ in 0..ups {
                    dir = dir_of(&dir).to_string();
                }
                self.rust_module(&dir, &segs[ups..])
            }
            Some("self") => self.rust_module(&Self::rust_child_dir(from), &segs[1..]),
            Some("std" | "core" | "alloc") | None => Target::Ignored,
            // `use sibling::Item` names a child module of the current file before it names a crate.
            Some(krate) => match self.rust_module(&Self::rust_child_dir(from), &segs[..1]) {
                Target::Ignored => Target::External(krate.to_string()),
                local => local,
            },
        }
    }

    fn python(&self, from: &str, spec: &str) -> Target {
        let dots = spec.chars().take_while(|c| *c == '.').count();
        let segs: Vec<&str> = spec[dots..].split('.').filter(|s| !s.is_empty()).collect();
        let try_under = |root: &str| -> Option<String> {
            for end in (1..=segs.len()).rev() {
                let base = join(root, &segs[..end].join("/"))?;
                for candidate in [format!("{base}.py"), format!("{base}/__init__.py")] {
                    if self.has(&candidate) {
                        return Some(candidate);
                    }
                }
            }
            None
        };
        if dots > 0 {
            let mut dir = dir_of(from).to_string();
            for _ in 1..dots {
                dir = dir_of(&dir).to_string();
            }
            return try_under(&dir).map(Target::Internal).unwrap_or(Target::Ignored);
        }
        // Absolute import: try the project root, `src/`, and every ancestor of the importing file.
        let mut roots = vec![String::new(), "src".to_string()];
        let mut dir = dir_of(from).to_string();
        while !dir.is_empty() {
            roots.push(dir.clone());
            dir = dir_of(&dir).to_string();
        }
        for root in roots {
            if let Some(found) = try_under(&root) {
                return Target::Internal(found);
            }
        }
        match segs.first() {
            Some(first) if !PY_STDLIB.contains(first) => Target::External(first.to_string()),
            _ => Target::Ignored,
        }
    }

    fn go(&self, spec: &str) -> Target {
        if let Some(module) = &self.go_module {
            if let Some(rest) = spec.strip_prefix(module.as_str()) {
                let dir = rest.trim_start_matches('/');
                let prefix = if dir.is_empty() { String::new() } else { format!("{dir}/") };
                // A Go import names a package directory; link to its first non-test source file.
                let found = self
                    .files
                    .range(prefix.clone()..)
                    .take_while(|f| f.starts_with(&prefix))
                    .find(|f| f.ends_with(".go") && !f.ends_with("_test.go") && !f[prefix.len()..].contains('/'));
                return found.map(|f| Target::Internal(f.clone())).unwrap_or(Target::Ignored);
            }
        }
        if spec.split('/').next().is_some_and(|first| first.contains('.')) {
            Target::External(spec.split('/').take(3).collect::<Vec<_>>().join("/"))
        } else {
            Target::Ignored // standard library
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn resolver(files: &[&str]) -> (BTreeSet<String>, Option<String>) {
        (files.iter().map(|s| s.to_string()).collect(), Some("example.com/app".to_string()))
    }
    fn int(s: &str) -> Target {
        Target::Internal(s.into())
    }
    fn ext(s: &str) -> Target {
        Target::External(s.into())
    }

    #[test]
    fn resolves_script_imports() {
        let (files, m) = resolver(&["src/App.tsx", "src/lib/api.ts", "src/views/index.ts", "src/util.js", "src/index.css"]);
        let r = Resolver { files: &files, go_module: m };
        let go = |spec| r.resolve(Lang::Script, "src/App.tsx", spec);
        assert_eq!(go("./lib/api"), int("src/lib/api.ts"));
        assert_eq!(go("./lib/api.js"), int("src/lib/api.ts"));
        assert_eq!(go("./views"), int("src/views/index.ts"));
        assert_eq!(go("./index.css"), int("src/index.css"));
        assert_eq!(go("@/util"), int("src/util.js"));
        assert_eq!(go("react"), ext("react"));
        assert_eq!(go("@tauri-apps/api/core"), ext("@tauri-apps/api"));
        assert_eq!(go("./missing"), Target::Ignored);
        assert_eq!(go("../../../etc/passwd"), Target::Ignored);
    }

    #[test]
    fn resolves_rust_imports() {
        let (files, m) = resolver(&[
            "src-tauri/src/lib.rs",
            "src-tauri/src/db.rs",
            "src-tauri/src/intel/mod.rs",
            "src-tauri/src/intel/graph.rs",
            "src-tauri/src/tasks.rs",
        ]);
        let r = Resolver { files: &files, go_module: m };
        assert_eq!(r.resolve(Lang::Rust, "src-tauri/src/lib.rs", "mod:db"), int("src-tauri/src/db.rs"));
        assert_eq!(r.resolve(Lang::Rust, "src-tauri/src/lib.rs", "mod:intel"), int("src-tauri/src/intel/mod.rs"));
        assert_eq!(r.resolve(Lang::Rust, "src-tauri/src/intel/mod.rs", "mod:graph"), int("src-tauri/src/intel/graph.rs"));
        assert_eq!(r.resolve(Lang::Rust, "src-tauri/src/intel/graph.rs", "crate::db::Db"), int("src-tauri/src/db.rs"));
        assert_eq!(r.resolve(Lang::Rust, "src-tauri/src/tasks.rs", "crate::intel::graph::Graph"), int("src-tauri/src/intel/graph.rs"));
        assert_eq!(r.resolve(Lang::Rust, "src-tauri/src/intel/graph.rs", "super::extract"), Target::Ignored);
        assert_eq!(r.resolve(Lang::Rust, "src-tauri/src/intel/graph.rs", "super::super::db"), int("src-tauri/src/db.rs"));
        assert_eq!(r.resolve(Lang::Rust, "src-tauri/src/db.rs", "serde::Serialize"), ext("serde"));
        assert_eq!(r.resolve(Lang::Rust, "src-tauri/src/intel/mod.rs", "graph::Graph"), int("src-tauri/src/intel/graph.rs"));
        assert_eq!(r.resolve(Lang::Rust, "src-tauri/src/db.rs", "std::fs"), Target::Ignored);
    }

    #[test]
    fn resolves_python_and_go_imports() {
        let (files, m) = resolver(&[
            "app/__init__.py",
            "app/core/db.py",
            "app/agent.py",
            "app/models.py",
            "internal/api/handler.go",
            "internal/api/handler_test.go",
            "main.go",
        ]);
        let r = Resolver { files: &files, go_module: m };
        assert_eq!(r.resolve(Lang::Python, "app/agent.py", ".models"), int("app/models.py"));
        assert_eq!(r.resolve(Lang::Python, "app/agent.py", "app.core.db"), int("app/core/db.py"));
        assert_eq!(r.resolve(Lang::Python, "app/core/db.py", "..models"), int("app/models.py"));
        assert_eq!(r.resolve(Lang::Python, "app/agent.py", "requests.adapters"), ext("requests"));
        assert_eq!(r.resolve(Lang::Python, "app/agent.py", "os.path"), Target::Ignored);
        assert_eq!(r.resolve(Lang::Go, "main.go", "example.com/app/internal/api"), int("internal/api/handler.go"));
        assert_eq!(r.resolve(Lang::Go, "main.go", "github.com/gin-gonic/gin/binding"), ext("github.com/gin-gonic/gin"));
        assert_eq!(r.resolve(Lang::Go, "main.go", "net/http"), Target::Ignored);
    }
}
