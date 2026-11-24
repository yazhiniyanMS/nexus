//! Credential detection in source text, and protected editing.
//!
//! [`detect`] finds the *values* of credentials (not their names) in text: private-key bodies,
//! well-known token formats, passwords inside connection strings, and string values assigned to
//! secret-named identifiers or `.env` keys. References to the environment (`process.env.X`,
//! `${X}`, `os.environ[...]`), placeholders ("your-api-key") and prose are left alone.
//!
//! Two uses:
//! - [`mask`] for text a model only reads (questions, research, review): values become
//!   `[redacted]`.
//! - [`Vault`] for text a model must reproduce (the Developer agent): each value becomes a unique
//!   placeholder before sending, and [`Vault::restore`] puts the exact original back in whatever
//!   the model writes. The provider never sees the value and the file is not corrupted.
//!
//! Detection is pattern-based. It will miss credentials with no recognisable shape under
//! unremarkable names, and it may hide harmless strings that look like credentials; with the
//! vault, a false positive costs the model some context but never changes the code.

use regex::Regex;
use std::sync::OnceLock;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// The body of a `-----BEGIN … PRIVATE KEY-----` block.
    PrivateKey,
    /// A value in a vendor token format (AWS, GitHub, Slack, OpenAI, Anthropic, Google, Stripe…).
    Token,
    /// The password part of `scheme://user:password@host`.
    Password,
    /// A string assigned to a secret-named identifier or `.env` key.
    Assignment,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Span {
    pub start: usize,
    pub end: usize,
    pub kind: Kind,
}

struct Patterns {
    private_key: Regex,
    token: Regex,
    connection: Regex,
    assignment: Regex,
    dotenv: Regex,
}

