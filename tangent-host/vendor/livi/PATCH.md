# `livi` 0.7.5, patched for aarch64

Upstream 0.7.5, unmodified except that six `i8`s are now `c_char` — in
`src/features/urid_map.rs` and one cast in `src/plugin.rs`.

Same bug as `tangent-host/vendor/lilv`, and the same reason: `c_char` is `i8`
on x86_64 and **`u8` on aarch64**, because C's plain `char` is unsigned on
ARM. The whole Rust LV2 stack was written on x86_64 and assumes the two are
interchangeable, so it builds on one Linux architecture and fails on the
other with type errors that name `*const u8` and read as a broken toolchain.

Behaviour on x86_64 is unchanged: `c_char` IS `i8` there, so this is the same
code. On aarch64 it is the difference between an artifact and no artifact.

## Scope

`[patch.crates-io]` is not target-conditional, so every platform resolves to
this copy — but `livi` is only in the dependency graph for `linux` and
`freebsd` (see `tangent-host/Cargo.toml`), so macOS and Windows never compile
it.

Upstream-able as written. If it lands, drop this directory and the
`[patch.crates-io]` line together.
