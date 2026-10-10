//! **The ban scan: no live code names the fixed desk** (design W8).
//!
//! For its whole life before the unlimited desk the desk had a width:
//! `recorder::CHANNELS` sized every settings array, engine table and seat
//! index, and a family of names grew out of it, `STRIPS`, the seat space's
//! `TOTAL_SEATS` and `INSERT_SPACE`, the engine's `RACK_SEATS` and
//! `MASTER_AUTO_LANE`, and the reorder's `seat_order` and
//! `permute_channel_inserts`. Each wave took some of them away, and each
//! lane kept a scan of its own files while it did. This is the one scan over
//! every crate's shipped code, so none of them can come back anywhere:
//!
//! * **The names**, as a whole word. What still counts in the legacy shape
//!   says so in its name (`LEGACY_LANES`, `LEGACY_STRIPS` and `LEGACY_SEATS`
//!   in the engine, `desk_legacy::V32_*` everywhere), so a reader can tell a
//!   frozen file's number from a width of the desk. Two files keep an
//!   unrelated constant of their own under one of these names (a stereo
//!   ring's two channels, [`LOCAL`]); there only a path through `recorder`
//!   or `instrument`, or a `use` of one, is refused.
//! * **The lane clamps** `.min(31)` and `.min(63)`. A release build has no
//!   overflow check, and a clamp is the worse failure: `.min(31)` folds lane
//!   40 onto lane 31, so arming one arms the other, with no panic anywhere.
//!   A clamp that is not a lane says what bounds it ([`NOT_LANES`]).
//! * **The internal builds' switch** (`config::STAGING`), deleted with this
//!   wave; nothing may route a build to the staging twins again.
//! * **A home found off the one road** (W8-safe-tests): the `dirs` crate,
//!   or a read of `HOME` or `USERPROFILE`, anywhere but
//!   `tangent_core::config`, whose `home` a test build sends to a scratch
//!   folder. A second road would put a test back among a person's files.
//!   The two roads that are somebody else's are named by their function
//!   ([`OTHER_HOMES`]).
//!
//! **Live code is what ships**: comments, string and char literals, items
//! under `#[cfg(test)]` and files that are test-only modules are not read.
//! A test may name what it pins as gone, and a comment may say what a thing
//! used to be called, which is how this repo says why.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// The source roots of every crate that ships, repository-relative. The
/// vendored trees under them are upstream code and are not ours to scan.
const ROOTS: [&str; 6] =
    ["tangent/src", "tangent-ui/src", "tangent-core/src", "tangent-host/src", "tangent-record/src", "plugin/src"];

/// **The fixed desk's names.** Each was a width of the desk, or a seat or
/// an order worked out from one.
const BANNED: [&str; 8] = [
    "CHANNELS",
    "STRIPS",
    "TOTAL_SEATS",
    "RACK_SEATS",
    "INSERT_SPACE",
    "MASTER_AUTO_LANE",
    "seat_order",
    "permute_channel_inserts",
];

/// **Files that keep a constant of their own under a banned name**, and the
/// name: the oscilloscope's and the tap ring's interleaved stereo, two
/// channels, which no desk ever sized. In these files the bare name is
/// theirs, and only a path through `recorder` or `instrument` is refused.
const LOCAL: [(&str, &str); 2] = [("tangent-ui/src/scope.rs", "CHANNELS"), ("tangent-ui/src/tapring.rs", "CHANNELS")];

/// **The lane clamps**, as written with the whitespace taken out.
const CLAMPS: [&str; 2] = [".min(31)", ".min(63)"];

/// **Clamps that are not lanes**: the file, a word on the clamp's own line
/// that says what it bounds, and the fact that makes the bound right.
const NOT_LANES: [(&str, &str, &str); 3] = [
    ("tangent/src/dx7/synth.rs", "algorithm", "the DX7 has thirty-two algorithms"),
    ("tangent/src/dx7/edit.rs", "algorithm", "the DX7 has thirty-two algorithms"),
    ("tangent-host/src/instance.rs", "silenceFlags", "a VST3 bus's silence flags are one u64"),
];

