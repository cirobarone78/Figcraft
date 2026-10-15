# FigCraft architecture

> **Last reviewed:** 2026-10-10 · **Last updated:** 2026-10-14 · **Change:** minor (M1 Figma import/export checks and robustness fixes; live desktop PAT and current-writer synthetic Figma checks; bounded native Figma REST import and current `.fig` reader) · **Target:** Figma Design

How FigCraft is built today. Engine first, a thin swappable UI, everything a command; a Figma-shaped
data model (a scene graph of frames with auto layout, components and variables). ~108,800 lines of
Rust (2026-10-10).

## Crates and layers

`cargo xtask layers` enforces the layering (`xtask/src/layers.rs`): a crate may depend only on
lower layers. Nothing below L6 depends on egui, eframe, rfd or winit, so the UI crate can be
replaced.

| Layer | Crate | Lines | Role |
|---|---|---|---|
| L0 | `figcraft-geom` | ~0.5k | Affine transforms, shape path builders (rounded and smoothed corners, polygons, stars, arcs) on kurbo |
| L0 | `figcraft-color` | ~1.3k | Colours (HSB/HSL, hex/CSS, Display P3), paints (solid, gradients, image), blend modes |
| L1 | `figcraft-doc` | ~8.5k | Document model: pages, node tree in a persistent map (`imbl`, stable ids), properties, components, styles, variables, prototype data, comments, versions. Serde everywhere |
| L2 | `figcraft-layout` | ~1.8k | Auto layout (horizontal, vertical, wrap, grid; hug/fill/fixed; min/max), constraints, layout grid geometry |
| L2 | `figcraft-text` | ~3.1k | Font database (optional embedded craft-fonts + system scan), shaping (harfrust), bidi, line layout, glyph outlines (skrifa), variable fonts |
| L2 | `figcraft-collab` | ~3.4k | Real-time collaboration core (no networking): synced object/property model, change sets, fractional positions, server ordering and validation, client reconciliation, wire messages |
| L2 | `figcraft-plugin` | ~1.0k | Plugin manifest and the WebAssembly sandbox (wasmi, metered, memory cap, no file system, network or clock) |
| L2 | `figcraft-pathops` | ~1.1k | Booleans (linesweeper), outline stroke with alignment, arrowheads |
| L3 | `figcraft-render` | ~5.4k | Scene → pixels on tiny-skia (CPU): isolated layers for opacity/blend/effects, clips, masks, shadows, blurs (uniform, progressive, background), noise, texture, glass, text; banded in parallel; retained rendering of only what changed; levels of detail when zoomed out |
| L3 | `figcraft-format` | ~5.2k | Native `.figcraft` (zip of `document.json` + images; streamed JSON), local library files, versions, REST API JSON import, best-effort `.fig` reader and experimental writer |
| L4 | `figcraft-svg` | ~3.7k | SVG export and import (usvg) |
| L4 | `figcraft-pdf` | ~2.3k | PDF export (krilla) with print options |
| L5 | `figcraft-engine` | ~39.3k | Sessions, undo history (`Arc` snapshots), the command registry (`cmd/*`, `spec!`), pointer tools and vector edit mode, snapping and measuring, components sync, libraries, prototype runtime, Dev Mode code generation, the menu catalog (`catalog.rs`) and the keymap (`keymap.rs`), a panic guard |
| L6 | `figcraft-ui-egui` | ~31.9k | The UI3-style frontend: canvas (render worker), toolbar, layers, Design/Prototype/Inspect panels, colour picker, menus, dialogs, presenter, comments, accessibility (AccessKit), the control channel server |
| L6 | `figcraft-mcp` | ~2.4k | MCP server (JSON-RPC over stdio), headless or bridged to a running app |
| L6 | `figcraft-collab-net` | ~3.0k | Live sessions over the network: the WebSocket transport (tungstenite, a reader and a writer thread per connection), the hub that runs a collaboration core server per open file for the session host the app embeds and for `figcraft-server` (behind a `Backend`), the joining and managing clients (publishing), invite links, presence relay and limits; the browser client |
| L6 | `figcraft-figma-net` | ~1.2k | Bounded read-only public Figma REST transport: separate API/asset HTTPS clients, temporary PAT, retries, cancellation and image/variable downloads; portable URL and bundle types; no engine dependency |
| apps | `figcraft`, `figcraft-cli`, `figcraft-web`, `figcraft-server` | ~3.2k | Desktop app (eframe + wgpu), command line, browser build (WASM, WebGPU/WebGL2), collaboration server (files, share links, persistence; [`server.md`](server.md)) |
| plugins | `plugins/` (own workspace) | | Rust plugin SDK and two example plugins, built for wasm32 by `cargo xtask plugins` ([`plugins.md`](plugins.md)) |
| tools | `xtask` | | `ci`, `layers`, `assets`, `parity`, `version`, `icon`, `wasm`, `plugins`, `bench` |

