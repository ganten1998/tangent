//! **The phrase scan: no surface says a key does less than it does.**
//!
//! For its whole life until the unlimited desk, the product said a supporter
//! key was "a thank-you, not an unlock" and that Tangent "stays free", on
//! five surfaces at once: the Supporter Key dialog, the README, the store
//! page, the key email and the About card. A key now lifts the free desk's
//! thirty-two track limit (keys already sold included), so every one of
//! those sentences became a promise broken in the place a buyer reads it.
//! They were rewritten together; this keeps any of them from coming back on
//! one surface while the others say the opposite, which is how the About
//! card came to print "16-channel mixer" over a 32-channel desk.
//!
//! **What is scanned, and why that and not more.**
//!
//! * The prose surfaces whole: `README.md`, `docs/STORE-CONTENT.md` (the
//!   store description and the post-purchase page are pasted from it), the
//!   new release's `GUMROAD.md`, and the `[Unreleased]` section of
//!   `CHANGELOG.md`. Older Gumroad pages and released changelog sections are
//!   history and say what was true then.
//! * The Rust surfaces by their STRING LITERALS outside test code:
//!   `tangent-ui/src` and `tools/tangent-fulfil/src` (the key email). A
//!   comment is free to quote the stale line as the reason it was changed,
//!   which is this repo's idiom, and an assertion message in a test is not
//!   something the product says.
//!
//! **What is refused** ([`stale`] is the one rule):
//!
//! * "thank-you, nothing more", "thank-you, not an unlock", "not an unlock";
//! * "nothing else" or "nothing more" in a sentence that names the key;
//! * "stays free" in a sentence that does not name the track limit;
//! * "Thirty-two channels", "Thirty-two tracks", "-channel mixer" and
//!   "{}-channel": the desk described by a width, for everybody.
//!
//! This file is the one place the refused phrases may appear as literals, so
//! it is the one file the scan skips.

use std::path::{Path, PathBuf};

/// Phrases refused anywhere, in any case.
const ALWAYS: [&str; 3] = [
    "thank-you, nothing more",
    "thank-you, not an unlock",
    "not an unlock",
];

/// The desk described by a width that is everybody's. Matched as written:
/// the capital is the claim's start ("Thirty-two channels plus a master"),
/// while a test name or a comment reading "thirty-two tracks" of the FREE
/// desk is true.
const WIDTH_CLAIMS: [&str; 4] = [
    "Thirty-two channels",
    "Thirty-two tracks",
    "-channel mixer",
    "{}-channel",
];

/// What is wrong with `text`, one entry per hit, or nothing.
///
/// `text` is one surface's copy: a whole prose file, or one string literal.
/// The sentence rules read sentences, split by [`sentences`].
fn stale(text: &str) -> Vec<String> {
    let mut hits = Vec::new();
    let flat = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let lower = flat.to_lowercase();
    for phrase in ALWAYS {
        if lower.contains(phrase) {
            hits.push(format!("says {phrase:?}"));
        }
    }
    for phrase in WIDTH_CLAIMS {
        if flat.contains(phrase) {
            hits.push(format!("describes the desk by a width: {phrase:?}"));
        }
    }
    for sentence in sentences(text) {
        let low = sentence.to_lowercase();
        let names_the_key = has_word(&low, "key") || has_word(&low, "keys") || low.contains("supporter");
        if names_the_key && (low.contains("nothing else") || low.contains("nothing more")) {
            hits.push(format!("says a key does nothing else: {sentence:?}"));
        }
        let names_the_limit = (low.contains("32") || low.contains("thirty-two")) && low.contains("track");
        if low.contains("stays free") && !names_the_limit {
            hits.push(format!("says \"stays free\" without the track limit: {sentence:?}"));
        }
    }
    hits
}

/// `word` in `text` with no letter, digit or hyphen joined to either side,
/// so "key" is found in "a key." and not in "keyboard" or "keep".
fn has_word(text: &str, word: &str) -> bool {
    let joined = |c: Option<char>| c.is_some_and(|c| c.is_alphanumeric() || c == '-' || c == '_');
    text.match_indices(word).any(|(at, _)| {
        !joined(text[..at].chars().next_back()) && !joined(text[at + word.len()..].chars().next())
    })
}

