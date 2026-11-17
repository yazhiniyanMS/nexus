//! Keyword retrieval of source snippets for a question. Needs no AI: the snippets are useful
//! on their own, and become the numbered sources an AI answer must cite.

use serde::Serialize;

#[derive(Serialize, Debug, Clone)]
pub struct Source {
    /// 1-based citation number.
    pub n: usize,
    pub path: String,
    pub start_line: u32,
    pub end_line: u32,
    pub text: String,
    /// Why this snippet was chosen, for display.
    pub reason: String,
}

pub struct IndexedFile {
    pub path: String,
    pub text: String,
    /// (name, line) of symbols defined in the file.
    pub symbols: Vec<(String, u32)>,
}

const STOPWORDS: &[&str] = &[
    "the", "and", "for", "with", "that", "this", "what", "where", "when", "which", "how", "does", "are", "from", "into", "there", "have",
    "has", "can", "any", "all", "about", "why", "who", "use", "used", "using", "work", "works", "before", "after", "code", "file", "files",
    "project", "function", "class", "explain", "show", "tell", "find", "get",
];
const MAX_SOURCES: usize = 8;
const WINDOW_BEFORE: usize = 4;
const WINDOW_LINES: usize = 40;
const BUDGET_CHARS: usize = 28_000;

pub fn keywords(question: &str) -> Vec<String> {
    let mut out: Vec<String> = vec![];
    for raw in question.split(|c: char| !(c.is_alphanumeric() || c == '_')) {
        let word = raw.to_lowercase();
        if word.len() >= 3 && !STOPWORDS.contains(&word.as_str()) && !out.contains(&word) {
            out.push(word);
        }
    }
    out
}