/// The internal builds' switch, spelled in halves so that this file, which
/// looks for it, does not hold it.
fn staging() -> String {
    ["STAG", "ING"].concat()
}

/// The repository root.
fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..")
}

/// Every `.rs` file under `dir`, recursively, past any `vendor` directory.
pub(crate) fn rust_files(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let rd = std::fs::read_dir(&d).unwrap_or_else(|e| panic!("{}: {e}", d.display()));
        for e in rd {
            let p = e.expect("a directory entry").path();
            if p.is_dir() {
                if p.file_name().is_some_and(|n| n != "vendor") {
                    stack.push(p);
                }
            } else if p.extension().is_some_and(|x| x == "rs") {
                out.push(p);
            }
        }
    }
    out.sort();
    out
}

/// **Whether an attribute makes what follows it test-only**: `cfg(test)`,
/// or an `all(..)` that requires `test`. An `any(test, ..)` also builds
/// outside tests, and `not(test)` is the opposite, so neither counts.
pub(crate) fn is_test_cfg(attr: &str) -> bool {
    let a: String = attr.split_whitespace().collect();
    a.starts_with("#[cfg(test)]") || (a.starts_with("#[cfg(all(") && a.contains("test") && !a.contains("not(test"))
}

/// **The files of `files` that are test-only modules**, by the `mod`
/// declarations that bring them in: a `mod x;` under a test cfg (with a
/// `#[path]` honoured), and everything below one.
///
/// The lexer skips a test item INSIDE a file; a module declared test-only
/// in its parent (`mod era_corpus;` under `#[cfg(test)]` in `lib.rs`, the
/// app's `#[path = "app_width_tests.rs"]` suites, the engine's
/// `render_golden.rs`) is a whole file of test code that says nothing
/// about it at its top, so it has to be found from its declaration.
pub(crate) fn test_only_files(files: &[PathBuf]) -> BTreeSet<PathBuf> {
    let mut dirs: Vec<PathBuf> = Vec::new();
    let mut out = BTreeSet::new();
    for f in files {
        let src = std::fs::read_to_string(f).expect("a source file");
        let here = f.parent().expect("a file has a directory");
        let stem = f.file_stem().and_then(|s| s.to_str()).unwrap_or_default();
        // Where `mod x;` looks for `x`: beside a mod-rs file, in a directory
        // named for any other.
        let base = if matches!(stem, "lib" | "main" | "mod") { here.to_path_buf() } else { here.join(stem) };
        let mut attrs: Vec<&str> = Vec::new();
        for line in src.lines() {
            let t = line.trim();
            if t.starts_with("#[") || t.starts_with("///") || t.starts_with("//!") {
                attrs.push(t);
                continue;
            }
            let decl = t
                .strip_prefix("pub(crate) ")
                .or_else(|| t.strip_prefix("pub "))
                .unwrap_or(t)
                .strip_prefix("mod ")
                .and_then(|r| r.strip_suffix(';'));
            if let Some(name) = decl.map(str::trim) {
                if attrs.iter().any(|a| is_test_cfg(a)) {
                    let path = attrs.iter().find_map(|a| {
                        let a: String = a.split_whitespace().collect();
                        a.strip_prefix("#[path=\"").and_then(|r| r.strip_suffix("\"]")).map(str::to_owned)
                    });
                    match path {
                        Some(p) => {
                            out.insert(here.join(p));
                        }
                        None => {
                            out.insert(base.join(format!("{name}.rs")));
                            out.insert(base.join(name).join("mod.rs"));
                            dirs.push(base.join(name));
                        }
                    }
                }
            }
            attrs.clear();
        }
    }
    // A test-only module's own children are test-only too.
    for f in files {
        if dirs.iter().any(|d| f.starts_with(d)) || out.iter().any(|t| f.starts_with(t.with_extension(""))) {
            out.insert(f.clone());
        }
    }
    out.retain(|p| files.contains(p));
    out
}

