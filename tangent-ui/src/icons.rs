//! The instrument set, and the one way anything draws it.
//!
//! The drawings are openly licensed artwork the owner sourced and has full
//! rights to use. They were not drawn in-house, and nothing here should say
//! they were.
//!
//! Thirty-five drawings, embedded as raw coverage and painted as MASKS: the
//! file says where the ink goes and the CALLER says what colour it is. That
//! separation is not a nicety here. Every badge these replace was a vector
//! drawn with `ink` handed down from whatever surface it landed on, because
//! this app's themes move the ground out from under a glyph — sixteen desks
//! and a dark mode for each — and a picture with a colour baked into it goes
//! invisible on some of them. A mask cannot: it arrives colourless and is
//! tinted with the ink the caller already measured against its own surface.
//!
//! **Why the coverage is raw.** `tangent-ui` depends on egui and on nothing
//! else, deliberately - its `Cargo.toml` argues that what is absent from that
//! list matters more than what is present - so there is no PNG decoder in the
//! GUI and there is not going to be one for a picture of a trumpet. The
//! decoding happens offline in `scripts/make-icons.py`, which is also where a
//! redrawn icon is regenerated, and what ships is the result: 192 by 192
//! bytes, one per pixel, row-major, no header. `include_bytes!` picks it up
//! and there is nothing left to parse at startup.
//!
//! **Why a truncated file cannot reach the screen.** [`Icon::coverage`] is
//! typed `&'static [u8; BYTES]`, not `&[u8]`, so a file of the wrong length
//! fails to compile with the path in the error. A length check at runtime
//! would be a check somebody has to remember to run; this one cannot be
//! skipped, and a half-written asset can never draw a smear.
//!
//! **Why 192 and not more.** These are only ever minified: the biggest place
//! an icon lands is the Track menu's picker, twenty-eight points, which is
//! seventy device pixels on a 2x display. egui hands the GPU one texture and
//! asks for bilinear filtering, and bilinear takes a 2x2 sample - it is a
//! good downscale by a factor of two or three and a sparkly one by a factor
//! of seven. 192 keeps the worst real case under 3x. A 512-pixel source would
//! look WORSE on screen, not better, and cost eight times the binary.

use egui::{Color32, ColorImage, Painter, Rect, TextureHandle, TextureOptions, Vec2};

/// The side of every embedded block, in pixels. `scripts/make-icons.py`
/// writes this and nothing reads a size out of the file, because there is no
/// header in the file to read one out of.
pub const SIDE: usize = 192;

/// How long every asset is. See the module note on why this is a TYPE.
const BYTES: usize = SIDE * SIDE;

/// The whole set, and the file each one comes from.
///
/// The variant names are the owner's own file names in Rust's spelling, so
/// that a drawing, its `.a8`, its `.png` in Dropbox and the arm that draws it
/// can all be found by grepping one word.
macro_rules! icon_set {
    ($($variant:ident => $file:literal,)*) => {
        /// One drawing from the set. Copy, because it is an index in
        /// everything but name and gets passed around like one.
        #[derive(Clone, Copy, PartialEq, Eq, Debug)]
        pub enum Icon {
            $(
                #[doc = concat!("`assets/icons/", $file, ".a8`")]
                $variant,
            )*
        }

        impl Icon {
            /// Every drawing, in the order they are declared. What the tests
            /// sweep and what proves the code table reaches all of them.
            pub const ALL: &'static [Self] = &[$(Self::$variant,)*];

            /// The file this drawing came from, without the extension.
            ///
            /// Doubles as the cache key, which is why it has to stay unique -
            /// two icons sharing a slug would share a texture and one of them
            /// would draw as the other.
            pub const fn slug(self) -> &'static str {
                match self { $(Self::$variant => $file,)* }
            }

            /// The raw coverage, one byte per pixel, top row first.
            const fn coverage(self) -> &'static [u8; BYTES] {
                match self {
                    $(Self::$variant => include_bytes!(
                        concat!("../../assets/icons/", $file, ".a8")
                    ),)*
                }
            }
        }
    };
}

