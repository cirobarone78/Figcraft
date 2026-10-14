//! The presence projection (CraftHub spec §6.1, §18.4 row 1): what FigCraft's live UI state
//! looks like on the wire, and what a peer's state renders as here. Presence is ephemeral — it
//! is never stored, never journalled, never saved (§2.7's rule, kept verbatim in v3).
//!
//! FigCraft's row in the per-app projection table: cursor in page coordinates, the page being
//! worked on, the selection, the viewport rectangle (for follow mode), and the text object
//! being edited with its caret. FigCraft has no timeline, no tracks and no generation jobs —
//! those fields belong to the siblings (FilmCraft, EffectCraft, ArtCraft) and stay `None` here.

use craft_collab::adapter::UiState;

/// Everything the FigCraft UI needs to render one peer: the projection is a two-way street —
/// the same fields go out and come back.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PeerView {
    /// The peer's cursor in page coordinates, on the page they're on.
    pub cursor: Option<(f64, f64)>,
    /// The page the peer is on (a peer on another page renders only in the page bar).
    pub page: Option<u64>,
    /// The peer's selection (outline highlights, never edit locks — §6.7 does locks).
    pub selection: Vec<craft_collab::ids::NodeId>,
    /// The peer's viewport, offered to the follow mode.
    pub viewport: Option<(f64, f64, f64, f64)>,
    /// The peer is typing in this text object (caret and selection in characters).
    pub editing_text: Option<(craft_collab::ids::NodeId, usize, Option<usize>)>,
}

impl PeerView {
    /// Project FigCraft's UI state into the fields the hub's `UiState` carries.
    ///
    /// The selection is bounded (§2.12: at most 10,000 ids) by the core's sanitizer, not here —
    /// the app reports what it truly has and the hub enforces the budget.
    #[must_use]
    pub fn from_ui(ui: &UiState) -> Self {
        Self { cursor: ui.cursor, page: ui.page, selection: ui.selection.clone(), viewport: ui.viewport, editing_text: ui.editing_text }
    }

    /// Whether this peer shows anything on `page` at all (the page bar always shows everyone;
    /// the canvas only the peers on the same page).
    #[must_use]
    pub fn on_page(&self, page: u64) -> bool {
        self.page == Some(page)
    }
}