/// **What is wrong with one file's live code**, one entry per hit: `code`
/// is the file as [`crate::copy_scan::lex`] leaves it (comments, literals
/// and test items blanked, lines kept), `rel` its repository path.
fn banned_in(rel: &str, code: &str) -> Vec<String> {
    let local: Vec<&str> = LOCAL.iter().filter(|(f, _)| *f == rel).map(|(_, n)| *n).collect();
    let mut hits = Vec::new();
    // Whether the statement this line is part of is a `use` through
    // `recorder` or `instrument`, which a list spreads over lines.
    let mut use_through = false;
    let mut in_use = false;
    for (n, line) in code.lines().enumerate() {
        let t = line.trim_start();
        if !in_use && (t.starts_with("use ") || t.starts_with("pub use ") || t.starts_with("pub(crate) use ")) {
            in_use = true;
            use_through = false;
        }
        if in_use {
            use_through |= has_word(line, "recorder") || has_word(line, "instrument");
        }
        let at = |what: &str| format!("{rel}:{}: {what}: {}", n + 1, line.trim());
        for name in BANNED {
            for (i, _) in word_at(line, name) {
                let before: String = line[..i].split_whitespace().collect();
                let qualified = before.ends_with("recorder::") || before.ends_with("instrument::");
                if !local.contains(&name) || qualified || (in_use && use_through) {
                    hits.push(at(name));
                }
            }
        }
        if !word_at(line, &staging()).is_empty() {
            hits.push(at("the internal builds' switch"));
        }
        let squashed: String = line.split_whitespace().collect();
        for clamp in CLAMPS {
            if squashed.contains(clamp)
                && !NOT_LANES.iter().any(|(f, word, _)| *f == rel && has_word(line, word))
            {
                hits.push(at(clamp));
            }
        }
        if in_use && line.contains(';') {
            in_use = false;
        }
    }
    hits
}

/// Where `word` stands in `text` as a whole identifier (no letter, digit or
/// `_` joined to either side), by byte offset.
fn word_at<'a>(text: &'a str, word: &'a str) -> Vec<(usize, &'a str)> {
    let joined = |c: Option<char>| c.is_some_and(|c| c.is_alphanumeric() || c == '_');
    text.match_indices(word)
        .filter(|(at, _)| !joined(text[..*at].chars().next_back()) && !joined(text[at + word.len()..].chars().next()))
        .collect()
}

fn has_word(text: &str, word: &str) -> bool {
    !word_at(text, word).is_empty()
}

/// **The one road to a home**: the file that asks the platform where a
/// person's home is (`tangent_core::config::home`, with `documents` and
/// `videos` beside it), which a test build turns into a scratch folder.
const HOME_ROAD: &str = "tangent-core/src/config.rs";

/// The environment variables a home is read from.
const HOME_VARS: [&str; 4] = ["HOME", "USERPROFILE", "HOMEDRIVE", "HOMEPATH"];

/// **A road to a home that is not ours to send to the scratch folder**, by
/// its file and the function it stands in: outside that function the file
/// is held to the rule like any other.
struct Kept {
    file: &'static str,
    func: &'static str,
    /// Whether shipped code outside the file may call it. One that may not
    /// stays exempt only while nothing does: the day something asks it for
    /// a folder to write in, it is a road like any other.
    called: bool,
    why: &'static str,
}

const OTHER_HOMES: [Kept; 2] = [
    Kept {
        file: "tangent-host/src/scan.rs",
        func: "fn search_paths(",
        called: true,
        why: "the VST3 SDK's own search paths: other people's plugins, read and never written",
    },
    Kept {
        file: "tangent-record/src/take.rs",
        func: "fn default_root(",
        called: false,
        why: "a take folder named and never touched, which no shipped code asks for \
              (`Settings::record_root` decides where takes go); the recorder links no tangent-core",
    },
];

