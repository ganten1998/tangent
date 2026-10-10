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
//!   newest release's `GUMROAD.md` (in `next/` until the cut renames it), and
//!   the `[Unreleased]` section of `CHANGELOG.md`. Older Gumroad pages and
//!   released changelog sections are history and say what was true then.
//! * The Rust surfaces by their STRING LITERALS outside test code:
//!   `tangent-ui/src` and `tools/tangent-fulfil/src` (the key email). A
//!   comment is free to quote the stale line as the reason it was changed,
//!   which is this repo's idiom, and an assertion message in a test is not
//!   something the product says.
//! * Everything else that ships, by the dash rule alone, since none of it
//!   says what a key does: every other crate's shipped strings, and what a
//!   download or an installer carries for a person to read or see (the
//!   licence pages, the README and the licence notes beside the program, the
//!   installers' pages and scripts, the FreeBSD package's message, and the
//!   text the build scripts write into a download). Each file is read as it
//!   reaches its reader ([`Reads`]): what a person is handed is read whole,
//!   comments and all; what a program parses or renders is read without the
//!   comments it drops.
//!
//! **What is refused** ([`stale`] is the one rule):
//!
//! * "thank-you, nothing more", "thank-you, not an unlock", "not an unlock";
//! * "nothing else" or "nothing more" in a sentence that names the key;
//! * "stays free" in a sentence that does not name the track limit;
//! * "Thirty-two channels", "Thirty-two tracks", "-channel mixer" and
//!   "{}-channel": the desk described by a width, for everybody;
//! * an em dash, anywhere: the owner's rule for published text, because
//!   the mark has come to read as machine-written and a reader once said so
//!   of his. A comma, a colon, parentheses or a full stop say the same.
//! * on a surface that says buses have no limit, a sentence calling folders
//!   buses that does not say they count. The README and the store page call a
//!   folder a real bus (it sums its children and runs a rack over the sum),
//!   and the free desk's limit counts folders, so "buses ... have no limit
//!   for anyone" beside "folders are real buses" promises a free user the
//!   folder past the limit, which the gate then refuses.
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

/// The em dash, refused on every surface (see the module doc).
const EM_DASH: char = '\u{2014}';

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
    for sentence in sentences(text).into_iter().filter(|s| s.contains(EM_DASH)) {
        hits.push(format!("has an em dash: {sentence:?}"));
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
    if says_buses_are_unlimited(text) {
        for sentence in sentences(text) {
            let low = sentence.to_lowercase();
            let folder_as_bus = calls_folders_buses(&low);
            // "Count" as a claim that they do: "counted" in "never counted"
            // is not the word, and a negated count is the opposite claim.
            let says_they_count = (has_word(&low, "count") || has_word(&low, "counts"))
                && !["not count", "never count", "n't count"].iter().any(|p| low.contains(p));
            if folder_as_bus && !says_they_count {
                hits.push(format!(
                    "calls folders buses without saying they count, beside \"buses have no limit\": {sentence:?}"
                ));
            }
        }
    }
    hits
}

/// Whether `sentence` (lowercase) says a folder IS a bus: "folders are real
/// buses", "folders that are real buses", "a folder acts as a bus". A
/// sentence that only names both ("a folder's send to the effects bus", "any
/// channel, folder, bus or the master") is not the claim, and the changelog
/// is full of those, so the words between must be a link ("is", "are", "as")
/// and its fillers, and nothing else.
fn calls_folders_buses(sentence: &str) -> bool {
    const FILLER: [&str; 10] = ["is", "are", "as", "that", "a", "real", "also", "act", "acts", "each"];
    let words: Vec<&str> = sentence.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).collect();
    words.iter().enumerate().filter(|(_, w)| matches!(**w, "folder" | "folders")).any(|(i, _)| {
        let rest = &words[i + 1..];
        let Some(gap) = rest.iter().position(|w| !FILLER.contains(w)) else {
            return false;
        };
        matches!(rest[gap], "bus" | "buses") && rest[..gap].iter().any(|w| matches!(*w, "is" | "are" | "as"))
    })
}

