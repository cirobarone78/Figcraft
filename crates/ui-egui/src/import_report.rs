//! Compatibility notes stay visible after a successful import, including menu and canvas drops.
//! It is a modal card like the other dialogs: the imported document is edited once it is closed.

use serde_json::{Value, json};

use crate::FigApp;
use crate::i18n::{tr, tr_noop, trf};
use crate::theme::Tokens;

/// A report stays small even when a damaged input produces many or very long notes.
pub const MAX_NOTES: usize = 500;
pub const MAX_NOTE_CHARS: usize = 2_000;

/// The fixed notes of the `.fig` and REST importers, translated in the report. `.fig` imports name
/// "the file" where REST imports name the JSON they read. Other notes are shown as they are.
const IMPORT_NOTES: &[&str] = &[
    tr_noop("conditional prototype actions nested too deeply were skipped"),
    tr_noop("conditional prototype actions with conditions that can't be evaluated were skipped"),
    tr_noop("connector labels were skipped"),
    tr_noop("connectors whose ends can't be placed are empty"),
    tr_noop("damaged vector geometry was skipped"),
    tr_noop("damaged vector networks import as outlines"),
    tr_noop("document children other than pages were skipped"),
    tr_noop("elbowed and curved connectors without a saved path import as straight lines"),
    tr_noop("entries without a node type were skipped"),
    tr_noop("filled areas of a vector with their own fill use the layer's fill"),
    tr_noop("gamepad triggers without exactly one known controller button were skipped"),
    tr_noop("grid tracks of fixed or content size import as equal tracks"),
    tr_noop("instance property values beyond the first 256 of an instance were left out"),
    tr_noop("instance swap properties whose default component isn't in the file were skipped"),
    tr_noop("instances inside their own main component were imported as frames"),
    tr_noop("instances of components that aren't in the file show no layers"),
    tr_noop("instances of components that aren't in the file were imported as frames"),
    tr_noop("instances that would nest a component inside itself were imported as frames"),
    tr_noop("instances without a main component were imported as empty frames"),
    tr_noop("invalid or oversized vector networks import as outlines"),
    tr_noop("invalid or oversized vector path data was skipped"),
    tr_noop("key triggers without a supported key were skipped"),
    tr_noop("key up triggers import as key presses"),
    tr_noop("layers without a position were placed at their parent's origin"),
    tr_noop("layers without a type were skipped"),
    tr_noop("points with their own stroke join use the layer's"),
    tr_noop("prototype actions of unknown types were skipped"),
    tr_noop("prototype actions without a destination in the file were skipped"),
    tr_noop("prototype expressions on variables that aren't in the file were skipped"),
    tr_noop("prototype expressions on variables that aren't in the variables JSON were skipped"),
    tr_noop("prototype expressions with unsupported functions were skipped"),
    tr_noop("prototype interactions without supported actions were skipped"),
    tr_noop("prototype navigations of unknown types were skipped"),
    tr_noop("prototype transitions of unknown types play instantly"),
    tr_noop("prototype triggers of unknown types were skipped"),
    tr_noop("set-variable actions on variables that aren't in the file were skipped"),
    tr_noop("set-variable actions on variables that aren't in the variables JSON were skipped"),
    tr_noop("set-variable actions with values that can't be evaluated were skipped"),
    tr_noop("set-variable actions with values that don't fit the variable were skipped"),
    tr_noop("set-variable actions without a value were skipped"),
    tr_noop("set-variable-mode actions on collections or modes that aren't in the file were skipped"),
    tr_noop("set-variable-mode actions on collections or modes that aren't in the variables JSON were skipped"),
    tr_noop("stars and polygons without point counts or geometry use the default point count"),
    tr_noop("stroke styles of single vector network segments were left out"),
    tr_noop("text style overrides refer to missing table entries"),
    tr_noop("the file has no pages"),
    tr_noop("the variables JSON has no `variableCollections` (expected the local variables response) and was ignored"),
    tr_noop("unknown component property types were skipped"),
    tr_noop("variable aliases to variables that aren't in the file (or of another type) were skipped"),
    tr_noop("variable aliases to variables that aren't in the variables JSON (or of another type) were skipped"),
    tr_noop("variable bindings to properties the layer doesn't have were skipped"),
    tr_noop("variable bindings to variables that aren't in the file (library variables) were skipped"),
    tr_noop("variable bindings to variables that aren't in the variables JSON (library variables) were skipped"),
    tr_noop(
        "variable bindings were skipped: the file JSON has no variable definitions (pass the local variables JSON as `variables`, or embed its `meta` next to the file JSON)",
    ),
    tr_noop(
        "variable bindings were skipped: the file has no variable definitions (pass the local variables JSON as `variables`, or embed its `meta` next to the file)",
    ),
    tr_noop("variable bindings whose variable type doesn't fit the property were skipped"),
    tr_noop("variable collections without modes were skipped"),
    tr_noop("variable mode lookups in prototype expressions that aren't a variable and one of its modes were skipped"),
    tr_noop("variable modes of collections that aren't in the file were skipped"),
    tr_noop("variable modes of collections that aren't in the variables JSON were skipped"),
    tr_noop("variable values that don't match their type (or composed colours) were skipped"),
    tr_noop("variables of unknown types were skipped"),
    tr_noop("variables whose collection isn't in the file were skipped"),
    tr_noop("variables whose collection isn't in the variables JSON were skipped"),
    tr_noop("variables without readable values were skipped"),
    tr_noop("vectors without geometry are empty"),
    tr_noop("vectors without geometry are empty (export the file JSON with geometry=paths)"),
    tr_noop("very long text was cut to 1,048,576 characters"),
    tr_noop("very long vector paths were skipped"),
    tr_noop("video actions are not supported and were skipped"),
    tr_noop("video triggers are not supported and were skipped"),
];