/// **Where live code finds a home without the one road**, one entry per
/// hit: the `dirs` crate by path (every answer it has is a home or a folder
/// under one) and a read of [`HOME_VARS`]. `code` is `src` as the lexer
/// leaves it; the variable's name is a literal, so it is looked for on the
/// source line of a live `var`/`var_os` call.
fn home_roads_in(rel: &str, src: &str, code: &str) -> Vec<String> {
    if rel == HOME_ROAD {
        return Vec::new();
    }
    let kept: Vec<std::ops::Range<usize>> =
        OTHER_HOMES.iter().filter(|k| k.file == rel).filter_map(|k| fn_lines(code, k.func)).collect();
    let mut hits = Vec::new();
    for (n, (live, line)) in code.lines().zip(src.lines()).enumerate() {
        if kept.iter().any(|r| r.contains(&n)) {
            continue;
        }
        let at = |what: &str| format!("{rel}:{}: {what}: {}", n + 1, line.trim());
        if word_at(live, "dirs").iter().any(|(i, _)| live[i + "dirs".len()..].trim_start().starts_with("::")) {
            hits.push(at("the `dirs` crate"));
        }
        if has_word(live, "var") || has_word(live, "var_os") {
            for v in HOME_VARS.iter().filter(|v| line.contains(&format!("\"{v}\""))) {
                hits.push(at(v));
            }
        }
    }
    hits
}

/// The lines (0-based, end exclusive) of the function whose signature in
/// `code` begins with `sig`, through the `}` that closes its body.
fn fn_lines(code: &str, sig: &str) -> Option<std::ops::Range<usize>> {
    let start = code.find(sig)?;
    let open = start + code[start..].find('{')?;
    let mut depth = 0usize;
    let mut close = None;
    for (i, c) in code[open..].char_indices() {
        match c {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    close = Some(open + i);
                    break;
                }
            }
            _ => {}
        }
    }
    let line = |at: usize| code[..at].matches('\n').count();
    Some(line(start)..line(close?) + 1)
}

/// **No live code names the fixed desk**, in any crate that ships. See the
/// module doc for what is refused and why.
#[test]
fn no_live_code_names_the_fixed_desk() {
    let root = root();
    let mut files = Vec::new();
    for r in ROOTS {
        files.extend(rust_files(&root.join(r)));
    }
    let tests = test_only_files(&files);
    let mut hits = Vec::new();
    let mut read = [0usize; ROOTS.len()];
    for f in &files {
        let rel = f.strip_prefix(&root).unwrap_or(f).to_string_lossy().replace('\\', "/");
        if tests.contains(f) || rel == "tangent-ui/src/ban_scan.rs" {
            continue;
        }
        if let Some(k) = ROOTS.iter().position(|r| rel.starts_with(r)) {
            read[k] += 1;
        }
        let src = std::fs::read_to_string(f).expect("a source file");
        hits.extend(banned_in(&rel, &crate::copy_scan::lex(&src).code));
    }
    // Not a vacuous pass: every crate's tree was found and read (176 files
    // when this was written, thirteen of them test-only modules).
    assert!(read.iter().all(|n| *n > 0), "a crate was not read: {:?}", ROOTS.iter().zip(read).collect::<Vec<_>>());
    assert!(read.iter().sum::<usize>() > 150, "the scan read only {} files", read.iter().sum::<usize>());
    // The test-only modules were found too, or the scan was reading tests.
    assert!(tests.len() >= 10, "only {} test-only module files were found", tests.len());
    assert!(hits.is_empty(), "live code names the fixed desk:\n  {}", hits.join("\n  "));
}