/// Rank files against the question and cut the densest passage from each of the best.
///
/// Keywords are weighted by rarity (a word found in few files says more than one found in most),
/// and the snippet is the window of lines containing the most keyword weight, so the passage
/// shown is where the topic is actually discussed, not just the top of the file.
pub fn retrieve(question: &str, files: &[IndexedFile], focus: Option<&str>, related: &[(String, &str)]) -> Vec<Source> {
    let kws = keywords(question);
    // Pass 1: where does each keyword occur?
    struct Hits {
        /// Per keyword: lines (0-based) mentioning it.
        lines: Vec<Vec<usize>>,
        /// Per keyword: best symbol match (weight, line, name).
        symbol: Vec<Option<(f64, u32, String)>>,
        path: Vec<bool>,
    }
    let mut all: Vec<Hits> = Vec::with_capacity(files.len());
    let mut df = vec![0usize; kws.len()];
    for f in files {
        let lower_path = f.path.to_lowercase();
        let mut h = Hits { lines: vec![vec![]; kws.len()], symbol: vec![None; kws.len()], path: vec![false; kws.len()] };
        for (i, line) in f.text.lines().enumerate() {
            if line.len() > 2000 {
                continue;
            }
            let lower = line.to_lowercase();
            for (k, kw) in kws.iter().enumerate() {
                if lower.contains(kw.as_str()) {
                    h.lines[k].push(i);
                }
            }
        }
        for (k, kw) in kws.iter().enumerate() {
            h.path[k] = lower_path.contains(kw.as_str());
            for (name, line) in &f.symbols {
                let lname = name.to_lowercase();
                let w = if lname == *kw {
                    12.0
                } else if lname.contains(kw.as_str()) {
                    5.0
                } else {
                    0.0
                };
                if w > h.symbol[k].as_ref().map(|s| s.0).unwrap_or(0.0) {
                    h.symbol[k] = Some((w, *line, name.clone()));
                }
            }
            if !h.lines[k].is_empty() || h.symbol[k].is_some() || h.path[k] {
                df[k] += 1;
            }
        }
        all.push(h);
    }
    let n = files.len().max(1) as f64;
    let idf: Vec<f64> = df.iter().map(|d| (1.0 + n / (*d).max(1) as f64).ln()).collect();

    // Pass 2: score files and pick each file's best window.
    let mut scored: Vec<(f64, &IndexedFile, usize, String)> = vec![];
    for (f, h) in files.iter().zip(&all) {
        let is_focus = focus == Some(f.path.as_str());
        let line_count = f.text.lines().count();
        let mut score = 0.0;
        let mut reasons: Vec<String> = vec![];
        let mut weight = vec![0.0f64; line_count];
        let mut mentioned: Vec<&str> = vec![];
        for (k, kw) in kws.iter().enumerate() {
            if h.path[k] {
                score += 4.0 * idf[k];
                reasons.push(format!("path contains “{kw}”"));
            }
            if let Some((w, line, name)) = &h.symbol[k] {
                score += w * idf[k];
                if let Some(slot) = weight.get_mut(*line as usize - 1) {
                    *slot += w * idf[k];
                }
                if reasons.len() < 3 {
                    reasons.push(format!("defines {name}"));
                }
            }
            if !h.lines[k].is_empty() {
                score += h.lines[k].len().min(6) as f64 * idf[k];
                mentioned.push(kw);
                for &i in &h.lines[k] {
                    weight[i] += idf[k];
                }
            }
        }
        // Reward files where several different keywords appear together.
        score *= 1.0 + 0.35 * (mentioned.len().saturating_sub(1)) as f64;
        if reasons.is_empty() && !mentioned.is_empty() {
            reasons.push(format!("mentions {}", mentioned.join(", ")));
        }
        if is_focus {
            score += 1.0e6;
            reasons.insert(0, "the file you asked about".to_string());
        } else if let Some((_, why)) = related.iter().find(|(p, _)| *p == f.path) {
            // Direct neighbours of the focus file matter more than keyword coincidences.
            score += 1.0e3;
            reasons.insert(0, (*why).to_string());
        }
        if score <= 0.0 || line_count == 0 {
            continue;
        }
        // Densest window of WINDOW_LINES lines.
        let window = if is_focus { WINDOW_LINES * 3 } else { WINDOW_LINES };
        let mut start = 0usize;
        if !is_focus && line_count > window {
            let mut sum: f64 = weight[..window].iter().sum();
            let mut best = sum;
            for i in 1..=line_count - window {
                sum += weight[i + window - 1] - weight[i - 1];
                if sum > best + 1e-9 {
                    best = sum;
                    start = i;
                }
            }
            // Keep a little lead-in before the first matching line of the chosen window.
            if let Some(first) = (start..start + window).find(|i| weight[*i] > 0.0) {
                start = first.saturating_sub(WINDOW_BEFORE).min(line_count - window);
            }
        }
        scored.push((score, f, start, reasons.join("; ")));
    }
    scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal).then(a.1.path.cmp(&b.1.path)));

    let mut out = vec![];
    let mut used = 0usize;
    for (_, f, start, reason) in scored.into_iter().take(MAX_SOURCES) {
        let lines: Vec<&str> = f.text.lines().collect();
        let window = if focus == Some(f.path.as_str()) { WINDOW_LINES * 3 } else { WINDOW_LINES };
        let end = (start + window).min(lines.len());
        let mut text = lines[start..end].join("\n");
        if used + text.len() > BUDGET_CHARS {
            let room = BUDGET_CHARS.saturating_sub(used);
            if room < 400 {
                break;
            }
            let mut cut = room;
            while !text.is_char_boundary(cut) {
                cut -= 1;
            }
            text.truncate(cut);
        }
        used += text.len();
        out.push(Source { n: out.len() + 1, path: f.path.clone(), start_line: start as u32 + 1, end_line: end as u32, text, reason });
    }
    out
}

