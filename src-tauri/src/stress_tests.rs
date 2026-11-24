//! Edge-case and load tests over synthetic projects. Every fixture lives in a fresh temp folder
//! and is deleted afterwards; nothing here touches a real project.
//!
//! The load test is ignored by default; run it with
//! `cargo test --release --lib stress_tests -- --ignored --nocapture`.

use crate::git::{git, status_of};
use crate::intel::build_index;
use crate::projects::analyze;
use crate::search::{search_project, SearchResult};
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;

fn scratch(label: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("nexus-{label}-{}", crate::db::new_id()));
    fs::create_dir_all(&dir).unwrap();
    crate::platform::paths::canonical(&dir).unwrap()
}

fn write(root: &Path, rel: &str, content: impl AsRef<[u8]>) {
    let p = root.join(rel);
    fs::create_dir_all(p.parent().unwrap()).unwrap();
    fs::write(p, content).unwrap();
}

fn search(root: &Path, pattern: &str) -> Value {
    let re = regex::Regex::new(pattern).unwrap();
    let mut out = SearchResult::default();
    search_project(&re, "p", "P", root, &mut out);
    serde_json::to_value(&out).unwrap()
}

/// Unusual names, deep nesting, links that loop or point outside, binary, empty, huge,
/// malformed and unreadable files: analysis, indexing and search finish, stay inside the
/// project and report what they skipped.
#[cfg(unix)]
#[test]
fn awkward_projects_are_handled() {
    use std::os::unix::fs::{symlink, PermissionsExt};
    let base = scratch("awkward");
    let root = base.join("proj with spaces ü");
    let outside = base.join("outside");
    write(&outside, "secret.rs", "pub fn OUTSIDE_MARKER() {}\n");

    let deep: String = (0..60).map(|i| format!("d{i}/")).collect();
    write(&root, &format!("{deep}deep.rs"), "pub fn deepest_function() {}\n");
    write(&root, "src/文件 ünï 😀.rs", "pub fn unicode_named() {}\n");
    write(&root, "src/main.rs", "mod a;\nfn main() { let s = \"unterminated;\n");
    write(
        &root,
        "src/bad_utf8.rs",
        [b"pub fn ok_before() {}\n".as_slice(), &[0xC3, 0x28, 0xFF, b'\n'], b"pub fn ok_after() {}\n"].concat(),
    );
    write(&root, "src/empty.rs", "");
    write(&root, "assets/blob.bin", [0u8, 1, 2, 3, 0, 255].repeat(1000));
    write(&root, "logs/huge.rs", "// filler line for size\n".repeat(120_000)); // ~2.9 MB, over the text limit
    write(&root, ".env", "API_KEY=not-a-real-key-just-a-fixture\n");
    write(&root, "locked/private.rs", "pub fn unreadable() {}\n");
    fs::set_permissions(root.join("locked/private.rs"), fs::Permissions::from_mode(0o000)).unwrap();
    symlink(base.join("does-not-exist"), root.join("broken-link.rs")).unwrap();
    symlink(&outside, root.join("outside-link")).unwrap();
    symlink(root.join("loop-b"), root.join("loop-a")).unwrap();
    symlink(root.join("loop-a"), root.join("loop-b")).unwrap();
    symlink(&root, root.join("src/self-link")).unwrap();

    let started = Instant::now();
    let analysis = analyze(&root);
    assert!(analysis.file_count >= 8, "{}", analysis.file_count);
    let index = build_index(&root, |_, _| {});
    let indexed: Vec<&str> = index.files.iter().map(|(p, _, _)| p.as_str()).collect();
    assert!(indexed.iter().any(|p| p.ends_with("deep.rs")), "deeply nested file indexed");
    assert!(indexed.iter().any(|p| p.contains("文件 ünï 😀")), "unicode name indexed: {indexed:?}");
    assert!(
        !indexed.iter().any(|p| p.contains("outside") || p.contains("self-link") || p.contains("loop-")),
        "links not followed: {indexed:?}"
    );
    assert!(!indexed.iter().any(|p| p.ends_with("huge.rs")), "oversized file skipped");
    let names: Vec<&str> = index.symbols.iter().map(|(_, s)| s.name.as_str()).collect();
    assert!(names.contains(&"ok_before") && names.contains(&"ok_after"), "invalid UTF-8 does not stop extraction: {names:?}");

    let found = search(&root, "OUTSIDE_MARKER");
    assert_eq!(found["matches"].as_array().unwrap().len(), 0, "search followed a link out of the project");
    let found = search(&root, "unicode_named|deepest_function");
    assert_eq!(found["matches"].as_array().unwrap().len(), 2, "{found}");
    // Search reads files up to 64 MB (more than the 1.5 MB preview limit), so the large file is
    // searched, line by line, rather than skipped.
    assert!(search(&root, "filler line for size")["matches"].as_array().unwrap().len() > 100);
    assert!(crate::projects::read_text(&root.join("assets/blob.bin")).is_none(), "binary is not read as text");
    assert!(started.elapsed().as_secs() < 30);

    fs::set_permissions(root.join("locked/private.rs"), fs::Permissions::from_mode(0o644)).unwrap();
    fs::remove_dir_all(&base).unwrap();
}