/// **No live code finds a home but the one road** (design W8-safe-tests).
///
/// A test build sends `tangent_core::config::home` to a scratch folder,
/// because `cfg(test)` cannot reach the copies of tangent-core and
/// tangent-ui another crate's tests link; that keeps a test away from a
/// person's files only while every path to them is built on that road. So
/// shipped code in every crate asks for a home there and nowhere else: not
/// through the `dirs` crate, and not by reading `HOME` or `USERPROFILE`.
/// [`OTHER_HOMES`] names the two roads that are somebody else's, each by
/// its function.
#[test]
fn no_live_code_finds_a_home_but_the_one_road() {
    let root = root();
    let mut files = Vec::new();
    for r in ROOTS {
        files.extend(rust_files(&root.join(r)));
    }
    let tests = test_only_files(&files);
    let mut hits = Vec::new();
    let mut read = 0usize;
    let mut road = false;
    for f in &files {
        let rel = f.strip_prefix(&root).unwrap_or(f).to_string_lossy().replace('\\', "/");
        if tests.contains(f) || rel == "tangent-ui/src/ban_scan.rs" {
            continue;
        }
        read += 1;
        let src = std::fs::read_to_string(f).expect("a source file");
        let code = crate::copy_scan::lex(&src).code;
        road |= rel == HOME_ROAD && code.contains("dirs::home_dir()");
        hits.extend(home_roads_in(&rel, &src, &code));
    }
    assert!(read > 150, "the scan read only {read} files");
    assert!(road, "{HOME_ROAD} no longer asks `dirs` for the home: the one road moved, so move HOME_ROAD");
    assert!(
        hits.is_empty(),
        "live code finds a home without tangent_core::config::home, so a test build's scratch folder \
         does not cover it:\n  {}",
        hits.join("\n  ")
    );
}

/// **Every exemption still stands on something.** A local constant that
/// was renamed, or a clamp that was taken out, would leave its exemption
/// excusing whatever came next under the same name.
#[test]
fn every_exemption_names_what_is_still_there() {
    let root = root();
    for k in OTHER_HOMES {
        let src = std::fs::read_to_string(root.join(k.file)).expect("a kept road's file");
        let code = crate::copy_scan::lex(&src).code;
        let lines = fn_lines(&code, k.func).unwrap_or_else(|| panic!("{} no longer has `{}`", k.file, k.func));
        // The function as the scan would read it with no exemption: its
        // lines alone, everything else blanked, so a hit is one of its own.
        let only = |text: &str| -> String {
            text.lines()
                .enumerate()
                .map(|(n, l)| if lines.contains(&n) { l } else { "" })
                .collect::<Vec<_>>()
                .join("\n")
        };
        let unkept = home_roads_in("tangent/src/x.rs", &only(&src), &only(&code));
        assert!(!unkept.is_empty(), "{}'s `{}` no longer finds a home ({}): drop the exemption", k.file, k.func, k.why);
        if !k.called {
            let name = k.func.trim_start_matches("fn ").trim_end_matches('(');
            let mut files = Vec::new();
            for r in ROOTS {
                files.extend(rust_files(&root.join(r)));
            }
            let tests = test_only_files(&files);
            for f in files.iter().filter(|f| !tests.contains(*f) && !f.ends_with(k.file)) {
                let other = crate::copy_scan::lex(&std::fs::read_to_string(f).expect("a source file")).code;
                assert!(
                    !has_word(&other, name),
                    "{} calls `{name}`, so it is a road to a home like any other: build it on \
                     tangent_core::config::home, or say why it is kept",
                    f.display()
                );
            }
        }
    }
    for (file, name) in LOCAL {
        let src = std::fs::read_to_string(root.join(file)).expect("a local constant's file");
        assert!(
            src.lines().any(|l| l.trim_start().starts_with(&format!("const {name}: usize"))),
            "{file} no longer keeps its own `{name}`"
        );
    }
    for (file, word, why) in NOT_LANES {
        let src = std::fs::read_to_string(root.join(file)).expect("a clamp's file");
        let code = crate::copy_scan::lex(&src).code;
        assert!(
            code.lines().any(|l| {
                let squashed: String = l.split_whitespace().collect();
                has_word(l, word) && CLAMPS.iter().any(|c| squashed.contains(c))
            }),
            "{file} no longer clamps its {word} ({why}): drop the exemption"
        );
    }
}