## Data model

A `Document` holds pages, a persistent map of nodes (frames, groups, sections, rectangles,
ellipses, polygons, stars, lines, vector networks, text with styled runs, boolean groups,
components, component sets, instances, slices), styles, variable collections, comments, versions
and images by hash. Instances keep their main's id and per-layer overrides; the engine keeps their
materialised children in sync when a main changes (directly for value, size and add/remove edits,
by a rebuild otherwise). Snapshots are cheap (`Arc` and structural sharing), which gives unlimited
undo with one step per gesture.

**M1 file identity update (2026-10-14):** The `.fig` mapper resolves original GUIDs through
instance expansion to native ids. Unchanged mains retain contextual source copies; proven swaps
use the current main. Missing external mains materialize as frames with owned children, while
known nested instances and copies retain validated source identities. These private provenance
hints are consumed only by the `.fig` pipeline; ordinary REST import ignores them. Experimental
export assigns new GUID addresses through proven source links, separately from inherited scalar
comparison baselines, and rejects unsupported nested type changes or ambiguous sources before writing.

Referenced Boolean declarations on excluded library pages are retained once per input scope in
existing page metadata; small node bindings keep their actual component context. Export emits
standard hidden SYMBOL definitions with each owner's own default and allocates fresh wire IDs.
Private aliases preserve native keys; ordinary REST ignores these markers. Active unknown
Boolean values without valid provenance are omitted with a counted warning; invalid private records never block opening or export. Nineteen focused BOOL tests,
fourteen capture/REST alias tests and an actual engine append/undo/redo/export test pass.

