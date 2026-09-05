# The drum kits Tangent ships

Two kits built by the owner of this app on Tangent's own drum pads, saved
with the pad strip's SAVE as self-contained `.kit` files — every sample is
embedded, so nothing beside them is needed. They are compiled into the binary
(`tangent/src/shipped.rs`) and written to the user's `Presets/Tangent Drums`
folder once, at launch, so that Load Kit has something to show on a fresh
install. A file the user deletes stays deleted.

## Where the samples came from

Both kits were assembled from the free packs at <https://www.drumkito.com/>:
the "Roland CR-78 Sample Pack" and the "LinnDrum Sample Pack", each described
there as "sampled from an original drum machine" and "totally free to
download". The site publishes no licence and no terms of use beyond a
site-wide "All Rights Reserved" notice, so the right to redistribute the
samples inside these kits has not been granted in writing. Permission is
the owner's to ask, at hello@drumkito.com; until it is answered, treat the
kits as the owner's own presets rather than as a pack anyone may take out of
the app and pass on.

| File | Size |
| --- | --- |
| `CR78.kit` | 715 KB |
| `LinnDrum.kit` | 852 KB |

The names say which machine each kit is modelled on. "CR-78" is Roland's and
"LinnDrum" is Linn Electronics'; the names are references to the instruments
whose sounds these kits carry, not branding, and nothing here is endorsed by
or affiliated with either company.

Each pad keeps the name of the file it was built from as provenance. The
originals carried the owner's absolute paths; those were scrubbed down to the
bare file name before shipping, and the loader never needs them because the
audio is embedded for every pad here.