icon_set! {
    Accordion => "accordion",
    Acoustic => "acoustic",
    Amp => "amp",
    Cabasa => "cabasa",
    Classical => "classical",
    Condenser => "condenser",
    DigitalWave => "digital-wave",
    Drumset => "drumset",
    Dynamic => "dynamic",
    EBass => "ebass",
    EgHumbucker => "eg-humbucker",
    EgSingleCoil => "eg-single-coil",
    Flute => "flute",
    Guiro => "guiro",
    Harmonica => "harmonica",
    Harp => "harp",
    Instrument => "instrument",
    Keyboard => "keyboard",
    LineIn => "line-in",
    Maracas => "maracas",
    Melodica => "melodica",
    Metronome => "metronome",
    Piano => "piano",
    Saxophone => "saxophone",
    Sitar => "sitar",
    Snare => "snare",
    Synthesizer => "synthesizer",
    Tambourine => "tambourine",
    Triangle => "triangle",
    Trombone => "trombone",
    Trumpet => "trumpet",
    Tuba => "tuba",
    UprightBass => "upright-bass",
    Violin => "violin",
    Wave => "wave",
    // **The second set**, nine drawings: seven percussion and two the app
    // itself reaches for. `Folder` is what a group wears when nobody has
    // chosen otherwise and `Master` is the master strip's own, so those two
    // are named in `mixer_panel::auto_icon` as well as offered in the picker.
    //
    // `Mariba` is the owner's own file name — see the note on this macro
    // about spellings — and the picker calls it "marimba", which is the
    // instrument.
    Djembe => "djembe",
    Folder => "folder",
    Kalimba => "kalimba",
    Mariba => "mariba",
    Master => "master",
    Vibraphone => "vibraphone",
    Vibraslap => "vibraslap",
    Xylophone1 => "xylophone1",
    Xylophone2 => "xylophone2",
}

/// **Where a drawing really lands inside the rect it was given.**
///
/// The assets are square, so fitting one to a rect means the largest centred
/// square that rect contains: a tall rect letterboxes top and bottom, a wide
/// one left and right, and NOTHING is ever stretched. A stretched icon is the
/// failure that reads as a bug even to somebody who has never seen the right
/// one — a circular tambourine drawn as an ellipse looks broken, not
/// resized.
///
/// Exposed rather than kept private because it is also the answer to "will
/// this fit": the strip's layout and the tests both ask it what a rect will
/// actually be covered by, instead of each re-deriving the same centring and
/// eventually disagreeing about it.
///
/// Built from the seat's own EDGES and then clamped back inside it. The
/// obvious spelling - centre plus or minus half a side - is wrong by up to an
/// ulp, because the centre is itself a division: on a square seat it produced
/// a square four millionths of a point outside the rect it was fitted to.
/// Nobody would ever see that, which is exactly why it is worth removing:
/// "nothing is cut off" is the owner's acceptance test, and a guarantee that
/// holds to within a rounding error is a guarantee somebody has to argue
/// about later.
pub fn fitted(rect: Rect) -> Rect {
    let side = rect.width().min(rect.height());
    if !side.is_finite() || side <= 0.0 {
        // `Rect::NOTHING` is how this codebase says "not on screen", and the
        // strip hands its badge rect straight through, so this arrives on
        // every master strip in the app.
        return Rect::NOTHING;
    }
    Rect::from_min_size(
        egui::Pos2::new(
            rect.left() + (rect.width() - side) * 0.5,
            rect.top() + (rect.height() - side) * 0.5,
        ),
        Vec2::splat(side),
    )
    .intersect(rect)
}