/// The prompt sent to a model: numbered sources, then the question.
pub fn build_prompt(question: &str, sources: &[Source]) -> (String, String) {
    let system = "You answer questions about a software project using only the numbered source excerpts provided. \
                  Cite the excerpts you rely on inline as [1], [2] and so on. If the excerpts do not contain the answer, \
                  say what is missing instead of guessing. Be concise and concrete."
        .to_string();
    let mut user = String::new();
    for s in sources {
        user.push_str(&format!("[{}] {} (lines {}-{})\n```\n{}\n```\n\n", s.n, s.path, s.start_line, s.end_line, s.text));
    }
    user.push_str(&format!("Question: {question}"));
    (system, user)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(path: &str, text: &str, symbols: &[(&str, u32)]) -> IndexedFile {
        IndexedFile { path: path.into(), text: text.into(), symbols: symbols.iter().map(|(n, l)| (n.to_string(), *l)).collect() }
    }

    #[test]
    fn extracts_keywords() {
        assert_eq!(keywords("How does the task approval gate work in run_loop?"), ["task", "approval", "gate", "run_loop"]);
    }

    #[test]
    fn ranks_definitions_above_mentions_and_windows_around_them() {
        let long: String =
            (1..=200).map(|i| if i == 150 { "fn run_loop() { approval }\n".to_string() } else { format!("// line {i}\n") }).collect();
        let files = [
            file("src/tasks.rs", &long, &[("run_loop", 150)]),
            file("src/notes.md", "we call run_loop here\n", &[]),
            file("src/other.rs", "nothing relevant\n", &[("unrelated", 1)]),
        ];
        let s = retrieve("where is run_loop defined?", &files, None, &[]);
        assert_eq!(s.iter().map(|x| x.path.as_str()).collect::<Vec<_>>(), ["src/tasks.rs", "src/notes.md"]);
        assert_eq!(s[0].n, 1);
        assert!(s[0].start_line <= 150 && s[0].end_line >= 150, "{}..{}", s[0].start_line, s[0].end_line);
        assert!(s[0].text.contains("fn run_loop"));
        assert!(s[0].reason.contains("defines run_loop"));
    }

    #[test]
    fn focus_file_always_comes_first_and_prompt_numbers_sources() {
        let files = [file("a.rs", "alpha beta\n", &[("alpha", 1)]), file("b.rs", "unrelated\n", &[])];
        let s = retrieve("alpha", &files, Some("b.rs"), &[]);
        assert_eq!(s[0].path, "b.rs");
        let (system, user) = build_prompt("what is alpha?", &s);
        assert!(system.contains("[1]"));
        assert!(user.contains("[1] b.rs (lines 1-1)") && user.contains("[2] a.rs") && user.ends_with("Question: what is alpha?"));
        assert!(retrieve("zzzz", &files, None, &[]).is_empty());
    }

    #[test]
    fn neighbours_of_the_focus_file_rank_right_after_it() {
        let files = [file("a.rs", "widget\n", &[]), file("b.rs", "x\n", &[]), file("c.rs", "widget widget widget\n", &[("widget", 1)])];
        let related = [("b.rs".to_string(), "imported by the file you asked about")];
        let s = retrieve("widget", &files, Some("a.rs"), &related);
        assert_eq!(s.iter().map(|x| x.path.as_str()).collect::<Vec<_>>(), ["a.rs", "b.rs", "c.rs"]);
        assert!(s[1].reason.starts_with("imported by"));
    }

    #[test]
    fn picks_the_densest_passage_and_weights_rare_words() {
        // "approval" is rare and clustered late in the file; "task" is everywhere.
        let mut text = String::new();
        for i in 1..=300 {
            text.push_str(&match i {
                10 => "struct Task { id: u32 }\n".to_string(),
                200..=204 => "// wait for approval before the task command starts\n".to_string(),
                _ => format!("// filler {i}\n"),
            });
        }
        let common: String = (0..50).map(|_| "task task task\n").collect();
        let files = [file("src/tasks.rs", &text, &[("Task", 10)]), file("src/list.rs", &common, &[]), file("src/z.rs", "task\n", &[])];
        let s = retrieve("how does task approval work?", &files, None, &[]);
        assert_eq!(s[0].path, "src/tasks.rs");
        assert!(s[0].start_line <= 200 && s[0].end_line >= 204, "window was {}..{}", s[0].start_line, s[0].end_line);
    }
}