#[derive(Clone, Debug)]
pub struct Report {
    pub name: String,
    pub pages: Option<u64>,
    pub layers: Option<u64>,
    pub images: Option<u64>,
    pub appended: bool,
    pub notes: Vec<String>,
    pub note_count: usize,
    /// The remote file's identity, never the submitted URL or authentication information.
    pub source: Option<(String, Option<String>)>,
}

impl Report {
    /// Only successful file commands with import notes or actual repairs create a report.
    pub fn from_result(command: &str, result: &Value) -> Option<Self> {
        if !matches!(command, "file.open" | "file.import") {
            return None;
        }
        let warnings = result.get("warnings").and_then(Value::as_array);
        let repairs = result.get("repairs").and_then(Value::as_array);
        if warnings.is_none() && repairs.is_none_or(Vec::is_empty) {
            return None;
        }
        // `file.import` of an SVG places a frame on the current page (no `pages`): its notes
        // are reported, but no pages were appended.
        let appended = command == "file.import" && (result.get("pages").is_some() || result.get("summary").is_some());
        if command == "file.import" && !appended && warnings.is_none_or(Vec::is_empty) {
            return None;
        }
        let summary = result.get("summary");
        let count = |key: &str| summary.and_then(|s| s.get(key)).and_then(Value::as_u64);
        let mut report = Self {
            name: summary.and_then(|s| s.get("name")).and_then(Value::as_str).unwrap_or(tr("Imported document")).chars().take(512).collect(),
            pages: count("pages"),
            layers: count("layers"),
            images: count("images"),
            appended,
            notes: Vec::new(),
            note_count: 0,
            source: None,
        };
        for note in warnings.into_iter().chain(repairs).flatten().filter_map(Value::as_str) {
            report.add_note(note);
        }
        Some(report)
    }

    pub fn add_note(&mut self, note: &str) {
        self.note_count = self.note_count.saturating_add(1);
        if self.notes.len() < MAX_NOTES {
            let mut text: String = note.chars().take(MAX_NOTE_CHARS).collect();
            if note.chars().nth(MAX_NOTE_CHARS).is_some() {
                text.push('…');
            }
            self.notes.push(text);
        }
    }

    pub fn inspect(&self) -> Value {
        json!({"name": self.name, "pages": self.pages, "layers": self.layers, "images": self.images,
            "appended": self.appended, "warnings": self.notes, "warningCount": self.note_count,
            "source": self.source.as_ref().map(|(key, version)| json!({"fileKey": key, "version": version}))})
    }
}

/// `note` in the UI language when it is a known import note; a repeat count (` (×N)`) is kept.
pub(crate) fn note_text(note: &str) -> String {
    let translated = |base: &str| if IMPORT_NOTES.contains(&base) { tr(base).to_owned() } else { base.to_owned() };
    match note.rsplit_once(" (×") {
        Some((base, count)) if count.strip_suffix(')').is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit())) => {
            format!("{} (×{count}", translated(base))
        }
        _ => translated(note),
    }
}

pub fn capture(app: &mut FigApp, command: &str, result: &Value) {
    if let Some(report) = Report::from_result(command, result) {
        app.ui.import_report = Some(report);
    }
}

pub fn show(app: &mut FigApp, ctx: &egui::Context) {
    let Some(report) = &app.ui.import_report else { return };
    let t = Tokens::get(ctx);
    let mut close_clicked = false;
    let ((), closed) = crate::dialogs::card(egui::Id::new("import_report"), tr("Import report"), 420.0, ctx, |ui| {
        ui.label(egui::RichText::new(&report.name).strong());
        ui.label(if report.appended { tr("Pages added to the document") } else { tr("Opened as a new document") });
        let mut counts = Vec::new();
        for (label, count) in [(tr("Pages"), report.pages), (tr("Layers"), report.layers), (tr("Images"), report.images)] {
            if let Some(n) = count {
                counts.push(format!("{label}: {n}"));
            }
        }
        if !counts.is_empty() {
            ui.label(counts.join(" · "));
        }
        if let Some((key, version)) = &report.source {
            ui.label(trf("Figma file: {key}", &[("key", key)]));
            if let Some(version) = version {
                ui.label(trf("Version: {version}", &[("version", version)]));
            }
        }
        ui.add_space(8.0);
        if report.notes.is_empty() {
            ui.label(tr("No compatibility warnings reported"));
        } else {
            ui.label(egui::RichText::new(tr("Compatibility notes")).strong());
            ui.label(
                egui::RichText::new(tr(
                    "FigCraft doesn't support every Figma feature yet. These notes list what was left out or changed; your file itself is unchanged.",
                ))
                .color(t.text_secondary),
            );
            egui::ScrollArea::vertical().id_salt("import_report_notes").max_height(260.0).show(ui, |ui| {
                for note in &report.notes {
                    ui.label(note_text(note));
                    ui.add_space(4.0);
                }
            });
        }
        let omitted = report.note_count.saturating_sub(report.notes.len());
        if omitted > 0 {
            ui.label(trf("{n} more notes omitted from this report", &[("n", &omitted)]));
        }
        ui.label(egui::RichText::new(tr("Missing or changed parts usually mean a feature FigCraft doesn't have yet, not a problem with the file. Text uses locally available fonts; review the imported design.")).color(t.text_secondary));
        crate::dialogs::footer(ui, |ui| {
            close_clicked = crate::widgets::button(ui, tr("Close"), false, true).clicked();
        });
    });
    if closed || close_clicked {
        app.ui.import_report = None;
    }
}