The complete library run passes 261 format and 851 Engine tests (two ignored; sample-gated
tests pass empty unless `FIGCRAFT_FIG_SAMPLES` is set), plus three private real files through
selected Engine export/open, the strict canonical format round trip, legacy-schema and
style/variable asset checks. Text runs/shared styles/decode budgets, blur, paragraph spacing,
text case and mask metadata have focused regressions. Canonical format checks normalize IDs and
representation, omit source/override state and local style/binding/mode references, and allow
numeric error `0.002 × max(|a|, |b|, 1)`. Asset tests separately retain links/bindings/modes with
ID-to-name mapping, exact variable JSON and style name/type lists; Engine checks use selected
fields and Float32 geometry tolerance. Original hashes stayed unchanged during the run. Native
persistence passes all three with exact imported-field/image-byte equality; the zero-warning check
still reports import notes. Figma accepts a small synthetic file from the current writer;
a read-only check through Figma's own MCP server passes 124 assertions. The shared
`figcraft-figma-net` transport also has a live desktop PAT import check. Direct CLI/MCP token
authentication, successful live variable/image downloads, broader Figma acceptance, native macOS
execution and source/pixel fidelity remain unverified
([status](figma-import.md#validation-and-limits)).

## Commands: one path for every surface

Every user-visible behaviour is a command in `crates/engine/src/cmd/*` (455 ids after #101)
with an id, label, parameter docs and undo flag. The menus (generated from the catalog), keyboard
shortcuts, Actions palette, CLI (`figcraft-cli run`), JSON control channel (`--control <port>`) and
MCP server all dispatch the same ids, so agents can do anything a person can; WebAssembly plugins call them too, within their manifest's permissions. Commands never open
dialogs themselves; they request UI, and programmatic callers drop those requests.
`cargo xtask parity` measures the catalog's coverage ([`parity-checklist.md`](parity-checklist.md)).

## Rendering

The CPU renderer is the single source of pixels for the canvas, exports and the browser. The
desktop canvas renders on a worker thread at the device pixel ratio and keeps its last image,
re-rendering only the region a pan uncovers or an edit touches; overlays (selection, handles,
guides, labels) are drawn by egui on top. A seeded random walk checks the retained canvas against
full renders. SVG and PDF export walk the same scene and embed rasters where those formats can't
express an effect.

## File I/O

`.figcraft` is the native format (atomic saves, autosave checkpoints; the desktop app keeps
recovery copies of unsaved documents that reopen after a crash or a forced quit). Imports: local
`.fig` (Kiwi/canvas decoder, best effort), REST API file JSON with variables and images, SVG,
raster images and clipboard content. Native desktop/CLI/MCP can fetch a Figma Design link through
`figma-net`; its bounded worker supplies bytes to the same `file.open`/`file.import` commands.
HTTP and credentials stay above the engine. REST and `.fig` open as unsaved documents; import
appends pages as one undo step. Exports: PNG, JPG, WebP, SVG, PDF, code and experimental `.fig`
([`file-format-parity.md`](file-format-parity.md), [`figma-import.md`](figma-import.md)).

## Robustness

Workspace lints deny `unwrap`, `expect`, `panic!` and friends in shipped code; `unsafe_code` is
forbidden. `figcraft_engine::guard` turns a panicking command into an error and rolls the document
back; the desktop app also guards each UI frame. Junk-parameter sweeps run every command with bad
input.

## Revision history

| Date | Change | Summary |
|---|---|---|
| 2026-10-14 | minor | M1 validation refresh: a current-writer synthetic file is accepted in Figma with 124 read-only assertions; live desktop PAT import succeeds on a synthetic design, with optional variables HTTP 403 reported as a warning. Broader/real-file acceptance, source/pixel fidelity, native macOS, successful live variable/image downloads and direct CLI/MCP authentication remain open ([scope](figma-import.md#validation-and-limits)). Historical assessment unchanged |
| 2026-10-14 | minor | Figma import/export robustness: large native files open, `.fig` size budgets, deep REST JSON, inconsistent Boolean metadata dropped with a warning, text-style unlinking on `.fig` export, byte imports not kept for Repeat, colliding ids renamed on append; import docs made environment-neutral. Historical assessment unchanged |
| 2026-10-14 | minor | Referenced hidden BOOL retention/guard and actual append checks pass. Complete library run: 223 Format pass, 832 Engine pass/2 ignored (counts include sample-gated tests that pass empty without `FIGCRAFT_FIG_SAMPLES`); with three private sample files, selected Engine export/open, strict canonical format round trip, legacy schema, style/variable asset and exact native imported-field/image-byte persistence checks pass within their stated scope; the zero-warning check still reports import notes. A small synthetic file was accepted in Figma's browser app; a read-only check through Figma's own MCP server passes 52 assertions on it. Link/PAT import is tested only against synthetic HTTP responses. Not verified: live Figma API, current-writer Figma acceptance, native macOS, pixel fidelity. Historical assessment unchanged |
| 2026-10-14 | minor | M1 .fig GUID/context source resolution, missing-main frame ownership and address/scalar separation documented; ordinary REST provenance remains ignored, native save format and layering unchanged |
| 2026-10-13 | minor | Current `.fig` reader documented; native read-only Figma REST transport, byte/image command inputs and visible import reports; browser keeps local imports |
| 2026-10-11 | minor | `collab-net` hosts through one hub for the app's sessions and the standalone server; added the `figcraft-server` app |
| 2026-10-10 | minor | Merged main after #101–#103: WASM plugins (plugins area 0% → 35% breadth, 20% ready; ecosystem 5% → 20%) and the multiplayer core without networking (multiplayer 5% → 15% breadth, 5% ready; collaboration 8% → 12%); menu breadth 97.5% → 98.5% (200/203); ready for real work ~46% → ~47%; full parity 420–790 h → 390–745 h; beta 210–380 h → 200–365 h. Fixed the stated weight sums (areas 1–16 sum to 94, total 113; percentages were already computed with those) |
| 2026-10-10 | major | Created from the code as of PR #99 |