fn patterns() -> &'static Patterns {
    static P: OnceLock<Patterns> = OnceLock::new();
    P.get_or_init(|| Patterns {
        private_key: Regex::new(r"-----BEGIN [A-Z0-9 ]*PRIVATE KEY-----\r?\n([\s\S]*?)\r?\n?-----END [A-Z0-9 ]*PRIVATE KEY-----").unwrap(),
        token: Regex::new(concat!(
            r"\b(?:AKIA|ASIA)[0-9A-Z]{16}\b",
            r"|\bgh[pousr]_[A-Za-z0-9]{30,}\b",
            r"|\bgithub_pat_[A-Za-z0-9_]{30,}\b",
            r"|\bglpat-[A-Za-z0-9_-]{20,}\b",
            r"|\bxox[abprs]-[A-Za-z0-9-]{10,}\b",
            r"|\bsk-(?:ant-|proj-)?[A-Za-z0-9_-]{20,}\b",
            r"|\b(?:sk|rk)_(?:live|test)_[A-Za-z0-9]{16,}\b",
            r"|\bAIza[0-9A-Za-z_-]{35}\b",
            r"|\bhf_[A-Za-z0-9]{30,}\b",
            r"|\bnpm_[A-Za-z0-9]{36}\b",
            r"|\beyJ[A-Za-z0-9_-]{10,}\.eyJ[A-Za-z0-9_-]{10,}\.[A-Za-z0-9_-]{10,}\b",
        ))
        .unwrap(),
        // scheme://user:password@host — the password is group 1.
        connection: Regex::new(r"\b[a-zA-Z][a-zA-Z0-9+.-]*://[^\s:/@'\x22]+:([^\s@'\x22/]+)@").unwrap(),
        // name, optional type annotation, `:` `=` or `=>`, then a quote; the value is read after it.
        assignment: Regex::new(r#"([A-Za-z_][A-Za-z0-9_.-]*)["']?\s*(?::[^=\n"'(){}]{1,40}=|=>|[:=])\s*(["'`])"#).unwrap(),
        // KEY=value lines of .env files and shell scripts: no spaces around `=`, which is what
        // tells them apart from assignments in code (`api_key = os.environ[...]`).
        dotenv: Regex::new(r#"(?m)^[ \t]*(?:export[ \t]+)?([A-Za-z_][A-Za-z0-9_]*)=(["']?)([^\s"'#][^\r\n"']*)"#).unwrap(),
    })
}

/// Words that mark a value as an example rather than a credential.
fn looks_like_placeholder(value: &str) -> bool {
    let v = value.to_ascii_lowercase();
    v.len() < 8
        || v.chars().any(char::is_whitespace)
        || v.starts_with('$')
        || v.starts_with('%')
        || v.starts_with('<')
        || v.contains("${")
        || v.contains("{{")
        || ["your", "example", "changeme", "change_me", "placeholder", "dummy", "xxxxxx", "******", "redacted", "todo", "replace"]
            .iter()
            .any(|w| v.contains(w))
        || v.chars().all(|c| c == v.chars().next().unwrap_or('x'))
}

/// `apiKey`, `api_key`, `API-KEY` → upper-case segments.
fn segments(identifier: &str) -> Vec<String> {
    let mut out = String::new();
    let mut prev_lower = false;
    for c in identifier.chars() {
        if c.is_ascii_uppercase() && prev_lower {
            out.push('_');
        }
        prev_lower = c.is_ascii_lowercase() || c.is_ascii_digit();
        out.push(if c == '-' || c == '.' { '_' } else { c.to_ascii_uppercase() });
    }
    out.split('_').filter(|s| !s.is_empty()).map(String::from).collect()
}

/// Whether an identifier in code names a credential. A bare `key` (React keys, map keys) does
/// not; `apiKey`, `secret_key`, `password`, `authToken` do.
pub fn is_secret_identifier(identifier: &str) -> bool {
    let segs = segments(identifier);
    let has = |s: &str| segs.iter().any(|x| x == s);
    let strong = ["SECRET", "SECRETS", "PASSWORD", "PASSWD", "PASSPHRASE", "TOKEN", "APIKEY", "CREDENTIAL", "CREDENTIALS", "PRIVATE"];
    if strong.iter().any(|s| has(s)) {
        return true;
    }
    has("KEY") && ["API", "ACCESS", "SIGNING", "ENCRYPTION", "MASTER", "CLIENT", "AUTH", "LICENSE", "WEBHOOK"].iter().any(|q| has(q))
}

fn push(out: &mut Vec<Span>, start: usize, end: usize, kind: Kind) {
    if end > start {
        out.push(Span { start, end, kind });
    }
}

/// Credential values in `text`, in order, never overlapping.
pub fn detect(text: &str) -> Vec<Span> {
    let p = patterns();
    let mut spans = vec![];
    for c in p.private_key.captures_iter(text) {
        let body = c.get(1).unwrap();
        push(&mut spans, body.start(), body.end(), Kind::PrivateKey);
    }
    for m in p.token.find_iter(text) {
        push(&mut spans, m.start(), m.end(), Kind::Token);
    }
    for c in p.connection.captures_iter(text) {
        // Any password in a URL is masked, short or not; only references are left alone.
        let pw = c.get(1).unwrap();
        let v = pw.as_str();
        if !(v.starts_with('$') || v.starts_with('%') || v.starts_with('<') || v.contains("${") || v.contains("{{")) {
            push(&mut spans, pw.start(), pw.end(), Kind::Password);
        }
    }
    for c in p.assignment.captures_iter(text) {
        let name = c.get(1).unwrap().as_str();
        if !is_secret_identifier(name) {
            continue;
        }
        let quote = c.get(2).unwrap();
        let rest = &text[quote.end()..];
        let Some(close) = rest.find(|ch: char| ch == quote.as_str().chars().next().unwrap() || ch == '\n') else { continue };
        if rest[close..].starts_with('\n') {
            continue;
        }
        let value = &rest[..close];
        if !looks_like_placeholder(value) {
            push(&mut spans, quote.end(), quote.end() + close, Kind::Assignment);
        }
    }
    for c in p.dotenv.captures_iter(text) {
        let name = c.get(1).unwrap().as_str();
        if !(crate::exec::env::is_secret_name(name) || is_secret_identifier(name)) {
            continue;
        }
        let value = c.get(3).unwrap();
        let v = value.as_str().trim_end();
        // URLs are left to the connection-string rule, which masks only their password.
        if !v.contains("://") && !looks_like_placeholder(v) {
            push(&mut spans, value.start(), value.start() + v.len(), Kind::Assignment);
        }
    }
    // Keep the earliest span where two overlap (a token inside an assignment is one secret).
    spans.sort_by_key(|s| (s.start, std::cmp::Reverse(s.end)));
    let mut out: Vec<Span> = vec![];
    for s in spans {
        if out.last().is_none_or(|last| s.start >= last.end) {
            out.push(s);
        }
    }
    out
}

/// Replace every credential value with `[redacted]` (private-key bodies with a note), for text
/// that is only read.
pub fn mask(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut at = 0;
    for s in detect(text) {
        out.push_str(&text[at..s.start]);
        out.push_str(if s.kind == Kind::PrivateKey { "[private key redacted]" } else { "[redacted]" });
        at = s.end;
    }
    out.push_str(&text[at..]);
    out
}

/// Reversible protection for text a model edits. Each distinct value gets a placeholder that is
/// unique to this vault, so the model cannot guess or forge one from another run.
pub struct Vault {
    tag: String,
    values: Vec<String>,
}

impl Default for Vault {
    fn default() -> Self {
        Vault { tag: crate::db::new_id().replace('-', "")[..10].to_string(), values: vec![] }
    }
}

impl Vault {
    fn placeholder(&self, i: usize) -> String {
        format!("NEXUS_SECRET_{}_{}", i + 1, self.tag)
    }

    /// Number of distinct values protected so far.
    pub fn len(&self) -> usize {
        self.values.len()
    }

    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }

    /// `text` with every detected credential value replaced by its placeholder.
    pub fn protect(&mut self, text: &str) -> String {
        let mut out = String::with_capacity(text.len());
        let mut at = 0;
        for s in detect(text) {
            let value = &text[s.start..s.end];
            let i = match self.values.iter().position(|v| v == value) {
                Some(i) => i,
                None => {
                    self.values.push(value.to_string());
                    self.values.len() - 1
                }
            };
            out.push_str(&text[at..s.start]);
            out.push_str(&self.placeholder(i));
            at = s.end;
        }
        out.push_str(&text[at..]);
        out
    }

    /// Put the original values back. A placeholder this vault never issued (a model inventing
    /// one, or copying one from another run) is an error rather than text written to a file.
    pub fn restore(&self, text: &str) -> std::result::Result<String, String> {
        static ANY: OnceLock<Regex> = OnceLock::new();
        let any = ANY.get_or_init(|| Regex::new(r"NEXUS_SECRET_(\d+)_([0-9a-f]{10})").unwrap());
        let mut out = String::with_capacity(text.len());
        let mut at = 0;
        for c in any.captures_iter(text) {
            let whole = c.get(0).unwrap();
            let n: usize = c[1].parse().unwrap_or(0);
            if c[2] != self.tag || n == 0 || n > self.values.len() {
                return Err(format!("the model wrote a credential placeholder NEXUS did not give it ({})", whole.as_str()));
            }
            out.push_str(&text[at..whole.start()]);
            out.push_str(&self.values[n - 1]);
            at = whole.end();
        }
        out.push_str(&text[at..]);
        Ok(out)
    }

    /// The instruction the model needs to keep placeholders intact.
    pub fn instruction(&self) -> Option<String> {
        (!self.is_empty()).then(|| {
            format!(
                "Some values were replaced with placeholders of the form NEXUS_SECRET_<n>_{} because they are credentials kept on the \
                 user's machine. Where code needs one of those values, write its placeholder exactly as given; never change, split or \
                 invent placeholders, and never write a real credential.",
                self.tag
            )
        })
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// Fake credentials are built from fragments so the repository itself never contains a
    /// string a secret scanner would flag.
    fn fake(prefix: &str, body: &str, n: usize) -> String {
        format!("{prefix}{}", body.repeat(n))
    }

    /// A PEM marker line, assembled so no literal key header sits in the source.
    pub(crate) fn pem(edge: &str) -> String {
        format!("-----{edge} RSA {} KEY-----", "PRIVATE")
    }

    fn sample() -> String {
        let aws = fake("AKIA", "Q", 16);
        let gh = fake("ghp_", "a1", 18);
        let ant = fake("sk-ant-", "b2", 15);
        let slack = fake("xoxb-", "12345-", 4);
        format!(
            "const config = {{\n  apiKey: \"{ant}\",\n  region: \"eu-west-1\",\n}};\n\
             const password: &str = \"hunter2hunter2\";\n\
             AWS_ACCESS_KEY_ID = '{aws}'\n\
             headers = {{\"Authorization\": \"token {gh}\"}}\n\
             DATABASE_URL=postgres://app:Sup3rS3cret!@db.internal:5432/app\n\
             export SLACK_BOT_TOKEN={slack}\n\
             const db = mysql.connect(\"mysql://root:r00tpassw0rd@localhost/shop\");\n\
             {pem_start}\nMIIEpAIBAAKCAQEA1\nqwertyuiop0987\n{pem_end}\n",
            pem_start = pem("BEGIN"),
            pem_end = pem("END"),
        )
    }

    #[test]
    fn finds_each_kind_of_credential() {
        let text = sample();
        let found: Vec<(&str, Kind)> = detect(&text).iter().map(|s| (&text[s.start..s.end], s.kind)).collect();
        let values: Vec<&str> = found.iter().map(|(v, _)| *v).collect();
        assert!(values.contains(&"hunter2hunter2"), "{found:?}");
        assert!(values.contains(&"Sup3rS3cret!"), "connection string password only: {found:?}");
        assert!(values.contains(&"r00tpassw0rd"), "{found:?}");
        assert!(values.iter().any(|v| v.starts_with("sk-ant-")), "{found:?}");
        assert!(values.iter().any(|v| v.starts_with("AKIA")), "{found:?}");
        assert!(values.iter().any(|v| v.starts_with("ghp_")), "{found:?}");
        assert!(values.iter().any(|v| v.starts_with("xoxb-")), "{found:?}");
        assert!(found.iter().any(|(v, k)| *k == Kind::PrivateKey && v.contains("qwertyuiop0987")), "{found:?}");
        assert!(!values.contains(&"eu-west-1"), "ordinary configuration is not a secret");
    }

    #[test]
    fn references_placeholders_and_ordinary_code_are_not_secrets() {
        for text in [
            "const apiKey = process.env.OPENAI_API_KEY;",
            "api_key = os.environ[\"API_KEY\"]",
            "API_KEY=${API_KEY}",
            "password: \"${DB_PASSWORD}\"",
            "token = \"<your token here>\"",
            "OPENAI_API_KEY=your-api-key-here",
            "const passwordLabel = \"Enter your password\";",
            "items.map(item => <Row key=\"settings-sidebar-row\" />)",
            "const tokenizer = \"whitespace-splitter\";",
            "postgres://db.internal:5432/app",
            "let password = read_password();",
            "minPasswordLength: \"12\"",
            "SECRET_KEY=changeme",
            "pub fn api_key(&self) -> &str { &self.key }",
        ] {
            assert!(detect(text).is_empty(), "false positive in {text:?}: {:?}", detect(text));
        }
    }

    /// The defining property: protecting and restoring gives back exactly the original text,
    /// and the protected text contains none of the values.
    #[test]
    fn protect_then_restore_is_exact_and_hides_every_value() {
        let text = sample();
        let mut vault = Vault::default();
        let protected = vault.protect(&text);
        for s in detect(&text) {
            let value = &text[s.start..s.end];
            assert!(!protected.contains(value), "{value} leaked:\n{protected}");
        }
        assert!(protected.contains(&pem("BEGIN")), "markers stay, so the model knows a key is there");
        assert_eq!(vault.restore(&protected).unwrap(), text);
        assert!(vault.instruction().unwrap().contains("NEXUS_SECRET_"));
    }

    /// A model that edits around placeholders keeps the user's values; one that invents or
    /// copies a placeholder is refused.
    #[test]
    fn edited_output_gets_the_real_values_back() {
        let text = "const config = { password: \"hunter2hunter2\", retries: 3 };\n";
        let mut vault = Vault::default();
        let protected = vault.protect(text);
        let placeholder = protected.split('"').nth(1).unwrap().to_string();
        assert!(placeholder.starts_with("NEXUS_SECRET_1_"));
        // The model renames the field, adds a timeout, and moves the value.
        let edited = format!("const config = {{\n  retries: 5,\n  timeoutMs: 1000,\n  dbPassword: \"{placeholder}\",\n}};\n");
        let restored = vault.restore(&edited).unwrap();
        assert!(restored.contains("dbPassword: \"hunter2hunter2\""), "{restored}");
        assert!(restored.contains("timeoutMs: 1000"));
        // Invented or foreign placeholders.
        let forged = edited.replace("NEXUS_SECRET_1_", "NEXUS_SECRET_7_");
        assert!(vault.restore(&forged).is_err());
        let other = Vault::default().protect(text);
        assert!(vault.restore(&other).is_err(), "a placeholder from another run is refused");
        // The same value twice gets one placeholder.
        let mut v = Vault::default();
        let twice = v.protect("a = { password: \"hunter2hunter2\" }\nb = { password: \"hunter2hunter2\" }\n");
        assert_eq!(v.len(), 1);
        assert_eq!(twice.matches("NEXUS_SECRET_1_").count(), 2);
    }

    #[test]
    fn masking_keeps_names_and_hides_values() {
        let masked = mask(&sample());
        assert!(masked.contains("const password: &str = \"[redacted]\";"), "{masked}");
        assert!(masked.contains("postgres://app:[redacted]@db.internal"), "{masked}");
        assert!(masked.contains("[private key redacted]"));
        assert!(masked.contains("region: \"eu-west-1\""));
        assert!(!masked.contains("hunter2") && !masked.contains("qwertyuiop"));
    }
}