/// Repositories in unusual states give a status or a readable error, never a panic.
#[test]
fn unusual_repositories_report_their_state() {
    let dir = scratch("repos");
    let g = |root: &Path, args: &[&str]| git(root, args).unwrap();

    // Not a repository at all.
    let plain = dir.join("plain");
    write(&plain, "a.txt", "x");
    assert_eq!(serde_json::to_value(status_of(&plain).unwrap()).unwrap()["is_repo"], false);

    // A repository with no commits.
    let empty = dir.join("empty");
    write(&empty, "a.txt", "x");
    g(&empty, &["init", "-q", "-b", "main"]);
    let s = serde_json::to_value(status_of(&empty).unwrap()).unwrap();
    assert_eq!((s["is_repo"].as_bool(), s["has_commits"].as_bool()), (Some(true), Some(false)));

    // Detached HEAD.
    let detached = dir.join("detached");
    write(&detached, "a.txt", "x");
    g(&detached, &["init", "-q", "-b", "main"]);
    g(&detached, &["-c", "user.name=t", "-c", "user.email=t@example.invalid", "add", "."]);
    g(&detached, &["-c", "user.name=t", "-c", "user.email=t@example.invalid", "commit", "-q", "-m", "one"]);
    let head = g(&detached, &["rev-parse", "HEAD"]);
    g(&detached, &["checkout", "-q", "--detach", head.trim()]);
    let s = serde_json::to_value(status_of(&detached).unwrap()).unwrap();
    assert_eq!(s["has_commits"], true);
    assert_ne!(s["branch"], "main", "a detached HEAD is not reported as the branch: {s}");

    // A damaged repository: `.git` is a file with nonsense in it.
    let broken = dir.join("broken");
    write(&broken, ".git", "this is not a gitdir pointer\n");
    write(&broken, "a.txt", "x");
    match status_of(&broken) {
        Ok(s) => assert_eq!(serde_json::to_value(s).unwrap()["is_repo"], false),
        Err(e) => assert!(!e.to_string().is_empty()),
    }
    fs::remove_dir_all(&dir).unwrap();
}

/// Timings on a generated project of realistic shape. Prints a table; asserts only generous
/// upper bounds so it flags regressions by orders of magnitude, not machine noise.
#[test]
#[ignore]
fn load_baseline() {
    const DIRS: usize = 400;
    const FILES_PER_DIR: usize = 50;
    let root = scratch("load");
    for d in 0..DIRS {
        for f in 0..FILES_PER_DIR {
            let (ext, body) = if f % 2 == 0 {
                ("ts", format!("import {{ helper{f} }} from \"../m{}/f{}\";\nexport function feature_{d}_{f}(x: number): number {{\n  return helper{f}(x) + {f};\n}}\n// TODO tidy\n{}", (d + 1) % DIRS, (f + 2) % FILES_PER_DIR, "const filler = 1;\n".repeat(60)))
            } else {
                (
                    "rs",
                    format!(
                        "use crate::m{}::f{};\npub fn feature_{d}_{f}(x: u32) -> u32 {{ x + {f} }}\n{}",
                        (d + 3) % DIRS,
                        f,
                        "// filler\n".repeat(60)
                    ),
                )
            };
            write(&root, &format!("src/m{d}/f{f}.{ext}"), body);
        }
    }
    write(&root, "node_modules/big/index.js", "x".repeat(1000));
    let files = DIRS * FILES_PER_DIR;
    let mut rows: Vec<(String, f64)> = vec![];
    let mut time = |label: &str, f: &mut dyn FnMut()| {
        let t = Instant::now();
        f();
        rows.push((label.to_string(), t.elapsed().as_secs_f64() * 1000.0));
    };

    let mut analysis = None;
    time("analyse project", &mut || analysis = Some(analyze(&root)));
    assert_eq!(analysis.unwrap().file_count as usize, files, "node_modules skipped");
    let mut index = None;
    time("build code index", &mut || index = Some(build_index(&root, |_, _| {})));
    let index = index.unwrap();
    let db = crate::db::Db::in_memory().unwrap();
    db.conn().execute("INSERT INTO projects (id, name, path, added_at) VALUES ('p', 'P', ?1, 'x')", [root.to_string_lossy()]).unwrap();
    time("store code index", &mut || crate::intel::store_index(&mut db.conn(), "p", &index).unwrap());
    time("symbol search", &mut || {
        crate::intel::symbols_query(&db.conn(), Some("p"), "feature_123", None, 50).unwrap();
    });
    time("content search (literal)", &mut || {
        search(&root, "feature_399_49");
    });
    time("content search (regex)", &mut || {
        search(&root, r"helper\d+\(x\) \+ 4\d;");
    });
    git(&root, &["init", "-q", "-b", "main"]).unwrap();
    git(&root, &["add", "-A"]).unwrap();
    git(&root, &["-c", "user.name=t", "-c", "user.email=t@example.invalid", "commit", "-q", "-m", "load"]).unwrap();
    for d in 0..20 {
        write(&root, &format!("src/m{d}/f0.ts"), "changed\n");
    }
    time("git status (20 changed)", &mut || {
        status_of(&root).unwrap();
    });

    println!("\nLoad baseline: {files} source files, {} symbols, {} imports", index.symbols.len(), index.imports.len());
    for (label, ms) in &rows {
        println!("  {label:<28} {ms:>9.1} ms");
    }
    for (label, ms) in &rows {
        assert!(*ms < 60_000.0, "{label} took {ms} ms");
    }
    fs::remove_dir_all(&root).unwrap();
}