/// **Each ban bites when planted**, bare and through a path, in one line
/// and in a `use` list over several, and a comment, a literal, a test item
/// and a name that only contains a banned one do not.
#[test]
fn every_banned_name_and_clamp_fails_the_scan_when_planted() {
    let scan = |rel: &str, src: &str| banned_in(rel, &crate::copy_scan::lex(src).code);
    for name in BANNED {
        for plant in [
            format!("let n = {name};"),
            format!("let n = crate::recorder::{name} + 1;"),
            format!("use tangent_ui::recorder::{{Strip, {name}}};"),
            format!("use crate::instrument::{{\n    Engine,\n    {name},\n}};"),
        ] {
            assert_eq!(scan("tangent/src/x.rs", &plant).len(), 1, "missed: {plant}");
        }
    }
    let switch = format!("if tangent_core::config::{} {{ return; }}", staging());
    assert_eq!(scan("tangent/src/x.rs", &switch).len(), 1, "missed the switch");
    for clamp in ["lane.min(31)", "ch . min( 63 )", "strip.min(31) as u8"] {
        assert_eq!(scan("tangent/src/x.rs", &format!("let l = {clamp};")).len(), 1, "missed: {clamp}");
    }
    // A local stereo `CHANNELS` is its file's own; a path through the desk's
    // homes is not, even there.
    assert!(scan("tangent-ui/src/scope.rs", "const CHANNELS: usize = 2; let a = x * CHANNELS;").is_empty());
    assert_eq!(scan("tangent-ui/src/scope.rs", "let n = recorder::CHANNELS;").len(), 1);
    assert_eq!(scan("tangent-ui/src/scope.rs", "use crate::recorder::{CHANNELS, Strip};").len(), 1);
    assert_eq!(scan("tangent-ui/src/scope.rs", "use crate::recorder::{\n    Strip,\n    CHANNELS,\n};").len(), 1);
    // A clamp that is not a lane, where it says so; and only there.
    assert!(scan("tangent/src/dx7/synth.rs", "let a = usize::from(v.algorithm.min(31));").is_empty());
    assert_eq!(scan("tangent/src/dx7/synth.rs", "let lane = ch.min(31);").len(), 1);
    // Not hits: a comment, a literal, a test item, a longer name, a bigger
    // number, and the names that say they are the legacy shape.
    for fine in [
        "// it was `CHANNELS`, and `x.min(31)` folded lane 40 onto 31",
        "/* STRIPS */ let s = \"TOTAL_SEATS and .min(63)\";",
        "#[cfg(test)]\nmod tests {\n    const RACK_SEATS: usize = 1;\n    fn f() { let _ = x.min(31); }\n}",
        "#[cfg(all(test, feature = \"recorder\"))]\nfn f() { let _ = MASTER_AUTO_LANE; }",
        "let n = V32_CHANNELS + LEGACY_STRIPS + LEGACY_SEATS + MAX_CHANNELS_SEEN;",
        "let n = x.min(310) + y.min(6) + z.min(631);",
        "fn seat_order_of() {} fn permute_channel_inserts_was() {}",
    ] {
        assert!(scan("tangent/src/x.rs", fine).is_empty(), "flagged: {fine}: {:?}", scan("tangent/src/x.rs", fine));
    }
}

