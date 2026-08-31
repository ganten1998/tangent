# Vendored `lv2-sys`

`lv2-sys/` is [`lv2-sys` 2.0.0](https://crates.io/crates/lv2-sys) (ISC) with
**one line added and nothing else changed**.

## Why

`lv2-sys` ships pre-generated bindings and selects them from a table:

```rust
#[cfg_attr(target_os = "linux", path = "linux/mod.rs")]
#[cfg_attr(target_os = "windows", path = "windows.rs")]
mod unsupported;
```

Anything not in that table gets `unsupported.rs`, which is a single
`compile_error!("The targeted operation system is not supported by rust-lv2!")`.
So on FreeBSD the crate does not build, and since `livi` → `lilv` → `lv2-sys`,
**no LV2 host can be built on the platform at all** — which is the platform
where LV2 is very nearly the only plugin format that exists.

## The change

```rust
#[cfg_attr(target_os = "freebsd", path = "linux/mod.rs")]
```

FreeBSD reads the same bindings as Linux, and that is correct rather than a
shortcut. `src/linux/` contains **no Linux-specific code**: it is a pure
`target_arch` dispatch over four files (`x86.rs`, `x86_64.rs`, `arm.rs`,
`aarch64.rs`) of bindgen output from the LV2 C headers — plain declarations of
structs and function pointers, with zero references to Linux, glibc, `/proc`,
epoll or anything else platform-bound, and no size assertions. FreeBSD/amd64
shares the SysV AMD64 C ABI with Linux/x86_64, so the same bindings describe the
same ABI. The directory is really "generic Unix, by architecture" and is only
called `linux` because those were the platforms upstream had tested.

## Verified

Against the real stack on FreeBSD 15, not by inspection alone:

* `eg-amp` — control port discovered with its declared range (−90..24 dB), and
  the gain applied exactly: 0 dB → 0.5000, +6 dB → 0.9976, −6 dB → 0.2506.
* `eg-fifths` — a MIDI note in through an atom sequence comes back as the note
  *and* the note a fifth above, so URID mapping, atom construction and the
  event round trip all work.
* Dragonfly Hall Reverb 3.2.10, a real third-party stereo effect: 18 control
  ports, and a decaying tail after an impulse.

## Upstream

This should go back: <https://github.com/RustAudio/rust-lv2>. The change is one
`cfg_attr` and needs no new bindings, since the existing ones already apply.
When it lands, delete this directory and drop the `exclude` and
`[patch.crates-io]` lines from the workspace root `Cargo.toml`.

Note that upstream `lv2-sys` has not been released since 2020, so the patch may
outlive the wait. It costs one line to carry.

## Why `Cargo.toml.orig` and `.cargo_vcs_info.json` are here

Every copy under this directory (`lilv/`, `livi/`, `lv2-sys/`) is an unpacked
crates.io `.crate` tarball, and those two files are part of what the tarball
ships. They are kept on purpose, not forgotten: the whole discipline of these
copies is that each one stays diffable against its upstream tarball, byte for
byte outside the declared patch, and stripping packaging files would make
every such diff open with noise instead of nothing. Do not tidy them away.
