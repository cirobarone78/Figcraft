# The collaboration behavior suite (FigCraft's projections)

> The shared suite every Craft app runs before its CraftHub integration is enabled (the CraftHub
> specification §18.9): each row is one automated test with a written scenario, in the family's
> behaviour-suite tradition — the suites that caught four real bugs before shipping in M9.
> This file is FigCraft's copy of the shared catalog: the scenarios are identical in every app,
> and the *projections* column is where each app writes what the scenario means for its own UI.

## The scenarios

| # | Scenario | The guarantee under test | FigCraft's projection |
|---|----------|--------------------------|-----------------------|
| 1 | Join by code, join by link | One argument that recognizes itself; the file opens in its own tab, other sessions untouched | A `.crafthub` session opens as a new tab; the pages bar stays per-document |
| 2 | Concurrent editing converges | Snapshot-hash equality after arbitrary interleavings, drops and reconnects (per seed) | The canvas state after convergence equals the reference render tree |
| 3 | Read-only, with its reason | The reason is shown (view link, role without edit, region lock), never just a badge | The mode bar shows "view link" / "viewer role" / "locked region" with the region tinted |
| 4 | Batch refused with fixes | The server's refusal carries the fixes; the client applies them and lands on the server state | A moved-to-deleted-parent node snaps to the page with a toast explaining why |
| 5 | Per-user undo | My steps are mine alone; later writes by others survive my undo | Undo after a peer's edit undoes only my edit; the peer's shape stays where they put it |
| 6 | Offline, then resume as the same client | The recovery copy reattaches; unsent changes replay or land as a "(recovered copy)" beside the file | The unsent drag replays on reconnect; a conflicting copy appears in Home ▸ Online |
| 7 | Cursor chat and its bounds | ≤100 characters, ephemeral, never enters the document or the journal | The bubble renders at the peer's cursor for its lifetime, then is gone |
| 8 | Spotlight and follow | Follow tracks view and page; any view movement of mine stops the follow | The viewport animates to the presenter's; my own pan breaks the leash instantly |
| 9 | Presence and per-app projections | Ephemeral, throttled to change-only, bounded sizes | Cursor, page, selection outlines, viewport, and the text-caret badge on the edited text object |
| 10 | Region locks | Locks sync as document properties; locked regions tint; the reason is shown | A locked frame tints; editing inside it is refused with "locked by <name>" |
| 11 | Contribution, review, merge | The branch lives inside the live session; merge is one undo step; each side's changes show | The review panel shows both sides' property diffs, Inspect-style |
| 12 | Comments and their anchors | Anchor kinds per app; comments are entries with ids and order | FigCraft anchors a comment to a node (the selection); the pin rides the node |
| 13 | Background file notifications | Delivered when the tab is not front; bounded, dismissible | The tab badge shows "3 new" for a backgrounded shared file |
| 14 | Closing a file ends its session alone | Other sessions keep syncing behind the front tab | Closing one shared tab never drops another document's peers |
| 15 | The revert guard | `file.revert` is refused while its document is in a live session (including reconnect and pre-welcome) | The toast says "leave the session before reverting"; the document and its previews are intact |

## FigCraft's adapter surface (what the suite drives)

* **Property table** — the 44 synchronized node fields with their compile-time guard
  (`figcraft-hub::props`), the kind discriminants, and the document-level lists
  (styles, variables, collections, comments) with their orders.
* **Position keys** — the fractional sibling keys live beside the document
  (`FigcraftHub`), assigned by current order at first sync, kept by the longest rising
  run under edits, and spread when they grow past the family limit.
* **Presence** — cursor in page coordinates, the page, the selection, the viewport and the
  text-edit caret (`figcraft-hub::presence`). FigCraft has no timeline and no generation
  jobs: those projections stay `None` here and belong to the siblings.
* **Commands** — the unified `hub.*` catalog with the v2 `collab.*` twins
  (`figcraft-hub::commands`): same names in every Craft app, the explicit
  lexical-unification rule.
* **Deep links** — `figcraft://join?<code\|token>&server=<url>` (`figcraft-hub::deeplink`).

## The enable rule

No app is enabled before its protocol core passes its simulation tests in CI for a full week
(nightly, fixed seeds) — and enabling happens through the app's own `crafthub` feature flag,
never through the core, so a regression is one flag flip away from off (§18.4).
