//! Dependency graphs at file or module (directory) level, with fan-in/out and cycle detection.

use serde::Serialize;
use std::collections::{BTreeMap, HashMap, HashSet};

#[derive(Serialize, Debug, Clone)]
pub struct Node {
    pub id: String,
    pub label: String,
    /// "module", "file" or "external".
    pub kind: &'static str,
    pub files: u32,
    pub lines: u64,
    pub fan_in: u32,
    pub fan_out: u32,
    pub in_cycle: bool,
}

#[derive(Serialize, Debug, Clone, PartialEq)]
pub struct Edge {
    pub from: String,
    pub to: String,
    pub weight: u32,
}

#[derive(Serialize, Debug, Default)]
pub struct Graph {
    pub nodes: Vec<Node>,
    pub edges: Vec<Edge>,
    /// Groups of nodes that depend on each other in a loop.
    pub cycles: Vec<Vec<String>>,
    pub truncated: bool,
}

pub struct ImportRow {
    pub from: String,
    pub target: String,
    pub external: bool,
}

/// The module a file belongs to: its first `depth` directory segments.
pub fn module_of(path: &str, depth: usize) -> String {
    let parts: Vec<&str> = path.split('/').collect();
    let dirs = &parts[..parts.len() - 1];
    if dirs.is_empty() {
        return "(root)".to_string();
    }
    dirs[..dirs.len().min(depth.max(1))].join("/")
}

pub fn build(files: &[(String, u64)], imports: &[ImportRow], file_level: bool, depth: usize, externals: bool, max_nodes: usize) -> Graph {
    let key = |path: &str| if file_level { path.to_string() } else { module_of(path, depth) };
    let mut nodes: BTreeMap<String, Node> = BTreeMap::new();
    for (path, lines) in files {
        let id = key(path);
        let node = nodes.entry(id.clone()).or_insert_with(|| Node {
            label: if file_level { path.rsplit('/').next().unwrap_or(path).to_string() } else { id.clone() },
            id,
            kind: if file_level { "file" } else { "module" },
            files: 0,
            lines: 0,
            fan_in: 0,
            fan_out: 0,
            in_cycle: false,
        });
        node.files += 1;
        node.lines += lines;
    }
    let known: HashSet<&str> = files.iter().map(|(p, _)| p.as_str()).collect();
    let mut weights: BTreeMap<(String, String), u32> = BTreeMap::new();
    for imp in imports {
        if !known.contains(imp.from.as_str()) {
            continue;
        }
        let from = key(&imp.from);
        let to = if imp.external {
            if !externals {
                continue;
            }
            let id = format!("ext:{}", imp.target);
            nodes.entry(id.clone()).or_insert_with(|| Node {
                id: id.clone(),
                label: imp.target.clone(),
                kind: "external",
                files: 0,
                lines: 0,
                fan_in: 0,
                fan_out: 0,
                in_cycle: false,
            });
            id
        } else if known.contains(imp.target.as_str()) {
            key(&imp.target)
        } else {
            continue;
        };
        if from != to {
            *weights.entry((from, to)).or_insert(0) += 1;
        }
    }
    for (from, to) in weights.keys() {
        nodes.get_mut(from).unwrap().fan_out += 1;
        nodes.get_mut(to).unwrap().fan_in += 1;
    }

    // Keep the most connected nodes when the graph is too large to draw.
    let mut truncated = false;
    if nodes.len() > max_nodes {
        truncated = true;
        let mut ranked: Vec<(&String, u32)> = nodes.iter().map(|(id, n)| (id, n.fan_in + n.fan_out)).collect();
        ranked.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(b.0)));
        let keep: HashSet<String> = ranked.into_iter().take(max_nodes).map(|(id, _)| id.clone()).collect();
        nodes.retain(|id, _| keep.contains(id));
        weights.retain(|(f, t), _| keep.contains(f) && keep.contains(t));
    }

    let edges: Vec<Edge> = weights.into_iter().map(|((from, to), weight)| Edge { from, to, weight }).collect();
    let cycles = cycles(&nodes.keys().cloned().collect::<Vec<_>>(), &edges);
    for group in &cycles {
        for id in group {
            if let Some(n) = nodes.get_mut(id) {
                n.in_cycle = true;
            }
        }
    }
    Graph { nodes: nodes.into_values().collect(), edges, cycles, truncated }
}

