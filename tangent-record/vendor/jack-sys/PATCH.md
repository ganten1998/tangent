# `jack-sys` 0.5.1, patched so that `dynamic_loading` actually means it

Upstream `jack-sys` 0.5.1 is unmodified here except for **one conditional** in
`build.rs`. It is vendored for the reason `lv2-sys` and `lilv` are: without the
change, a platform we ship cannot have the feature at all.

## What is different

Upstream probes for libjack unconditionally and then makes the result fatal on
Linux:

```rust
let library_found = pkg_config::find_library("jack");
if target_os.as_ref().map(|s| s.as_str()) == Ok("linux") {
    library_found.unwrap();
}
```

Both lines now sit inside `if std::env::var_os("CARGO_FEATURE_DYNAMIC_LOADING").is_none()`.

## Why it matters

`dynamic_loading` — which is a DEFAULT feature of the `jack` crate, and the
only reason Tangent can offer JACK at all — reaches every libjack function
through `libloading` at run time. Nothing in the crate references a libjack
symbol at link time when it is on. So on Linux the probe did two harmful
things and no useful one:

1. **It put libjack back in `DT_NEEDED`.** `pkg_config::find_library` prints
   `cargo:rustc-link-lib=jack` when it succeeds. A binary built on a machine
   that happens to have jack development files installed therefore refuses to
   START on a machine without libjack — which is the precise failure
   `dynamic_loading` exists to prevent, arriving silently, on a build that
   looked fine. Tangent's Linux artifact must run on a stock desktop with no
   pro-audio stack installed; that is not negotiable, and it cannot be left
   depending on which packages the build host has.
2. **It made the build require jack development files** for a library the
   build never opens. The bindings in `build.rs` are a hardcoded table, not
   bindgen output, so no header is read on any path. This mattered beyond
   convenience: the Linux artifacts are cross-built, and a sysroot would have
   been needed for a link that does not happen.

Upstream already ignores the probe's result on macOS, Windows and the BSDs.
This extends that to the one case where the result is not merely uninteresting
but wrong.

Verified: `readelf -d` on the Linux binary lists `libc`, `libgcc_s` and the
LV2 stack, and **no `libjack.so.0`** — while `jack_sys::library()` opens
`libjack.so.0` at run time when one is installed.

## Scope

`[patch.crates-io]` is not target-conditional, so every platform resolves to
this copy — but `jack-sys` is only in the dependency graph where `cpal`'s
`jack` feature is on, which is `linux` and `freebsd` (see
`tangent-record/Cargo.toml`). macOS and Windows never compile it.

With `dynamic_loading` OFF the patched build.rs runs exactly the upstream code,
so a static-linking consumer is unaffected.

Upstream-able as written. If it lands, drop this directory and the
`[patch.crates-io]` line together.