/// Whether some sentence of `text` says buses are unlimited or uncounted.
fn says_buses_are_unlimited(text: &str) -> bool {
    sentences(text).iter().any(|sentence| {
        let low = sentence.to_lowercase();
        has_word(&low, "buses")
            && ["no limit", "unlimited", "never counted", "not counted"].iter().any(|p| low.contains(p))
    })
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
/// joins its lines, `\"` is a quote. [`lex`]'s literals.
fn literals(src: &str) -> Vec<String> {
    lex(src).literals
}

/// **A source file, lexed for the scans**: its string literals outside
/// test code ([`literals`]), and its CODE outside test code, which is the
/// text with every comment, string and char literal and test item blanked
/// to spaces and every newline kept, so a scan reads only what ships and
/// still names the line it found something on (`ban_scan`).
pub(crate) struct Lexed {
    pub(crate) literals: Vec<String>,
    pub(crate) code: String,
}

/// **The one lexer both scans read through.**
///
/// A small lexer rather than a text search, because a text search reads the
/// comments (where a stale line is quoted on purpose) and cannot tell an
/// item under `#[cfg(test)]` from one that ships. It knows `//` and nested
/// `/* */` comments, normal, byte and raw strings, char literals (so `'"'`
/// opens nothing) and lifetimes. An item under a test cfg (`#[cfg(test)]`,
/// or an `all(..)` that needs `test`: `ban_scan::is_test_cfg`) is skipped
/// to its end: the `;` or the `}` that closes its first `{`, counted with
/// strings and comments still lexed so a brace inside one is not a brace.
pub(crate) fn lex(src: &str) -> Lexed {
    let s: Vec<char> = src.chars().collect();
    let n = s.len();
    let at = |i: usize, pat: &str| pat.chars().enumerate().all(|(k, c)| s.get(i + k) == Some(&c));
    let mut out = Vec::new();
    let mut code = String::with_capacity(src.len());
    // What a blanked stretch leaves in the code: its newlines, so every
    // line after it keeps its number.
    let blank = |code: &mut String, from: usize, to: usize| {
        code.extend(s[from..to.min(n)].iter().map(|c| if *c == '\n' { '\n' } else { ' ' }));
    };
    // `Some(depth)` while inside a skipped test item: the depth of `{` /
    // `(` / `[` reached in it, and whether its body has opened.
    let mut skipping: Option<(i32, bool)> = None;
    let mut i = 0;
    while i < n {
        let c = s[i];
        if at(i, "//") {
            let from = i;
            while i < n && s[i] != '\n' {
                i += 1;
            }
            blank(&mut code, from, i);
            continue;
        }
        if at(i, "/*") {
            let from = i;
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
            blank(&mut code, from, i);
            continue;
        }
        if at(i, "#[cfg(") && skipping.is_none() {
            // The attribute whole, to its `]`, strings inside it skipped.
            let mut j = i + 2;
            let mut depth = 1;
            while j < n && depth > 0 {
                match s[j] {
                    '"' => {
                        j += 1;
                        while j < n && s[j] != '"' {
                            j += if s[j] == '\\' { 2 } else { 1 };
                        }
                    }
                    '[' => depth += 1,
                    ']' => depth -= 1,
                    _ => {}
                }
                j += 1;
            }
            let attr: String = s[i..j.min(n)].iter().collect();
            if crate::ban_scan::is_test_cfg(&attr) {
                skipping = Some((0, false));
                blank(&mut code, i, j);
                i = j;
                continue;
            }
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
                let from = i;
                i = end + close.chars().count();
                blank(&mut code, from, i);
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
                        // `\u{2014}` is the character it names: a dash
                        // spelled as an escape is still a dash on screen.
                        'u' if s.get(j + 2) == Some(&'{') => {
                            let close = (j + 3..n).find(|&k| s[k] == '}').unwrap_or(n);
                            let hex: String = s[j + 3..close].iter().collect();
                            if let Some(ch) = u32::from_str_radix(&hex, 16).ok().and_then(char::from_u32) {
                                lit.push(ch);
                            }
                            j = close + 1;
                            continue;
                        }
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
            let from = i;
            i = j + 1;
            blank(&mut code, from, i);
            continue;
        }
        if c == '\'' {
            let from = i;
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
                code.push(if skipping.is_some() { ' ' } else { '\'' });
                continue;
            }
            blank(&mut code, from, i);
            continue;
        }
        let skipped = skipping.is_some();
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
        code.push(if skipped && c != '\n' { ' ' } else { c });
        i += 1;
    }
    Lexed { literals: out, code }
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

/// The changelog section that is still being written: `[Unreleased]` from
/// its heading to the next `## [`, or, when a cut renamed `[Unreleased]` to
/// the version (as the 2.3.0 cut did), the newest section, which is where
/// this copy then lives. Panicking on a missing `[Unreleased]` would refuse a
/// release for doing what a release does.
fn unreleased(changelog: &str) -> &str {
    let start = changelog
        .find("## [Unreleased]")
        .or_else(|| changelog.find("## ["))
        .expect("CHANGELOG.md has no `## [` section at all");
    let body = &changelog[start..];
    let end = body[3..].find("\n## [").map_or(body.len(), |e| e + 3);
    &body[..end]
}

/// The folder under `docs/release/` that holds the newest release's page,
/// from the folder names that hold a `GUMROAD.md`.
///
/// The page is written in `next/` and its own header says to rename the
/// folder to the version when the release is cut, so a fixed path to
/// `next/GUMROAD.md` would turn the release gate red the moment someone did
/// what the page says. Versions order numerically ("4.10" after "4.4"), and a
/// name that is not a version ("next") is the unreleased page and sorts above
/// every version. The older pages stay as the history of what each release's
/// page said, and are not scanned.
fn newest_release<'a>(names: &[&'a str]) -> Option<&'a str> {
    let version = |name: &str| -> Option<Vec<u32>> { name.split('.').map(|p| p.parse().ok()).collect() };
    names.iter().copied().max_by_key(|name| {
        let v = version(name);
        (v.is_none(), v.unwrap_or_default(), name.to_string())
    })
}

