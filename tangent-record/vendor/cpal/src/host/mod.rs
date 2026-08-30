#[cfg(target_os = "android")]
pub(crate) mod aaudio;
#[cfg(any(
    target_os = "linux",
    target_os = "dragonfly",
    target_os = "freebsd",
    target_os = "netbsd"
))]
pub(crate) mod alsa;
#[cfg(all(windows, feature = "asio"))]
pub(crate) mod asio;
#[cfg(any(target_os = "macos", target_os = "ios"))]
pub(crate) mod coreaudio;
#[cfg(target_os = "emscripten")]
pub(crate) mod emscripten;
#[cfg(all(
    any(
        target_os = "linux",
        target_os = "dragonfly",
        target_os = "freebsd",
        target_os = "netbsd"
    ),
    feature = "jack"
))]
pub(crate) mod jack;
pub(crate) mod null;
// FreeBSD only, and not the other BSDs: the ioctl numbers and the
// `dev.pcm.N.%desc` sysctl this reads are FreeBSD's. DragonFly and NetBSD keep
// the ALSA-shim path until somebody with one of those machines says otherwise.
#[cfg(target_os = "freebsd")]
pub(crate) mod oss;
#[cfg(windows)]
pub(crate) mod wasapi;
#[cfg(all(target_arch = "wasm32", feature = "wasm-bindgen"))]
pub(crate) mod webaudio;