/// **Draw `icon` into `rect`, in `ink`.**
///
/// The texture is built the first time this icon is asked for on this context
/// and kept for the life of it. Rebuilding per frame would upload a megabyte
/// and a half every time the mixer redrew; `TextureHandle` is a reference into
/// the context's own texture manager and clones for the price of an `Arc`, so
/// the cached thing is a handle and the context owns the pixels.
///
/// It is cached in the CONTEXT's memory rather than in a static. A plugin host
/// can open several editors at once and each gets its own `egui::Context` with
/// its own texture manager; a texture id from one is meaningless in another,
/// and a static cache would hand the second editor the first one's ids and
/// draw whatever happened to be at those slots. Keying on the context makes
/// that unrepresentable rather than merely unlikely.
///
/// The two `data_mut` calls are deliberately not one: `load_texture` takes the
/// context's own lock, so building the texture inside the closure that holds
/// the memory lock deadlocks. Look, release, build, insert.
pub fn draw(painter: &Painter, rect: Rect, ink: Color32, icon: Icon) {
    let dst = fitted(rect);
    if !dst.is_positive() {
        return;
    }
    let ctx = painter.ctx();
    let id = egui::Id::new(("tangent::icons", icon.slug()));
    let handle = match ctx.data_mut(|d| d.get_temp::<TextureHandle>(id)) {
        Some(h) => h,
        None => {
            // A mask texel is PREMULTIPLIED white at the coverage: egui
            // multiplies the texture by the vertex colour, so white times
            // `ink` is `ink` and coverage times full alpha is coverage.
            // `from_white_alpha` is exactly `[a, a, a, a]`, which is what
            // premultiplied white at alpha `a` means.
            let pixels = icon.coverage().iter().map(|&a| Color32::from_white_alpha(a));
            let image = ColorImage::new([SIDE, SIDE], pixels.collect());
            let h = ctx.load_texture(icon.slug(), image, TextureOptions::LINEAR);
            ctx.data_mut(|d| d.insert_temp(id, h.clone()));
            h
        }
    };
    painter.image(
        handle.id(),
        dst,
        Rect::from_min_max(egui::Pos2::ZERO, egui::Pos2::new(1.0, 1.0)),
        ink,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::{Pos2, Rect, Vec2};

    /// Every asset is there and is exactly the size the format promises.
    ///
    /// The type on [`Icon::coverage`] already refuses to compile against a
    /// truncated file, so this cannot fail while the crate builds - which is
    /// the point of writing it down: it says what the invariant IS, and it
    /// fails the moment somebody relaxes that signature to `&[u8]` for
    /// convenience and reopens the smear.
    #[test]
    fn every_asset_is_a_full_block_of_coverage() {
        assert_eq!(Icon::ALL.len(), 44, "the set is forty-four drawings");
        for icon in Icon::ALL {
            assert_eq!(
                icon.coverage().len(),
                SIDE * SIDE,
                "{} is not {SIDE}x{SIDE}",
                icon.slug()
            );
        }
    }

    /// No two drawings share a slug, because the slug is the cache key.
    #[test]
    fn the_slugs_are_unique() {
        let mut seen: Vec<&str> = Icon::ALL.iter().map(|i| i.slug()).collect();
        seen.sort_unstable();
        let n = seen.len();
        seen.dedup();
        assert_eq!(seen.len(), n, "two icons share a slug and so share a texture");
    }

    /// Nothing in the set is blank, and nothing in it is a solid block.
    ///
    /// Both are what a conversion that went wrong looks like: reading the
    /// wrong channel out of the PNG gives an empty mask, and reading the
    /// ground instead of the ink gives a filled square. Neither would fail
    /// any length check, and both would be obvious only on screen.
    #[test]
    fn every_drawing_has_ink_and_air() {
        for icon in Icon::ALL {
            let lit = icon.coverage().iter().filter(|&&a| a > 8).count();
            let share = lit as f32 / (SIDE * SIDE) as f32;
            assert!(
                (0.02..0.85).contains(&share),
                "{} covers {:.1}% of its block - blank or solid, not a drawing",
                icon.slug(),
                share * 100.0
            );
        }
    }

    /// **The clear margin `make-icons.py` leaves is really there.**
    ///
    /// The outermost ring of every asset is empty, which is what lets a badge
    /// sit in a rect that grazes its neighbour without touching it, and what
    /// gives every stroke an antialiased end instead of a cut one. Regenerate
    /// with `PAD = 0` and this is what says so.
    #[test]
    fn every_drawing_keeps_clear_of_its_own_edge() {
        for icon in Icon::ALL {
            let c = icon.coverage();
            let edge = (0..SIDE)
                .flat_map(|i| [c[i], c[(SIDE - 1) * SIDE + i], c[i * SIDE], c[i * SIDE + SIDE - 1]])
                .max()
                .unwrap_or(0);
            assert_eq!(edge, 0, "{} runs to the edge of its block", icon.slug());
        }
    }

    /// **Nothing is ever painted outside the rect it was handed.**
    ///
    /// The owner's acceptance test is "nothing cut off", and the way an icon
    /// gets cut off is by being drawn bigger than its seat and clipped by
    /// whatever is around it. Checked at the sizes and shapes the app really
    /// uses - the strip's square badge, the picker's cell, the toolbar's
    /// shrunken chip - plus two deliberately lopsided rects, because the
    /// letterboxing is the part a future edit is most likely to replace with
    /// a stretch.
    #[test]
    fn a_drawing_never_leaves_the_rect_it_was_given() {
        for (w, h) in [
            (28.0, 28.0),  // the picker cell
            (33.0, 33.0),  // the strip badge at a usual window height
            (16.0, 16.0),  // the arrangement toolbar's metronome
            (12.0, 12.0),  // the smallest the strip badge ever gets
            (64.0, 20.0),  // wide: must letterbox left and right
            (20.0, 64.0),  // tall: must letterbox top and bottom
            (41.0, 40.0),  // odd by a point, where a centring bug hides
        ] {
            let seat = Rect::from_min_size(Pos2::new(7.5, 11.25), Vec2::new(w, h));
            let painted = fitted(seat);
            assert!(
                seat.contains_rect(painted),
                "a {w}x{h} seat is painted outside at {painted:?}"
            );
            // And it FILLS the short axis: letterboxing is not licence to
            // draw small. A regression that shrank every icon by a third
            // would still be "contained".
            let side = w.min(h);
            assert!(
                (painted.width() - side).abs() < 0.01 && (painted.height() - side).abs() < 0.01,
                "a {w}x{h} seat should take the whole {side}, not {}x{}",
                painted.width(),
                painted.height()
            );
            assert!(
                (painted.center() - seat.center()).length() < 0.01,
                "a {w}x{h} seat draws off-centre"
            );
        }
    }

    /// A rect with no room in it draws nothing rather than panicking.
    ///
    /// `Rect::NOTHING` is how this codebase says "this control is not on
    /// screen", and the strip hands its badge rect straight through, so the
    /// degenerate case is reached on every master strip in the app.
    #[test]
    fn an_empty_seat_is_not_a_panic() {
        let ctx = egui::Context::default();
        let _ = ctx.run(Default::default(), |c| {
            let area = Rect::from_min_size(Pos2::ZERO, Vec2::splat(64.0));
            let p = Painter::new(c.clone(), egui::LayerId::debug(), area);
            draw(&p, Rect::NOTHING, Color32::WHITE, Icon::Wave);
            draw(&p, Rect::ZERO, Color32::WHITE, Icon::Wave);
        });
    }

    /// **One texture per icon, however many times it is drawn.**
    ///
    /// The mixer draws a badge on every strip every frame; the picker draws
    /// thirty-six at once. Building the texture at each call would upload a
    /// megabyte and a half per frame, which is the kind of cost that shows up
    /// as "the app got slow" long after the change that caused it.
    #[test]
    fn a_texture_is_built_once_and_kept() {
        let ctx = egui::Context::default();
        let count = || ctx.tex_manager().read().num_allocated();
        let _ = ctx.run(Default::default(), |_| {});
        let before = count();
        let seat = Rect::from_min_size(Pos2::ZERO, Vec2::splat(32.0));
        let _ = ctx.run(Default::default(), |c| {
            let p = Painter::new(c.clone(), egui::LayerId::debug(), seat);
            for _ in 0..24 {
                draw(&p, seat, Color32::WHITE, Icon::Wave);
                draw(&p, seat, Color32::BLACK, Icon::Metronome);
            }
        });
        assert_eq!(count(), before + 2, "the same icon uploaded more than once");
        // And a second frame reuses what the first built.
        let after_first = count();
        let _ = ctx.run(Default::default(), |c| {
            let p = Painter::new(c.clone(), egui::LayerId::debug(), seat);
            draw(&p, seat, Color32::WHITE, Icon::Wave);
            draw(&p, seat, Color32::BLACK, Icon::Metronome);
        });
        assert_eq!(count(), after_first, "the cache did not survive the frame");
    }
}