/// The newest release's Gumroad page, repository-relative. See
/// [`newest_release`].
fn gumroad_page(root: &Path) -> String {
    let dir = root.join("docs/release");
    let names: Vec<String> = std::fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("{}: {e}", dir.display()))
        .map(|e| e.expect("a directory entry").path())
        .filter(|p| p.join("GUMROAD.md").is_file())
        .filter_map(|p| p.file_name().and_then(|f| f.to_str()).map(str::to_owned))
        .collect();
    let refs: Vec<&str> = names.iter().map(String::as_str).collect();
    let newest = newest_release(&refs).expect("docs/release holds no GUMROAD.md page");
    format!("docs/release/{newest}/GUMROAD.md")
}

/// **How a shipped file is read: as it reaches the person it is for.**
///
/// What a person is HANDED is read whole: a licence, a README, a script the
/// download asks them to read before they run it, whose comments are part of
/// what they read. What a program parses or renders is read the way it shows
/// it: a comment that pkg, makensis, the desktop or the installer's web
/// view drops never reaches anyone, so it is this repo's to write like every
/// other comment, while an installer page's `&mdash;` is the mark itself on
/// the page.
#[derive(Clone, Copy, Debug)]
enum Reads {
    /// Every character.
    Whole,
    /// HTML or XML: `<!-- -->` comments and `<style>` and `<script>` blocks
    /// dropped, character references resolved ([`entities`]).
    Markup,
    /// A UCL manifest (pkg-create(8)): a `#` comment outside a quoted string
    /// or a heredoc value is dropped. pkg parses the file and keeps none.
    Ucl,
    /// A file read line by line whose comment lines (starting `;` or `#`)
    /// its reader drops: the NSIS script, where makensis compiles them away,
    /// and the desktop entry, where freedesktop's `#` lines are notes.
    CommentLines,
}

