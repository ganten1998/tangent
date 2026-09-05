# The drum kits Tangent ships

Two kits built by the owner of this app on Tangent's own drum pads, saved
with the pad strip's SAVE as self-contained `.kit` files — every sample is
embedded, so nothing beside them is needed. They are compiled into the binary
(`tangent/src/shipped.rs`) and written to the user's `Presets/Tangent Drums`
folder once, at launch, so that Load Kit has something to show on a fresh
install. A file the user deletes stays deleted.

| File | Size |
| --- | --- |
| `CR78.kit` | 715 KB |
| `LinnDrum.kit` | 852 KB |

The names say which machine each kit is modelled on. "CR-78" is Roland's and
"LinnDrum" is Linn Electronics'; the names are references to the instruments
whose sounds these kits carry, not branding, and nothing here is endorsed by
or affiliated with either company.

Each pad keeps the absolute path of the file it was built from as provenance;
that path is the author's own machine and is ignored when the embedded audio
is present, which it is for every pad here.