/// `text` as sentences, whitespace folded.
///
/// A blank line, a heading (which is a block of its own), a list item, a
/// table row or a quote starts a new block (Markdown's own boundaries, so a
/// bullet is never read as the end of the sentence above it); inside a block
/// a sentence ends at '.', '!' or '?' followed by a space, with any closing
/// emphasis, quote or bracket between them. Rust's `\n` escapes are already newlines by the time
/// a literal arrives here (see [`literals`]).
fn sentences(text: &str) -> Vec<String> {
    let mut blocks: Vec<String> = Vec::new();
    let mut cur = String::new();
    for line in text.lines() {
        let t = line.trim();
        let starts_block = t.is_empty()
            || t.starts_with(['#', '-', '*', '|', '>'])
            || t.split_once(". ").is_some_and(|(n, _)| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()));
        if starts_block && !cur.is_empty() {
            blocks.push(std::mem::take(&mut cur));
        }
        if !cur.is_empty() {
            cur.push(' ');
        }
        cur.push_str(t);
        // A heading is one line: what follows it is a new block.
        if t.starts_with('#') {
            blocks.push(std::mem::take(&mut cur));
        }
    }
    blocks.push(cur);
    let mut out = Vec::new();
    for block in blocks {
        let block = block.split_whitespace().collect::<Vec<_>>().join(" ");
        let mut rest = block.as_str();
        // A stop may sit inside the emphasis or quote it closes: "set.**"
        // and "said so.\"" end their sentences too.
        let ends = |head: &str| {
            head.trim_end_matches(['*', '_', '"', '\'', ')', '`']).ends_with(['.', '!', '?'])
        };
        while let Some(at) = rest.match_indices(' ').map(|(i, _)| i).find(|&i| ends(&rest[..i])) {
            out.push(rest[..at].to_owned());
            rest = &rest[at + 1..];
        }
        if !rest.is_empty() {
            out.push(rest.to_owned());
        }
    }
    out
}

/// Every string literal in `src` outside test code, escapes resolved
/// enough for prose: `\n` and `\t` are whitespace, a `\` line continuation
/// joins its lines, `\"` is a quote.
///
/// A small lexer rather than a text search, because a text search reads the
/// comments (where a stale line is quoted on purpose) and cannot tell an
/// item under `#[cfg(test)]` from one that ships. It knows `//` and nested
/// `/* */` comments, normal, byte and raw strings, char literals (so `'"'`
/// opens nothing) and lifetimes. An item under `#[cfg(test)]` is skipped to
/// its end: the `;` or the `}` that closes its first `{`, counted with
/// strings and comments still lexed so a brace inside one is not a brace.
fn literals(src: &str) -> Vec<String> {
    let s: Vec<char> = src.chars().collect();
    let n = s.len();
    let at = |i: usize, pat: &str| pat.chars().enumerate().all(|(k, c)| s.get(i + k) == Some(&c));
    let mut out = Vec::new();
    // `Some(depth)` while inside a skipped test item: the depth of `{` /
    // `(` / `[` reached in it, and whether its body has opened.
    let mut skipping: Option<(i32, bool)> = None;
    let mut i = 0;
    while i < n {
        let c = s[i];
        if at(i, "//") {
            while i < n && s[i] != '\n' {
                i += 1;
            }
            continue;
        }
        if at(i, "/*") {
            let mut depth = 0;
            while i < n {
                if at(i, "/*") {
                    depth += 1;
                    i += 2;
                } else if at(i, "*/") {
                    depth -= 1;
                    i += 2;
                    if depth == 0 {
                        break;
                    }
                } else {
                    i += 1;
                }
            }
            continue;
        }
        if at(i, "#[cfg(test)]") && skipping.is_none() {
            skipping = Some((0, false));
            i += "#[cfg(test)]".len();
            continue;
        }
        let ident_before = i > 0 && (s[i - 1].is_alphanumeric() || s[i - 1] == '_');
        // Raw strings: r"..", r#".."#, br"..", with any number of hashes.
        let raw_at = if at(i, "br") { Some(i + 2) } else if c == 'r' { Some(i + 1) } else { None };
        if let Some(mut j) = raw_at.filter(|_| !ident_before) {
            let mut hashes = 0;
            while s.get(j) == Some(&'#') {
                hashes += 1;
                j += 1;
            }
            if s.get(j) == Some(&'"') {
                let close = format!("\"{}", "#".repeat(hashes));
                let start = j + 1;
                let mut end = start;
                while end < n && !at(end, &close) {
                    end += 1;
                }
                if skipping.is_none() {
                    out.push(s[start..end.min(n)].iter().collect());
                }
                i = end + close.chars().count();
                continue;
            }
        }
        if c == '"' {
            let mut lit = String::new();
            let mut j = i + 1;
            while j < n && s[j] != '"' {
                if s[j] == '\\' && j + 1 < n {
                    match s[j + 1] {
                        'n' | 't' | 'r' => lit.push('\n'),
                        // A line continuation: the newline and the next
                        // line's indent vanish.
                        '\n' => {
                            j += 2;
                            while j < n && s[j].is_whitespace() {
                                j += 1;
                            }
                            continue;
                        }
                        other => lit.push(other),
                    }
                    j += 2;
                } else {
                    lit.push(s[j]);
                    j += 1;
                }
            }
            if skipping.is_none() {
                out.push(lit);
            }
            i = j + 1;
            continue;
        }
        if c == '\'' {
            if s.get(i + 1) == Some(&'\\') {
                // '\n', '\'', '\u{..}': past the escaped character, to
                // the closing quote.
                let mut j = i + 3;
                while j < n && s[j] != '\'' {
                    j += 1;
                }
                i = j + 1;
            } else if s.get(i + 2) == Some(&'\'') {
                i += 3;
            } else {
                // A lifetime.
                i += 1;
            }
            continue;
        }
        if let Some((depth, opened)) = skipping.as_mut() {
            match c {
                '{' | '(' | '[' => {
                    if c == '{' && *depth == 0 {
                        *opened = true;
                    }
                    *depth += 1;
                }
                '}' | ')' | ']' => {
                    *depth -= 1;
                    // The `}` that closes the item's body, or one that
                    // closes what ENCLOSES an item with no body of its own
                    // (a field or a variant under the attribute).
                    if (c == '}' && *depth == 0 && *opened) || *depth < 0 {
                        skipping = None;
                    }
                }
                ';' if *depth == 0 => skipping = None,
                _ => {}
            }
        }
        i += 1;
    }
    out
}

