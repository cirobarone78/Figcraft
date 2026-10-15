# Importing Figma designs

FigCraft imports a local `.fig` file or a snapshot of a Figma Design file through the documented
REST API. Both routes use the same existing editable-document importer. Import is best effort:
review text, effects, components and unsupported layers before using the result.

## Local files

Choose **File → Open** or pass a `.fig` path when starting FigCraft to open a new unsaved document.
**File → Import from file…** and dropping a `.fig` file on the canvas add its pages to the current
document as one undo step. Opening an imported file does not use the source path as its save
destination; choose a new `.figcraft` path when saving.

A queued canvas drop stays bound to the document and page that received it, including the drop
position, even if tabs or pages change while the import card is shown. If that destination is
closed or removed, the import is cancelled instead of moving to another document. Home-screen
drops still open a new document.

An import report shows page, layer and image counts and compatibility notes. The CLI's
`info file.fig --json` also returns notes. Missing local fonts can change the appearance of text;
the fonts inside the source design are not downloaded or installed.

When pages are added to the current document, variable, collection and style ids from the
imported file that collide with ids already in the document are renamed, so existing assets and
their bindings are not replaced.

## From a Figma link (desktop)

A live desktop PAT import has been verified with a synthetic Design file. The optional variables
request returned HTTP 403 and became a warning while the base design imported; see
[validation and limits](#validation-and-limits) for the scope.

1. Choose **File → Import from Figma…**.
2. Paste an HTTPS Design link beginning with `https://www.figma.com/design/` or the older `/file/`.
   Branch links and other nested resource links are rejected; use the main file's link.
3. Enter a personal access token with `file_content:read` and access to that file.
4. Choose a new document or add pages to the current document, then start the import.

The token is masked, used for this import only, and omitted from settings, document files,
engine commands, inspection and import reports. It is sent only to `https://api.figma.com`.
Image downloads use a separate client without that authentication header. Errors do not echo
tokens, signed asset URLs or server response bodies.

Progress covers the document, local variables and image fills. Cancel immediately closes the
dialog and prevents a late result from changing a document. An already running HTTP request can
take up to its 30-second timeout to finish. If tabs change during download, an append still targets
the original document; closing that document discards the result with an error.

The whole Design file is imported, including links containing a selected `node-id`. FigJam,
Slides and other products are rejected. The result is a local snapshot with file key and version
in the report; there is no automatic sync or writeback to Figma.

Local variables are optional. Their endpoint requires `file_variables:read` and eligible
Enterprise access. Any failure to download or read them (other than cancellation or the overall
time limit) becomes a warning; resolved layer values still import. Missing or refused image
downloads become placeholders with notes. Only recognised HTTPS Figma asset origins and their
recognised redirects are downloaded.

Downloads are bounded: 64 MiB file JSON, 16 MiB variables JSON, 8 MiB image URL map, 32 MiB per
image, 256 MiB total and 2,048 distinct image fills. HTTP 429 retries are bounded and honour a
short `Retry-After`; longer waits produce an error. The entire job has a ten-minute limit.
The downloaded JSON is accepted up to 576 levels of nesting; layers nested deeper than 256 levels
are skipped with a warning. There is no separate limit on the number of values beyond the byte
limits. Local file imports have separate decoder limits. Limits may reject unusually large designs.

The browser build retains local `.fig` and REST JSON import. Token-based HTTP import requires
the desktop app or a native CLI/MCP process.

## CLI and MCP

The CLI and MCP link imports share the transport tested with synthetic HTTP responses and a live
desktop PAT import. Direct live token authentication through CLI or MCP remains unverified.

Set `FIGCRAFT_FIGMA_TOKEN` in the hosting process environment using your secret manager or a
temporary process variable. Do not put the token in command arguments, scripts committed to Git,
MCP tool arguments or agent prompts.

```sh
figcraft-cli import-figma https://www.figma.com/design/FILE_KEY/Design -o imported.figcraft
```

The command downloads a new document and atomically saves `.figcraft`, then prints source
metadata, counts and notes as JSON. An existing file at the `-o` path is replaced without asking;
choose a new path. No file is saved when download or conversion fails.
MCP exposes `import_figma {"url": "https://www.figma.com/design/FILE_KEY/Design"}`. It opens a
new unsaved document in either a headless or attached session. Save with `save_file` afterwards.
The token belongs to the MCP process environment even when attached to a desktop app.

Engine integrations can pass `{data: base64, name: "Design.json", images: {imageRef: base64},
variables: object}` to `file.open` or `file.import`; neither command performs networking.
Imports from bytes (a Figma link or a `data` payload) are not kept for **Repeat last command**
(`edit.repeat`), so the payload is not held in memory or replayed.

## Experimental `.fig` export

**File → Save as…** (Ctrl+Shift+S, Cmd+Shift+S on macOS) opens the standard operating-system
save dialog directly. On Windows, choose native `.figcraft` or **Figma (.fig)** in
the file-type selector. On macOS, both extensions are allowed; enter a name ending in `.fig`
to export a Figma copy. A `.figcraft` save updates the native destination and marks the document saved;
choosing `.fig` writes an export copy, reports the number of compatibility warnings and keeps the native
destination and unsaved changes. Regular Save and Ctrl+S (Cmd+S on macOS) use the native
destination when it exists, or open this save dialog for a document without a destination.

Use **File → Export… → The whole file → Figma (.fig)** to write a `.fig` copy directly,
or run `figcraft-cli export work.figcraft -o export-copy.fig --format fig` or
`figcraft-cli convert work.figcraft converted-copy.fig`. These routes use the same existing
experimental exporter. Choose a new destination: an explicit output path can overwrite a file.
The experimental `.fig` routes do not change the document's native save destination.

An export is refused when a nested instance has children and its exported type would change
from `INSTANCE` to `FRAME`, with `.fig export: nested instance type changes with children are not supported`.
This guard returns an error rather than writing that unsupported copy. Export is also refused
when a nested instance has no unambiguous source layer, with
`.fig export: a nested instance has no unambiguous source layer`. Both checks stop before writing
the target file. A Boolean property value without valid retained resource provenance is left
out with a warning rather than written against a guessed definition. Retained private Boolean
property-resource metadata that is inconsistent, oversized or no longer referenced is likewise
dropped with a counted warning; opening or exporting never fails because of it.
Other unsupported or approximated content can still produce warnings; these
guards do not establish general fidelity.

A text layer whose font, size, line height or letter spacing differs from its linked text style
is written without the style link, with a warning. This matches how FigCraft itself unlinks a
layer from its text style when those values are changed.

Reopen a successfully exported copy in FigCraft and compare pages/nodes, image fills,
variables/modes/bindings, shared styles and instance links with the native working document.
Then import the copy into Figma as a new test file and compare structure and appearance manually.
Keep source files, reference exports and screenshots private and outside the repository.
Own-importer round trips do not prove Figma compatibility. Current-writer acceptance is checked
only for a small synthetic file ([validation and limits](#validation-and-limits)); pixel fidelity
remains unmeasured.

## Building on macOS

The importer has no macOS-specific code. Native builds target `aarch64-apple-darwin` (Apple
Silicon) and `x86_64-apple-darwin` (Intel); without `--target`, Cargo builds for the Rust host:

```sh
cargo build --release --locked -p figcraft -p figcraft-cli
```

`packaging/macos/package.sh --arch aarch64|x86_64|universal` builds an app bundle, disk image and
CLI zip as described in [releasing](releasing.md); without Developer ID credentials it signs ad hoc
only. Native execution on macOS has not yet been verified for this importer.

## Validation and limits

Automated tests use synthetic responses and documents: authentication errors, rate limits,
URL and asset policy, cancellation, byte budgets, editable images and variables, undo, target
identity and report visibility. They complement the scoped live desktop and Figma writer checks
below. Successful import alone does not establish visual parity.

The `.fig` message decoder charges retained values against `max(256 MiB, 16 × encoded message
bytes)`, capped at 1 GiB. This is an accounting budget, not an exact heap ceiling; container growth
can make actual allocations larger. Value-count, schema and nesting limits remain in force.

**Decoder-budget follow-up (2026-10-14):** A further private file exceeded the former 256 MiB
decoded-value budget: its 47.38 MiB message required 592.73 MiB of charged values. With the
source-sized import budget, the Windows release CLI and desktop app open all 29 pages and
103,263 nodes. A native copy reopens with matching page, node, image, style and variable summaries;
the source file's hash is unchanged. Five compatibility notes remain, including unavailable
external components and unresolved instance source links. This verifies the decoder rejection
fix, not the visual misalignment reported in issue #329. The exporter retains its conservative
256 MiB decoded-value admission budget.

Referenced cached main components on an internal-only `.fig` canvas are retained as read-only offline
library definitions in the native hidden library store. Their containing context keeps inherited
variable modes and component-set ancestry; the cache does not add visible pages. Instance links,
source identities and overrides are resolved against these retained definitions. The private
carrier is consumed only by `.fig` import, not by public REST JSON import. Native saving retains
the cache, and experimental `.fig` export writes it under the internal asset canvas. This does not
create a live connection, update source or invented library identity. Truly unavailable mains
still use the existing frame fallback and compatibility note. Pre-existing import dirty markers
do not block unrelated visible edits: the library read-only guard checks whether a marked node
actually changed, while retaining the dirty queue and protecting changed remote nodes. Local
Undo/Redo recognizes only the exact recorded snapshot of the same document. Restoring that
approved state keeps its pending dirty queue instead of treating historical instance geometry
as a new override; preview and live-access checks remain active. A refused history command
restores the local history and revision as well as the document state.

A subsequent cached-main release check of the same private source retains 29 visible pages,
106,293 nodes and four images. Its hidden store retains 1,514 component definitions and restores
9,539 instance-to-main links without creating live library records. The external-component and
source-link notes disappear; the remaining three notes concern segment strokes, region fills
and grid tracks. Native CLI save/reopen retains the counts with no repair notes. Forty uniquely
paired icon paint records agree with read-only Figma metadata. This is representative evidence,
not exhaustive visual parity; the original source hash remains unchanged.

Component overrides can address a materialized child by its original `overrideKey` rather than
its current GUID. The importer resolves unique explicit aliases within the selected main,
one instance boundary at a time, including swapped and nested instances. Current GUIDs take
precedence; ambiguous aliases remain unresolved. Source identities and override precedence are
preserved. Synthetic tests cover ordinary frame descendants, nested priority, mixed alias/current
paths, swaps, ambiguity and two export/reimport rounds. Missing local fonts remain a separate
rendering limitation even when the imported family and style metadata are correct.

Direct paint color bindings are applied when a document is opened, before establishing the saved
baseline. A paint's stale literal fallback therefore does not replace the current bound color,
including on pages that have not yet been selected. This initialization does not run a full
layout normalization or applying geometry and component-property bindings.

The decoder-only Windows workspace run passed 2,362 tests with six ignored. Its first Ubuntu run
failed two recovery tests; the recovery suite passed separately and a complete unchanged-source
rerun passed 2,364 tests with six ignored. The later alias-mapping run passed 2,365 Windows and
2,367 Ubuntu tests, with six ignored on each. Format, Clippy, layers and assets passed on both
platforms, and Ubuntu WASM passed. That run's aggregate result was blocked because documentation
changed during the checks. These counts include sample-gated tests without private samples.

The local Windows/Ubuntu CI run passes all 11 checks. Fresh Windows GUI and CLI release builds
were exercised, and a separate synthetic local interoperability check passes 404 assertions.

Some format and Engine tests are sample-gated: they read private `.fig` files named by the
`FIGCRAFT_FIG_SAMPLES` environment variable and pass without checking anything when it is unset.
Test counts below include them.

**Verified** on the current source with three private real-world `.fig` files (not distributed):

- The files open; compatibility notes remain for unsupported content and unavailable external
  components (an older bare canvas exposed a missing `Image.dataBlob` resolver path, now covered
  by synthetic tests).
- Native `.figcraft` save/open preserves every persisted imported document field and encoded image
  byte exactly.
- The full library run passes 261 format-library tests and 851 Engine tests with two ignored.
- The selected Engine export/open comparison passes for layer order/types/names/text,
  position/size, selected layout values, image content and source-link presence.
- The strict canonical format round trip, legacy-schema checks and style/variable asset checks
  pass. Original file hashes stayed unchanged during the run.
- All seven staged compatibility/persistence checks pass on the three originals.
- The optional strict zero-warning check still reports the files' import notes; it is not a pass.

The format round trip compares canonical nodes: it remaps identities and normalizes representation
differences, removes source/override state, and clears style references, bindings and explicit
variable modes. Its numeric threshold is `0.002 × max(|a|, |b|, 1)`. The separate asset check keeps
style links/bindings/modes after ID-to-name mapping and exactly compares variable JSON and style
name/type lists. The Engine check uses selected fields and Float32 geometry tolerance. These
checks establish only their stated scope for those files.

The experimental exporter preserves nested assignment clears, outer override priority, explicit
instance identities through renames/swaps and known nested instances inside bodies materialized
as frames when an external main is unavailable. Import derives source links from original GUIDs
and expansion namespaces; export assigns new GUID addresses through those links. Local text
decoration and absolute child positioning have focused regressions, as do adjacent text runs,
shared styles, decode budgets, blur start radius, paragraph spacing, text case and mask metadata.

Boolean assignments backed by retained hidden library definitions survive export/reimport
without adding guessed defaults to a visible component. Focused codec, synthetic round-trip and
guard tests pass, and an Engine test passes two imports of the same resource file, undo/redo and
`.fig` export/reimport with page metadata and main links preserved. These checks cover FigCraft
retention; private alias data is not Figma acceptance proof.

Schema tests use explicit legacy `fig-kiwi` profiles 4, 70 and 89 to check required core and shared
fields, IDs, types, array flags, enum values and ordered structs. Modern and unknown profiles keep
the complete written-schema check. Missing modern definitions/fields in the legacy profiles are
reported as **UNVERIFIED**, not counted as supported or proved by those samples.

A manual Figma Design browser check accepted a small synthetic file from an earlier writer
version: one page with a 640×360 frame, a blue 180×96 rectangle, an orange 96×96 ellipse and a
text layer. Arial was missing there; changing that test layer to Inter made its text render.
Figma's saved local copy opened in an earlier FigCraft reader without warnings. A read-only check
through Figma's own MCP server passes 52 assertions on that file for frame/child topology, types,
positions, dimensions, visibility, opacity, colors, corners/ellipse and Inter Regular 32
text/content/modes. This small workflow establishes no pixel or original-font fidelity.

A small synthetic file from the current experimental writer was imported into Figma. A read-only
check through Figma's own MCP server passes 124 assertions for geometry, paints and text,
including Inter Regular 32 over the entire actual text span; Figma reported no missing font.
A Figma PNG was saved and visually inspected. This establishes acceptance of that synthetic file.

The current Windows desktop app also imported a synthetic Design file through the live REST API
with a temporary PAT. The optional variables request returned HTTP 403 and was reported as a
warning while the base design imported. The result was saved natively and reopened with the fresh
Windows CLI without repairs.

**Not verified:** broader current-writer or real-file Figma acceptance; source or pixel fidelity
against Figma; native execution on macOS; successful live variable/image downloads; direct live
token authentication through CLI or MCP.

**Recent behaviour changes:**

- Native `.figcraft` files and recovery copies are no longer limited to 256 MiB when opened;
  import sources still are.
- `.fig` archives are no longer rejected by their total size; the canvas and image budgets still
  apply. A damaged image member falls back to the image data embedded in the canvas.
- REST and link imports accept JSON nesting up to 576 levels (layers beyond 256 levels are skipped
  with a warning) and have no value-count limit beyond the byte limit.
- Inconsistent or unreferenced private Boolean property-resource metadata is dropped with a
  warning; open and export no longer fail because of it.
- `.fig` export writes a text layer without its style link, with a warning, when its font, size,
  line height or letter spacing differs from the linked text style.
- Byte imports (Figma link or `data` payload) are not kept for **Repeat last command**.
- Appending pages renames colliding variable, collection and style ids from the imported file.

Public API references: [file endpoints](https://developers.figma.com/docs/rest-api/file-endpoints/),
[personal access tokens](https://developers.figma.com/docs/rest-api/authentication/#access-tokens),
[variables endpoints](https://developers.figma.com/docs/rest-api/variables-endpoints/) and
[rate limits](https://developers.figma.com/docs/rest-api/rate-limits/).