/// Strongly connected components with more than one node (Tarjan, iterative).
fn cycles(ids: &[String], edges: &[Edge]) -> Vec<Vec<String>> {
    let index_of: HashMap<&str, usize> = ids.iter().enumerate().map(|(i, s)| (s.as_str(), i)).collect();
    let mut adj = vec![vec![]; ids.len()];
    for e in edges {
        if let (Some(&a), Some(&b)) = (index_of.get(e.from.as_str()), index_of.get(e.to.as_str())) {
            adj[a].push(b);
        }
    }
    let n = ids.len();
    let (mut index, mut low, mut on) = (vec![usize::MAX; n], vec![0; n], vec![false; n]);
    let (mut stack, mut counter, mut out) = (vec![], 0usize, vec![]);
    for start in 0..n {
        if index[start] != usize::MAX {
            continue;
        }
        let mut work = vec![(start, 0usize)];
        while let Some(&mut (v, ref mut next)) = work.last_mut() {
            if *next == 0 {
                index[v] = counter;
                low[v] = counter;
                counter += 1;
                stack.push(v);
                on[v] = true;
            }
            if *next < adj[v].len() {
                let w = adj[v][*next];
                *next += 1;
                if index[w] == usize::MAX {
                    work.push((w, 0));
                } else if on[w] {
                    low[v] = low[v].min(index[w]);
                }
            } else {
                work.pop();
                if let Some(&(parent, _)) = work.last() {
                    low[parent] = low[parent].min(low[v]);
                }
                if low[v] == index[v] {
                    let mut group = vec![];
                    while let Some(w) = stack.pop() {
                        on[w] = false;
                        group.push(ids[w].clone());
                        if w == v {
                            break;
                        }
                    }
                    if group.len() > 1 {
                        group.sort();
                        out.push(group);
                    }
                }
            }
        }
    }
    out.sort();
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn imp(from: &str, target: &str, external: bool) -> ImportRow {
        ImportRow { from: from.into(), target: target.into(), external }
    }
    fn files() -> Vec<(String, u64)> {
        ["src/app.ts", "src/ui/button.ts", "src/ui/card.ts", "src/lib/api.ts", "main.ts"].iter().map(|p| (p.to_string(), 10)).collect()
    }

    #[test]
    fn groups_files_into_modules() {
        assert_eq!(module_of("main.ts", 1), "(root)");
        assert_eq!(module_of("src/ui/button.ts", 1), "src");
        assert_eq!(module_of("src/ui/button.ts", 2), "src/ui");
        assert_eq!(module_of("src/app.ts", 2), "src");
    }

    #[test]
    fn builds_module_graph_with_weights_and_externals() {
        let imports = [
            imp("src/app.ts", "src/ui/button.ts", false),
            imp("src/app.ts", "src/ui/card.ts", false),
            imp("src/ui/card.ts", "src/ui/button.ts", false), // inside one module: no edge
            imp("src/ui/button.ts", "src/lib/api.ts", false),
            imp("src/app.ts", "react", true),
        ];
        let g = build(&files(), &imports, false, 2, true, 100);
        assert_eq!(
            g.edges,
            [
                Edge { from: "src".into(), to: "ext:react".into(), weight: 1 },
                Edge { from: "src".into(), to: "src/ui".into(), weight: 2 },
                Edge { from: "src/ui".into(), to: "src/lib".into(), weight: 1 },
            ]
        );
        let ui = g.nodes.iter().find(|n| n.id == "src/ui").unwrap();
        assert_eq!((ui.files, ui.lines, ui.fan_in, ui.fan_out), (2, 20, 1, 1));
        assert!(g.cycles.is_empty());
        assert!(build(&files(), &imports, false, 2, false, 100).nodes.iter().all(|n| n.kind != "external"));
    }

    #[test]
    fn detects_cycles_and_truncates() {
        let imports = [
            imp("src/app.ts", "src/lib/api.ts", false),
            imp("src/lib/api.ts", "src/ui/card.ts", false),
            imp("src/ui/card.ts", "src/app.ts", false),
            imp("main.ts", "src/app.ts", false),
        ];
        let g = build(&files(), &imports, true, 1, false, 100);
        assert_eq!(g.cycles, [vec!["src/app.ts".to_string(), "src/lib/api.ts".into(), "src/ui/card.ts".into()]]);
        assert!(g.nodes.iter().find(|n| n.id == "src/app.ts").unwrap().in_cycle);
        assert!(!g.nodes.iter().find(|n| n.id == "main.ts").unwrap().in_cycle);

        let small = build(&files(), &imports, true, 1, false, 3);
        assert!(small.truncated);
        assert_eq!(small.nodes.len(), 3);
        assert!(small.edges.iter().all(|e| small.nodes.iter().any(|n| n.id == e.from) && small.nodes.iter().any(|n| n.id == e.to)));
    }
}