/// **A test-only module is found from its declaration**, with or without a
/// `#[path]`, under `cfg(test)` or an `all(..)` that needs it, and its own
/// children with it; a module built outside tests is not.
#[test]
fn test_only_modules_are_found_from_their_declarations() {
    let dir = std::env::temp_dir().join(format!("tangent-ban-scan-{}-{:?}", std::process::id(), std::thread::current().id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("suite")).expect("a scratch tree");
    let files = [
        (
            "lib.rs",
            "pub mod shipped;\n/// Doc.\n#[cfg(test)]\nmod corpus;\n#[cfg(all(test, feature = \"x\"))]\nmod suite;\n\
             #[cfg(any(test, feature = \"x\"))]\nmod either;\n#[cfg(not(test))]\nmod release;\n",
        ),
        ("shipped.rs", "#[cfg(test)]\n#[path = \"shipped_tests.rs\"]\nmod tests;\n"),
        ("shipped_tests.rs", ""),
        ("corpus.rs", ""),
        ("suite/mod.rs", "mod inner;\n"),
        ("suite/inner.rs", ""),
        ("either.rs", ""),
        ("release.rs", ""),
    ];
    let mut paths = Vec::new();
    for (name, text) in files {
        std::fs::write(dir.join(name), text).expect("a scratch file");
        paths.push(dir.join(name));
    }
    paths.sort();
    let found: Vec<String> = test_only_files(&paths)
        .iter()
        .map(|p| p.strip_prefix(&dir).expect("under the scratch tree").to_string_lossy().replace('\\', "/"))
        .collect();
    assert_eq!(found, ["corpus.rs", "shipped_tests.rs", "suite/inner.rs", "suite/mod.rs"]);
    let _ = std::fs::remove_dir_all(&dir);
}

/// **A home found off the one road fails the scan** (design
/// W8-safe-tests): the `dirs` crate by path, in a call, a function handed
/// over or a `use`, and a read of each home variable. The road itself may;
/// a kept road may only inside the function it names; a comment, a
/// literal, a test item, a crate whose name only ends in the word and a
/// literal that only says "HOME" are not hits.
#[test]
fn a_home_found_off_the_one_road_fails_the_scan_when_planted() {
    let scan = |rel: &str, src: &str| home_roads_in(rel, src, &crate::copy_scan::lex(src).code);
    for plant in [
        "let h = dirs::home_dir();",
        "let d = dirs::document_dir().or_else(dirs::home_dir).unwrap();",
        "use dirs::video_dir;",
        "let h = dirs :: home_dir();",
        "let h = std::env::var_os(\"HOME\");",
        "let h = std::env::var(\"USERPROFILE\").ok();",
        "let d = env::var_os(\"HOMEDRIVE\");",
        "let p = env::var_os(\"HOMEPATH\");",
    ] {
        assert_eq!(scan("tangent/src/x.rs", plant).len(), 1, "missed: {plant}");
    }
    assert!(scan(HOME_ROAD, "let h = dirs::home_dir().or_else(|| std::env::var_os(\"HOME\"));").is_empty());
    let kept = "pub fn search_paths() -> Vec<PathBuf> {\n    let h = std::env::var_os(\"HOME\");\n    Vec::new()\n}\n";
    assert!(scan("tangent-host/src/scan.rs", kept).is_empty(), "the kept road was flagged");
    let beside = format!("{kept}fn beside() {{\n    let h = dirs::home_dir();\n}}\n");
    assert_eq!(scan("tangent-host/src/scan.rs", &beside).len(), 1, "an exemption reached past its function");
    assert_eq!(scan("tangent/src/x.rs", kept).len(), 1, "an exemption reached past its file");
    for fine in [
        "// dirs::home_dir() was the road, and var_os(\"HOME\") beside it",
        "let s = \"dirs::home_dir()\";",
        "#[cfg(test)]\nfn t() { let _ = dirs::home_dir(); let _ = std::env::var_os(\"HOME\"); }",
        "let h = subdirs::home_dir(); let d = my_dirs::x();",
        "let root = (\"HOME\".to_owned(), path);",
    ] {
        assert!(scan("tangent/src/x.rs", fine).is_empty(), "flagged: {fine}: {:?}", scan("tangent/src/x.rs", fine));
    }
}