/// The repository root.
fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..")
}

/// Every `.rs` file directly in `dir`, by path. The two scanned trees are
/// flat; a test below fails if either grows a subdirectory this would miss.
fn rust_files(dir: &Path) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir)
        .unwrap_or_else(|e| panic!("{}: {e}", dir.display()))
        .map(|e| e.expect("a directory entry").path())
        .collect();
    assert!(
        files.iter().all(|p| !p.is_dir()),
        "{} has a subdirectory the copy scan does not walk",
        dir.display()
    );
    files.retain(|p| p.extension().is_some_and(|e| e == "rs"));
    files.sort();
    files
}

/// The `[Unreleased]` section of a Keep-a-Changelog file: from its heading to
/// the next `## [`.
fn unreleased(changelog: &str) -> &str {
    let start = changelog.find("## [Unreleased]").expect("CHANGELOG.md has no [Unreleased] section");
    let body = &changelog[start..];
    let end = body[3..].find("\n## [").map_or(body.len(), |e| e + 3);
    &body[..end]
}

/// The new release's Gumroad page. The older pages under `docs/release/`
/// stay as the history of what each release's page said.
const GUMROAD_PAGE: &str = "docs/release/next/GUMROAD.md";

/// **No surface says a key does less than it does.** See the module doc.
#[test]
fn no_surface_says_a_key_is_only_a_thank_you() {
    let root = root();
    let mut hits = Vec::new();
    for doc in ["README.md", "docs/STORE-CONTENT.md", GUMROAD_PAGE] {
        let text = std::fs::read_to_string(root.join(doc)).unwrap_or_else(|e| panic!("{doc}: {e}"));
        hits.extend(stale(&text).into_iter().map(|h| format!("{doc}: {h}")));
    }
    let changelog = std::fs::read_to_string(root.join("CHANGELOG.md")).expect("CHANGELOG.md");
    hits.extend(stale(unreleased(&changelog)).into_iter().map(|h| format!("CHANGELOG.md [Unreleased]: {h}")));
    let mut scanned = 0;
    for dir in ["tangent-ui/src", "tools/tangent-fulfil/src"] {
        for file in rust_files(&root.join(dir)) {
            if file.file_name().is_some_and(|f| f == "copy_scan.rs") {
                continue;
            }
            scanned += 1;
            let src = std::fs::read_to_string(&file).expect("a source file");
            for lit in literals(&src) {
                hits.extend(stale(&lit).into_iter().map(|h| format!("{}: {h}", file.display())));
            }
        }
    }
    // Not a vacuous pass: both trees were found and read.
    assert!(scanned > 50, "the scan read only {scanned} Rust files");
    assert!(hits.is_empty(), "stale supporter copy:\n  {}", hits.join("\n  "));
}