/// **Every file a download or an installer carries for a person to read or
/// see**, beside the program and the surfaces the phrase scan reads, with
/// how each is read. Where each one goes is named beside it; the script
/// that puts it there is the authority if the two ever disagree.
///
/// Not here, each for its reason: the licence texts nobody here wrote
/// (`LICENSE`'s MIT grant, the fonts' `OFL.txt`, the ffmpeg licences
/// fetched beside its binary), which are quoted law and not ours to reword;
/// and the macOS package's `postinstall`, which prints nothing and is shown
/// to no one.
const SHIPPED: [(&str, Reads); 12] = [
    // The licence page of the macOS installer and of the Windows one (as
    // LICENSE.txt), and the Linux tarball (build-installer.sh).
    ("LICENSING.md", Reads::Whole),
    // README.txt beside the app in the macOS zip and disk image, the Linux
    // tarballs and the Windows zip.
    ("docs/ARTIFACT-README.md", Reads::Whole),
    // Beside the program in every download.
    ("THIRD-PARTY-LICENSES", Reads::Whole),
    // The application menu's entry on Linux and FreeBSD.
    ("assets/tangent.desktop", Reads::CommentLines),
    // Handed over in the Linux tarballs to be read before it is run; its
    // usage and its progress lines are what the person sees while it runs.
    ("installer/linux/install.sh", Reads::Whole),
    // The same, in the FreeBSD bundles.
    ("installer/freebsd/install.sh", Reads::Whole),
    // On the user's PATH from the FreeBSD package, with its measurements in
    // its comments: "the knowledge ships with the program".
    ("installer/freebsd/tangent-jack", Reads::Whole),
    // The FreeBSD package's one-line summary, its description, and the
    // message pkg prints at the end of the install.
    ("installer/freebsd/MANIFEST.in", Reads::Ucl),
    // The macOS installer's first and last pages, and its title and choice.
    ("installer/macos/welcome.html", Reads::Markup),
    ("installer/macos/conclusion.html", Reads::Markup),
    ("installer/macos/distribution.xml.in", Reads::Markup),
    // The Windows installer's pages, sections and messages.
    ("installer/windows/tangent.nsi", Reads::CommentLines),
];

/// The folder every download carries whole: the notice that maps each font
/// the program embeds to its licence, and the licences, each headed by a
/// note of ours. Read file by file, so a licence added to it is read without
/// being listed.
const FONT_LICENCES: &str = "assets/font-licenses";

/// **Text a build script writes into a download**, read out of the script:
/// the script, the line the text follows, the line that ends it (matched
/// whole, as a heredoc's terminator and a function's closing brace stand at
/// the start of their line), and how it is read.
const WRITTEN: [(&str, &str, &str, Reads); 3] = [
    // The app's Info.plist: macOS shows its usage strings when the app first
    // asks for the camera and the microphone.
    ("scripts/build-macos.sh", "/Contents/Info.plist\" <<PLIST", "PLIST", Reads::Markup),
    // README.txt in the FreeBSD bundles.
    ("scripts/build-freebsd.sh", "write_readme() {", "}", Reads::Whole),
    // ffmpeg-licenses/SOURCE.txt in the Linux and Windows downloads.
    ("scripts/fetch-ffmpeg.sh", "provenance() {", "}", Reads::Whole),
];

/// What of `src` reaches the person it ships to, read as `how` says. Every
/// newline is kept, so line n of the result is line n of the file.
fn shipped_text(src: &str, how: Reads) -> String {
    match how {
        Reads::Whole => src.to_owned(),
        Reads::Markup => entities(&markup_shown(src)),
        Reads::Ucl => ucl_shown(src),
        Reads::CommentLines => src
            .split_inclusive('\n')
            .map(|line| if line.trim_start().starts_with([';', '#']) { blank(line) } else { line.to_owned() })
            .collect(),
    }
}

/// `text` with every character but a newline turned into a space.
fn blank(text: &str) -> String {
    text.chars().map(|c| if c == '\n' { '\n' } else { ' ' }).collect()
}

/// Markup without what a renderer never shows: comments, and the insides of
/// `<style>` and `<script>`. An unclosed one hides the rest of the page, as
/// it does on the page.
fn markup_shown(src: &str) -> String {
    const HIDDEN: [(&str, &str); 3] = [("<!--", "-->"), ("<style", "</style>"), ("<script", "</script>")];
    // ASCII lowercasing moves no byte, so an offset found in one is the
    // same offset in the other.
    let lower = src.to_ascii_lowercase();
    let mut out = String::with_capacity(src.len());
    let mut at = 0;
    while let Some((from, close)) = HIDDEN
        .iter()
        .filter_map(|&(open, close)| lower[at..].find(open).map(|i| (at + i, close)))
        .min_by_key(|&(from, _)| from)
    {
        let to = lower[from..].find(close).map_or(src.len(), |i| from + i + close.len());
        out.push_str(&src[at..from]);
        out.push_str(&blank(&src[from..to]));
        at = to;
    }
    out.push_str(&src[at..]);
    out
}

