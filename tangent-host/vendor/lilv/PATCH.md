# `lilv` 0.2.4, patched for aarch64

Upstream `lilv` 0.2.4 is unmodified here except for **two lines** in
`src/ui.rs` and **one deletion** in `src/plugin.rs`, and it is vendored for the
same reason `lv2-sys` is: the crate does not build otherwise on a platform we
ship.

## What is different

`lilv_ui_supported_func`'s two URI parameters were declared `*const i8`:

```rust
container_type_uri: *const i8,
ui_type_uri: *const i8,
```

They are now `*const c_char`.

## Why it matters

`c_char` is `i8` on x86_64 and **`u8` on aarch64**, because C's plain `char`
is unsigned on ARM. So the crate compiled on `x86_64-unknown-linux-gnu` and
failed on `aarch64-unknown-linux-gnu` with three `E0308`s naming `*const u8`
— which read as a broken toolchain rather than as a signedness assumption.
The rest of the crate already spells this `c_char`; these two were missed.

The third error is a consequence of the first two: the `Option<fn>` at
`ui.rs:75` is built from this signature, so fixing the parameters fixes it.

## The second change: a stray `#[must_use]`

`ClassIter`'s `Iterator::next` carried `#[must_use]` (`src/plugin.rs:775`).
The attribute is not legal on a trait method inside an `impl` block and does
nothing there; rustc has been warning about it under `unused_attributes` and
says plainly that it **"will become a hard error in a future release"**. The
line is deleted.

Nothing is lost by the deletion: `Iterator::next` already returns
`#[must_use] Option<T>`, so the call site is warned either way. This is the one
change here that is not about a platform — it is about the crate continuing to
compile at all on a later toolchain, and 0.2.4 is old enough that waiting for
an upstream release to fix it is not a plan.

The other 106 `#[must_use]` attributes in this crate are on inherent methods,
where the attribute is legal. Only this one is on a trait impl, which is why
only one warning ever appeared.

## Scope

`[patch.crates-io]` is not target-conditional, so every platform resolves to
this copy — but `lilv` is only in the dependency graph for `linux` and
`freebsd` (see `tangent-host/Cargo.toml`), so macOS and Windows never compile
it at all. On x86_64 the patched code is identical in behaviour: `c_char` IS
`i8` there.

Upstream-able as written. If it lands, drop this directory and the
`[patch.crates-io]` line together.