/// **Every refused phrase is refused when planted**, on the surface kind it
/// would come back on: a prose page and a shipped string literal. The scan
/// above passing proves nothing unless this does too.
#[test]
fn every_refused_phrase_fails_the_scan_when_planted() {
    let plants = [
        "A key is a thank-you, nothing more.",
        "This key is a thank-you, not an unlock.",
        "Paying is not an unlock.",
        "A supporter key turns on a small heart and nothing else.",
        "It adds a cosmetic thank-you, and your key does nothing more.",
        "Tangent is free and it stays free.",
        "Thirty-two channels plus a master.",
        "Thirty-two tracks, each with a fader.",
        "Recorder, 32-channel mixer, arrangement.",
        "Recorder, {}-channel mixer.",
    ];
    for plant in plants {
        let page = format!("# Tangent\n\nPlay a chord.\n\n{plant}\n\n- A bullet.\n");
        assert!(!stale(&page).is_empty(), "the prose scan let {plant:?} through");
        let src = format!("fn card() -> &'static str {{\n    \"{plant}\"\n}}\n");
        let lits = literals(&src);
        assert_eq!(lits, vec![plant.to_owned()], "the lexer misread {src}");
        assert!(lits.iter().any(|l| !stale(l).is_empty()), "the literal scan let {plant:?} through");
    }
}

/// **The scan passes the copy that replaced them**, so it refuses the stale
/// line and not the subject.
#[test]
fn the_new_copy_passes_the_scan() {
    for good in [
        "Tangent is free, with up to 32 tracks.",
        "A supporter key (or a certain code) takes the track limit away and lights the heart.",
        "Buses, returns, sends and routes have no limit for anyone.",
        "Tangent is free and stays free at 32 tracks.",
        "Recorder, 32-track desk, arrangement, MIDI editor, drum machine, VST3",
        // "nothing else" in a sentence about something other than the key.
        "Folders are buses, and nothing else changes. The key of C is bright.",
        "A keyboard sends notes and nothing else.",
    ] {
        assert!(stale(good).is_empty(), "{good:?} was refused: {:?}", stale(good));
    }
}

/// **The lexer reads what ships and only that.** Comments may quote a stale
/// line as the reason it changed, a char literal is not a string, and an
/// item under `#[cfg(test)]` (a test's assertion message) is not copy.
#[test]
fn the_lexer_reads_shipped_literals_only() {
    let src = r##"
// A key was "a thank-you, nothing more".
/* nested /* "not an unlock" */ still a comment */
fn a<'x>(q: &'x str) -> char { let _ = '"'; let _ = '\''; 'a' }
const SHIPPED: &str = "one";
const RAW: &str = r#"two "quoted""#;
const JOINED: &str = "three \
                      four\nfive";
#[cfg(test)]
mod tests {
    fn t() { assert!(true, "{ not shipped }"); }
}
#[cfg(test)]
const TEST_ONLY: &str = "also not shipped";
const AFTER: &str = "six";
"##;
    assert_eq!(
        literals(src),
        vec!["one", "two \"quoted\"", "three four\nfive", "six"],
    );
}

#[test]
fn sentences_end_at_stops_and_markdown_blocks() {
    let text = "One sentence\nover two lines. Two!\n\n- a bullet\n- **Bold.** Then this.\n## Heading\nAfter it.";
    assert_eq!(
        sentences(text),
        vec![
            "One sentence over two lines.",
            "Two!",
            "- a bullet",
            "- **Bold.**",
            "Then this.",
            "## Heading",
            "After it.",
        ],
    );
    assert!(has_word("a key.", "key") && !has_word("keyboard", "key") && !has_word("keep", "key"));
}