/// Character references resolved as far as the dash rule needs: the named
/// em and en dash, and every numeric one (`&#8212;`, `&#x2014;`). Any other
/// `&` is left as written.
fn entities(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find('&') {
        out.push_str(&rest[..at]);
        let tail = &rest[at..];
        let named = tail.find(';').filter(|&end| end <= 12).and_then(|end| {
            let ch = match &tail[1..end] {
                "mdash" => Some('\u{2014}'),
                "ndash" => Some('\u{2013}'),
                name => {
                    let number = name.strip_prefix('#')?;
                    let code = match number.strip_prefix(['x', 'X']) {
                        Some(hex) => u32::from_str_radix(hex, 16).ok(),
                        None => number.parse().ok(),
                    };
                    code.and_then(char::from_u32)
                }
            };
            ch.map(|c| (c, end))
        });
        match named {
            Some((c, end)) => {
                out.push(c);
                rest = &tail[end + 1..];
            }
            None => {
                out.push('&');
                rest = &tail[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// A UCL manifest as pkg keeps it: a `#` outside a quoted string starts a
/// comment that runs to the end of its line, and the lines of a heredoc
/// value (`message = <<EOM` to a line that is `EOM`) are text, whatever
/// they start with.
fn ucl_shown(src: &str) -> String {
    let mut out = String::with_capacity(src.len());
    let mut heredoc: Option<&str> = None;
    for line in src.split_inclusive('\n') {
        let body = line.trim_end_matches(['\n', '\r']);
        if let Some(tag) = heredoc {
            if body == tag {
                heredoc = None;
            }
            out.push_str(line);
            continue;
        }
        let (mut quoted, mut escaped) = (false, false);
        let cut = body
            .char_indices()
            .find(|&(_, c)| {
                let comment = c == '#' && !quoted;
                if escaped {
                    escaped = false;
                } else if c == '\\' {
                    escaped = true;
                } else if c == '"' {
                    quoted = !quoted;
                }
                comment
            })
            .map_or(body.len(), |(i, _)| i);
        let kept = &body[..cut];
        // libucl's terminator is capital letters and nothing else.
        heredoc = kept
            .split_once("<<")
            .map(|(_, tag)| tag.trim())
            .filter(|tag| !tag.is_empty() && tag.bytes().all(|b| b.is_ascii_uppercase()));
        out.push_str(kept);
        out.push_str(&blank(&line[cut..]));
    }
    out
}

/// The text a build script writes, out of the script: the lines after the
/// first line that contains `opens`, up to the next line that is exactly
/// `closes`, and the number of the first of them. `None` when the script
/// no longer has either.
fn fragment(src: &str, opens: &str, closes: &str) -> Option<(usize, String)> {
    let lines: Vec<&str> = src.lines().collect();
    let start = lines.iter().position(|l| l.contains(opens))? + 1;
    let len = lines[start..].iter().position(|l| *l == closes)?;
    Some((start + 1, lines[start..start + len].join("\n")))
}

/// Each line of `text` that has an em dash, as `number: line`, the first
/// line numbered `first`.
fn dash_lines(text: &str, first: usize) -> Vec<String> {
    text.lines()
        .enumerate()
        .filter(|(_, line)| line.contains(EM_DASH))
        .map(|(i, line)| format!("{}: {}", first + i, line.trim()))
        .collect()
}

/// **No surface says a key does less than it does.** See the module doc.
#[test]
fn no_surface_says_a_key_is_only_a_thank_you() {
    let root = root();
    let mut hits = Vec::new();
    let gumroad = gumroad_page(&root);
    for doc in ["README.md", "docs/STORE-CONTENT.md", gumroad.as_str()] {
        let text = std::fs::read_to_string(root.join(doc)).unwrap_or_else(|e| panic!("{doc}: {e}"));
        hits.extend(stale(&text).into_iter().map(|h| format!("{doc}: {h}")));
    }
    let changelog = std::fs::read_to_string(root.join("CHANGELOG.md")).expect("CHANGELOG.md");
    hits.extend(stale(unreleased(&changelog)).into_iter().map(|h| format!("CHANGELOG.md [Unreleased]: {h}")));
    let mut scanned = 0;
    for dir in ["tangent-ui/src", "tools/tangent-fulfil/src"] {
        let files = rust_files(&root.join(dir));
        // A test-only module is a whole file of assertions and fixtures,
        // which no user reads: the same reason a `#[cfg(test)]` item is
        // skipped inside a file.
        let tests = crate::ban_scan::test_only_files(&files);
        for file in files {
            if tests.contains(&file) || file.file_name().is_some_and(|f| f == "copy_scan.rs") {
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

/// **No string any other crate ships has an em dash** either: the desktop
/// host's notices ("Nothing is armed...", the export's and the IR
/// capture's lines), the engine's and the host's errors that reach a
/// toast, and the plugin's. Only the dash rule: these crates are not where
/// a key is described, and their literals name device widths ("a
/// 2-channel input") that the width rule would read as the desk.
/// `tangent-ui` and the fulfilment email are read whole by the scan above.
#[test]
fn no_shipped_string_in_any_crate_has_an_em_dash() {
    let root = root();
    let mut hits = Vec::new();
    let mut read = 0;
    for dir in ["tangent/src", "tangent-host/src", "tangent-record/src", "tangent-core/src", "plugin/src"] {
        let mut files = crate::ban_scan::rust_files(&root.join(dir));
        let tests = crate::ban_scan::test_only_files(&files);
        // A build script's strings ship when it writes them into the
        // program: tangent's writes the Windows version resource, whose
        // FileDescription is the line Task Manager and Properties show.
        let build = root.join(dir.strip_suffix("/src").expect("a crate's src")).join("build.rs");
        if build.is_file() {
            files.push(build);
        }
        for file in files.iter().filter(|f| !tests.contains(*f)) {
            read += 1;
            let src = std::fs::read_to_string(file).expect("a source file");
            for lit in literals(&src).into_iter().filter(|l| l.contains(EM_DASH)) {
                let rel = file.strip_prefix(&root).unwrap_or(file).display().to_string();
                hits.push(format!("{rel}: {lit:?}"));
            }
        }
    }
    // Not a vacuous pass: the five trees were found and read.
    assert!(read > 90, "the scan read only {read} files");
    assert!(hits.is_empty(), "a shipped string has an em dash:\n  {}", hits.join("\n  "));
}

/// **Nothing a download or an installer carries has an em dash** either:
/// the licence pages, the README and the licence notes beside the program,
/// the installers' pages and scripts, the FreeBSD package's message, and the
/// text the build scripts write into a download ([`SHIPPED`],
/// [`FONT_LICENCES`], [`WRITTEN`]). These were never read by a scan, and the
/// FreeBSD message, the licence page and both macOS installer pages (as
/// `&mdash;`) each had one. Only the dash rule, as for the other crates:
/// none of this says what a key does.
#[test]
fn nothing_the_downloads_and_installers_carry_has_an_em_dash() {
    let root = root();
    let read = |rel: &str| std::fs::read_to_string(root.join(rel)).unwrap_or_else(|e| panic!("{rel}: {e}"));
    let dir = root.join(FONT_LICENCES);
    let mut licences: Vec<String> = std::fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("{}: {e}", dir.display()))
        .map(|e| e.expect("a directory entry").path())
        .filter(|p| p.extension().is_some_and(|x| x == "txt"))
        .filter_map(|p| p.file_name().and_then(|f| f.to_str()).map(|f| format!("{FONT_LICENCES}/{f}")))
        .collect();
    licences.sort();
    // Not a vacuous pass: the notice and the six fonts' licences.
    assert!(licences.len() >= 7, "{FONT_LICENCES} holds only {licences:?}");
    let files = SHIPPED.iter().map(|&(rel, how)| (rel.to_owned(), how));
    let mut hits = Vec::new();
    for (rel, how) in files.chain(licences.into_iter().map(|rel| (rel, Reads::Whole))) {
        let text = shipped_text(&read(rel.as_str()), how);
        hits.extend(dash_lines(&text, 1).into_iter().map(|h| format!("{rel}:{h}")));
    }
    for (script, opens, closes, how) in WRITTEN {
        let (first, body) = fragment(&read(script), opens, closes).unwrap_or_else(|| {
            panic!("{script} no longer writes its text after {opens:?}: point this scan at where it went")
        });
        assert!(!body.trim().is_empty(), "{script} has nothing between {opens:?} and {closes:?}");
        let text = shipped_text(&body, how);
        hits.extend(dash_lines(&text, first).into_iter().map(|h| format!("{script}:{h}")));
    }
    assert!(hits.is_empty(), "text a download carries has an em dash:\n  {}", hits.join("\n  "));
}

/// **Each kind of shipped file is read as it reaches the person it is
/// for**: the mark is found wherever it shows, written as itself or as a
/// character reference, and nowhere a packager, a compiler or a renderer
/// drops. The scan above passing proves nothing unless this does too.
#[test]
fn a_shipped_file_is_read_as_it_reaches_the_person_it_is_for() {
    const D: char = EM_DASH;
    // An installer page: a comment and a style sheet are never shown, and
    // `&mdash;` is the mark on the page.
    let page = "<html><head><style>\n/* light \u{2014} on light */\n</style></head>\n<!-- a\n\u{2014} b -->\n\
                <p>one &mdash; two</p>\n<p>&#8212; &#x2014; &#X2014;</p>\n<p>&amp; &ndash; &bogus; & &#xZZ;</p>\n";
    let shown = shipped_text(page, Reads::Markup);
    assert_eq!(shown.lines().count(), page.lines().count(), "a line was lost or gained:\n{shown}");
    assert_eq!(dash_lines(&shown, 1), [format!("6: <p>one {D} two</p>"), format!("7: <p>{D} {D} {D}</p>")]);
    assert!(shown.contains("<p>&amp; \u{2013} &bogus; & &#xZZ;</p>"), "{shown}");
    // The FreeBSD manifest: a comment is dropped; a quoted `#` and a
    // heredoc's line that starts with one are what pkg shows.
    let manifest = "# a comment \u{2014} dropped\ncomment = \"a # kept \u{2014} here\"\nmessage = <<EOM\n\
                    # shown \u{2014} too\nEOM\nname = \"x\" # trailing \u{2014} dropped\n";
    let shown = shipped_text(manifest, Reads::Ucl);
    assert_eq!(shown.lines().count(), manifest.lines().count(), "a line was lost or gained:\n{shown}");
    assert_eq!(dash_lines(&shown, 1), [format!("2: comment = \"a # kept {D} here\""), format!("4: # shown {D} too")]);
    // The Windows installer and the desktop entry: comment lines are
    // dropped, and what they say is shown.
    let nsi = "; a comment \u{2014} dropped\n  # another \u{2014} dropped\nMessageBox MB_OK \"one \u{2014} two\"\n";
    let shown = shipped_text(nsi, Reads::CommentLines);
    assert_eq!(dash_lines(&shown, 1), [format!("3: MessageBox MB_OK \"one {D} two\"")]);
    let entry = "[Desktop Entry]\n# a note \u{2014} dropped\nComment=one \u{2014} two\n";
    let shown = shipped_text(entry, Reads::CommentLines);
    assert_eq!(dash_lines(&shown, 1), [format!("3: Comment=one {D} two")]);
    // A script handed over to be read: its comments are read with it.
    let script = "#!/bin/sh\n# read before running \u{2014} kept\necho done\n";
    assert_eq!(dash_lines(&shipped_text(script, Reads::Whole), 1), [format!("2: # read before running {D} kept")]);
    // Text a build script writes: from the line after the opener to the
    // terminator standing alone (an indented one ends nothing), numbered as
    // lines of the script.
    let build = "set -e\ncat > \"$APP/Contents/Info.plist\" <<PLIST\n<!-- a note \u{2014} not shown -->\n\
                 <string>asks \u{2014} for the camera</string>\n  PLIST\nPLIST\necho after \u{2014}\n";
    let (first, body) = fragment(build, "/Contents/Info.plist\" <<PLIST", "PLIST").expect("the plist is found");
    assert_eq!((first, body.lines().count()), (3, 3), "{body}");
    assert_eq!(
        dash_lines(&shipped_text(&body, Reads::Markup), first),
        [format!("4: <string>asks {D} for the camera</string>")]
    );
    assert_eq!(fragment(build, "write_readme() {", "}"), None);
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
        // The README's shape before it said folders count.
        "Buses, returns, sends and routes have no limit for anyone. Folders are real buses: a group sums its children.",
        // The Gumroad description's: the claim and the folder a section apart.
        "Buses, returns, sends and routes, with no limit. A DAW with folders that are real buses.",
        // Both in one sentence, and a folder's bay named a bus's.
        "Folders are buses and buses are never counted.",
        "Buses are unlimited. A folder acts as a bus over its children.",
        "Buses have no limit. Folders are buses and do not count.",
        // The em dash, wherever it stands in a sentence.
        "A key lifts the limit \u{2014} for every version.",
        "Thirty-three tracks\u{2014}and more.",
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
        // Folders called buses, saying they count, beside the unlimited claim.
        "Buses, returns, sends and routes have no limit for anyone. Folders are real buses and count as tracks: a group sums its children.",
        "Buses, returns, sends and routes, with no limit. A DAW with folders that are real buses (each counts as a track).",
        // Folders called buses on a surface that makes no claim about buses.
        "Folders are buses: a group sums its children.",
        // Buses unlimited on a surface that never calls a folder one.
        "Buses, returns, sends and routes have no limit for anyone. Save into the session's folder.",
        // Folders and buses named in one sentence, neither called the other.
        "Buses have no limit. Load it onto any channel, folder, bus or the master.",
        "Buses have no limit. A track in a folder is heard through the folder's send to the effects bus.",
        // A hyphen and an en dash are not the mark the rule is about.
        "Routing - Channel 3, the 2026\u{2013}2027 season.",
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
const ESCAPED: &str = "seven \u{2014} eight";
#[cfg(all(test, feature = "x"))]
const GATED: &str = "not shipped either";
const LAST: &str = "nine";
"##;
    assert_eq!(
        literals(src),
        vec!["one", "two \"quoted\"", "three four\nfive", "six", "seven \u{2014} eight", "nine"],
    );
    // And the CODE the ban scan reads: the same skips, every line kept.
    let code = lex(src).code;
    assert_eq!(code.lines().count(), src.lines().count(), "the code lost or gained a line");
    for kept in ["const SHIPPED", "const AFTER", "const ESCAPED", "const LAST", "fn a<'x>(q: &'x str)"] {
        assert!(code.contains(kept), "the code lost `{kept}`:\n{code}");
    }
    for gone in ["nothing more", "nested", "not an unlock", "TEST_ONLY", "GATED", "mod tests", "quoted", "eight"] {
        assert!(!code.contains(gone), "the code kept `{gone}`:\n{code}");
    }
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

/// **The page scanned is the newest release's, whatever the cut named it.**
/// `next` is scanned until the release renames it to its version, then that
/// version's folder is; a version orders numerically, and history is never
/// chosen over either.
#[test]
fn the_gumroad_page_scanned_is_the_newest_releases() {
    assert_eq!(newest_release(&["4.2", "4.4", "next"]), Some("next"));
    assert_eq!(newest_release(&["next", "4.4", "4.2"]), Some("next"));
    // The cut renamed `next` to its version.
    assert_eq!(newest_release(&["4.2", "4.4", "5.1"]), Some("5.1"));
    assert_eq!(newest_release(&["4.10", "4.4", "4.2"]), Some("4.10"));
    assert_eq!(newest_release(&["5.1.0", "5.1"]), Some("5.1.0"));
    assert_eq!(newest_release(&[]), None);
    // And the tree as it stands resolves to a page that exists.
    let root = root();
    let page = gumroad_page(&root);
    assert!(root.join(&page).is_file(), "{page} does not exist");
}

/// **A changelog cut that renamed `[Unreleased]` scans its newest section**
/// rather than panicking the release gate, and a kept `[Unreleased]` is
/// still the section scanned, ending at the next version.
#[test]
fn the_changelog_section_scanned_survives_a_cut() {
    let kept = "# Changelog\n\n## [Unreleased]\n\n- new.\n\n## [5.0.0] - 2026-08-31\n\n- old.\n";
    assert_eq!(unreleased(kept), "## [Unreleased]\n\n- new.\n");
    let renamed = "# Changelog\n\n## [5.1.0] - 2026-10-09\n\n- new.\n\n## [5.0.0] - 2026-08-31\n\n- old.\n";
    assert_eq!(unreleased(renamed), "## [5.1.0] - 2026-10-09\n\n- new.\n");
}
