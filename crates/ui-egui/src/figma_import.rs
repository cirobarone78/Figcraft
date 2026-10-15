//! Desktop Figma REST import. Downloads run off the UI thread; only a complete bundle reaches
//! the existing file commands. Tokens are temporary and never enter engine parameters or inspection.

#[cfg(not(target_arch = "wasm32"))]
pub use native::{Dialog, inspect, is_open, keys, open, poll, preview, show};

#[cfg(target_arch = "wasm32")]
pub fn open(app: &mut crate::FigApp) {
    app.toast(crate::i18n::tr("Import from Figma requires the desktop app. You can open a local .fig or REST JSON file instead."));
}

#[cfg(target_arch = "wasm32")]
pub fn keys(_: &mut crate::FigApp, _: &egui::Context) -> bool {
    false
}

#[cfg(target_arch = "wasm32")]
pub fn is_open(_: &crate::FigApp) -> bool {
    false
}

#[cfg(target_arch = "wasm32")]
pub fn poll(_: &mut crate::FigApp, _: &egui::Context) {}

#[cfg(target_arch = "wasm32")]
pub fn show(_: &mut crate::FigApp, _: &egui::Context) {}

#[cfg(target_arch = "wasm32")]
pub fn preview(_: &mut crate::FigApp, _: &str, _: usize, _: usize) {}

#[cfg(target_arch = "wasm32")]
pub fn inspect(_: &crate::FigApp) -> serde_json::Value {
    serde_json::json!({"available": false, "open": false})
}

#[cfg(not(target_arch = "wasm32"))]
mod native {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::mpsc::{Receiver, SyncSender, TryRecvError, sync_channel};

    use figcraft_figma_net::{FigmaClient, ImportBundle, ImportError, ImportStage, Progress, parse_design_url};
    use serde_json::{Value, json};

    use crate::FigApp;
    use crate::i18n::{tr, trf};
    use crate::theme::Tokens;

    const MAX_URL_CHARS: usize = 2_048;
    const MAX_TOKEN_CHARS: usize = 1_024;

    #[derive(Clone, Copy, Default, PartialEq, Eq)]
    enum Mode {
        #[default]
        NewDocument,
        Append,
    }

    #[derive(Clone, Copy)]
    enum Destination {
        NewDocument,
        Append(u64),
    }

    /// No Debug or Serialize: this state contains a password while its field is being edited.
    #[derive(Default)]
    pub struct Dialog {
        pub url: String,
        token: String,
        mode: Mode,
        error: Option<String>,
        job: Option<Job>,
    }

    impl Drop for Dialog {
        fn drop(&mut self) {
            clear_token(&mut self.token);
        }
    }

    /// Releasing the dialog or cancelling it also stops the worker, including a late response.
    struct Job {
        cancel: Arc<AtomicBool>,
        receiver: Receiver<Event>,
        destination: Destination,
        progress: Progress,
        /// A response that arrived while presenting: applied once the presentation ends.
        finished: Option<Result<ImportBundle, ImportError>>,
        /// Only a `preview` job holds its own sender, so its idle channel never reads as a stopped
        /// worker; real jobs leave the sender to the worker thread.
        _preview: Option<SyncSender<Event>>,
    }

    impl Drop for Job {
        fn drop(&mut self) {
            self.cancel.store(true, Ordering::Release);
        }
    }

    enum Event {
        Progress(Progress),
        Done(Result<ImportBundle, ImportError>),
    }

    /// An owned token in the worker, erased on success, failure or unwinding.
    struct TemporaryToken(String);

    impl Drop for TemporaryToken {
        fn drop(&mut self) {
            clear_token(&mut self.0);
        }
    }

    fn clear_token(token: &mut String) {
        let mut bytes = std::mem::take(token).into_bytes();
        bytes.fill(0);
    }

    pub fn open(app: &mut FigApp) {
        // Repeated menu invocations keep an in-flight job; they cannot start a second download.
        if app.ui.figma_import.is_none() {
            app.ui.figma_import = Some(Dialog::default());
        }
    }

    pub fn is_open(app: &FigApp) -> bool {
        app.ui.figma_import.is_some()
    }

    fn cancel(app: &mut FigApp) {
        // Dropping Job sets cancellation before any completed response can be applied.
        app.ui.figma_import = None;
    }

    pub fn keys(app: &mut FigApp, ctx: &egui::Context) -> bool {
        if app.ui.figma_import.is_some() && ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) {
            cancel(app);
            return true;
        }
        false
    }

    fn destination(app: &FigApp, mode: Mode) -> Result<Destination, &'static str> {
        match mode {
            Mode::NewDocument => Ok(Destination::NewDocument),
            Mode::Append => app.session.ds().map(|d| Destination::Append(d.uid)).map_err(|_| tr("Open a document before adding pages to it")),
        }
    }

    fn repaint_import(ctx: &egui::Context, viewport: egui::ViewportId) {
        ctx.request_repaint_of(viewport);
        // The main window can take over the import pane after its original viewport closes.
        if viewport != egui::ViewportId::ROOT {
            ctx.request_repaint_of(egui::ViewportId::ROOT);
        }
    }

    fn start(app: &mut FigApp, ctx: &egui::Context) {
        let Some(dialog) = app.ui.figma_import.as_ref().filter(|d| d.job.is_none()) else { return };
        let dest = destination(app, dialog.mode);
        let valid = parse_design_url(&dialog.url).map(|_| ()).map_err(|e| e.to_string()).and_then(|()| {
            if dialog.token.trim().is_empty() {
                Err(tr("Enter a personal access token with file_content:read permission").to_string())
            } else {
                Ok(())
            }
        });
        let Some(dialog) = &mut app.ui.figma_import else { return };
        let destination = match valid.and_then(|()| dest.map_err(str::to_string)) {
            Ok(destination) => destination,
            Err(error) => {
                clear_token(&mut dialog.token);
                dialog.error = Some(error);
                return;
            }
        };
        let url = dialog.url.clone();
        let token = TemporaryToken(std::mem::take(&mut dialog.token));
        let cancel = Arc::new(AtomicBool::new(false));
        let worker_cancel = cancel.clone();
        let (sender, receiver) = sync_channel(4);
        let worker_ctx = ctx.clone();
        let viewport = crate::windows::viewport(app.windows.current);
        let launched = std::thread::Builder::new().name("figma-import".into()).spawn(move || {
            let result = FigmaClient::new().and_then(|client| {
                client.import(&url, &token.0, &worker_cancel, |progress| {
                    let _ = sender.try_send(Event::Progress(progress));
                    repaint_import(&worker_ctx, viewport);
                })
            });
            // No token needs to outlive the last HTTP request, even if the result waits for the UI.
            drop(token);
            let _ = sender.send(Event::Done(result));
            repaint_import(&worker_ctx, viewport);
        });
        match launched {
            Ok(_) => {
                dialog.error = None;
                dialog.job = Some(Job {
                    cancel,
                    receiver,
                    destination,
                    progress: Progress { stage: ImportStage::File, completed: 0, total: 1 },
                    finished: None,
                    _preview: None,
                });
                app.ui.import_report = None;
            }
            Err(_) => dialog.error = Some(tr("Could not start the import worker. Try again.").to_string()),
        }
    }

    /// Hand back progress and apply exactly one completed response to the initiating document.
    pub fn poll(app: &mut FigApp, ctx: &egui::Context) {
        let Some(dialog) = &mut app.ui.figma_import else { return };
        let Some(job) = &mut dialog.job else { return };
        let mut finished = job.finished.take();
        while finished.is_none() {
            match job.receiver.try_recv() {
                Ok(Event::Progress(progress)) => job.progress = progress,
                Ok(Event::Done(result)) => {
                    finished = Some(result);
                    break;
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    dialog.job = None;
                    dialog.error = Some(tr("The import worker stopped before returning a document. Try again.").to_string());
                    return;
                }
            }
        }
        let Some(result) = finished else { return };
        // Never switch or edit documents under a running presentation; keep the response.
        if app.ui.present.is_some() {
            job.finished = Some(result);
            return;
        }
        let Some(job) = dialog.job.take() else { return };
        let destination = job.destination;
        if job.cancel.load(Ordering::Acquire) {
            cancel(app);
            return;
        }
        match result {
            Ok(bundle) => {
                if apply(app, bundle, destination) {
                    cancel(app);
                    crate::windows::repaint_others(app, ctx);
                }
            }
            Err(ImportError::Cancelled) => cancel(app),
            Err(error) => {
                if let Some(dialog) = &mut app.ui.figma_import {
                    dialog.error = Some(error.to_string());
                }
            }
        }
        ctx.request_repaint();
    }

    fn apply(app: &mut FigApp, bundle: ImportBundle, destination: Destination) -> bool {
        let previous = app.session.ds().ok().map(|d| d.uid);
        let command = match destination {
            Destination::NewDocument => "file.open",
            Destination::Append(uid) => {
                let Some(index) = app.session.docs.iter().position(|d| d.uid == uid) else {
                    let error = tr("The destination document was closed. Nothing was imported; choose a new destination and try again.");
                    app.ui.last_error = Some(error.to_string());
                    if let Some(dialog) = &mut app.ui.figma_import {
                        dialog.error = Some(error.to_string());
                    }
                    return false;
                };
                app.session.active = index;
                "file.import"
            }
        };
        let result = app.run(command, bundle.command_params());
        // Appending never sends the import into a newly selected tab or changes the active tab.
        if matches!(destination, Destination::Append(_))
            && let Some(index) = previous.and_then(|uid| app.session.docs.iter().position(|d| d.uid == uid))
        {
            app.session.active = index;
        }
        let Some(result) = result else {
            if let Some(dialog) = &mut app.ui.figma_import {
                dialog.error = app.ui.last_error.clone();
            }
            return false;
        };
        // `run` captures parser notes; transport notes, including optional-variable failures,
        // accompany them in the same report. A successful REST import always returns warnings.
        let mut report = crate::import_report::Report::from_result(command, &result);
        if let Some(report) = &mut report {
            for note in &bundle.warnings {
                report.add_note(note);
            }
            report.source = Some((bundle.file_key, bundle.version.map(|version| version.chars().take(256).collect())));
        }
        app.ui.import_report = report;
        if matches!(destination, Destination::NewDocument) {
            app.ui.home = false;
        }
        true
    }

    fn progress_label(progress: Progress) -> String {
        match progress.stage {
            ImportStage::File => tr("Downloading the Figma document…").to_string(),
            ImportStage::Variables => tr("Reading local variables…").to_string(),
            ImportStage::Images => trf("Downloading images: {done} of {total}", &[("done", &progress.completed), ("total", &progress.total)]),
            ImportStage::Retry => tr("Figma rate limit reached; waiting before retry…").to_string(),
        }
    }

    /// The importing phase's title: Drafts for a new document, else the destination's name.
    fn importing_title(app: &FigApp, destination: Destination) -> String {
        match destination {
            Destination::NewDocument => tr("Importing to Drafts").to_string(),
            Destination::Append(uid) => {
                let doc = app.session.docs.iter().find(|d| d.uid == uid).or_else(|| app.session.ds().ok());
                let name = doc.map(|d| d.doc.name.clone()).unwrap_or_default();
                trf("Importing to {name}", &[("name", &name)])
            }
        }
    }

    pub fn show(app: &mut FigApp, ctx: &egui::Context) {
        let has_document = app.session.ds().is_ok();
        let title = app.ui.figma_import.as_ref().and_then(|d| d.job.as_ref()).map(|job| importing_title(app, job.destination));
        let Some(dialog) = &mut app.ui.figma_import else { return };
        let t = Tokens::get(ctx);
        let mut start_clicked = false;
        let mut cancel_clicked = false;
        // Escape belongs to `keys` (after any dialog above this one): the modal's own
        // `should_close` and backdrop clicks are deliberately not used to close it.
        crate::dialogs::modal(egui::Id::new("figma_import"), ctx).show(ctx, |ui| {
            ui.set_width(480.0);
            if let Some(job) = &dialog.job {
                ui.label(egui::RichText::new(title.unwrap_or_default()).font(crate::theme::semibold(13.0)));
                ui.add_space(12.0);
                let file = parse_design_url(&dialog.url)
                    .map(|url| trf("Figma file: {key}", &[("key", &url.file_key)]))
                    .unwrap_or_else(|_| dialog.url.trim().to_string());
                ui.horizontal(|ui| {
                    ui.add(egui::Spinner::new().size(16.0));
                    ui.label(file);
                });
                let p = job.progress;
                if let Some(error) = &dialog.error {
                    ui.add_space(8.0);
                    ui.label(egui::RichText::new(error).color(t.danger));
                }
                ui.add_space(16.0);
                ui.horizontal(|ui| {
                    ui.add(egui::Spinner::new().size(12.0));
                    // Image downloads show how far they are as a percentage beside the spinner.
                    if p.stage == ImportStage::Images && p.total > 0 {
                        let percent = (p.completed.min(p.total) * 100) / p.total;
                        ui.label(egui::RichText::new(format!("{percent} %")).font(crate::theme::semibold(11.0)));
                    }
                    ui.label(egui::RichText::new(progress_label(p)).color(t.text_secondary));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        cancel_clicked = crate::widgets::danger_button(ui, tr("Cancel"), true).clicked();
                    });
                });
                return;
            }
            ui.label(egui::RichText::new(tr("Import from Figma")).font(crate::theme::semibold(13.0)));
            ui.add_space(12.0);
            ui.label(tr("Figma design URL"));
            let url = ui.add(
                egui::TextEdit::singleline(&mut dialog.url).id(egui::Id::new("figma_import_url"))
                    .hint_text("https://www.figma.com/design/…").desired_width(f32::INFINITY).char_limit(MAX_URL_CHARS),
            );
            crate::a11y::text(&url, tr("Figma design URL"), &dialog.url);
            if !dialog.url.trim().is_empty() && let Err(error) = parse_design_url(&dialog.url) {
                ui.label(egui::RichText::new(error.to_string()).color(t.danger));
            }
            ui.add_space(8.0);
            ui.label(tr("Personal access token"));
            let token = ui.add(
                egui::TextEdit::singleline(&mut dialog.token).id(egui::Id::new("figma_import_token"))
                    .password(true).desired_width(f32::INFINITY).char_limit(MAX_TOKEN_CHARS),
            );
            // egui's password mode still records plaintext undo snapshots. Clear only this
            // field's history each frame, before Start, Escape or closing can drop the dialog.
            if let Some(mut state) = egui::TextEdit::load_state(ui.ctx(), token.id) {
                state.clear_undoer();
                state.store(ui.ctx(), token.id);
            }
            // Never pass the password to our AccessKit annotations or inspection helpers.
            crate::a11y::text(&token, tr("Personal access token"), "");
            ui.label(egui::RichText::new(tr("Token is used for this import only and is not saved. It needs file_content:read; local variables also need file_variables:read and an eligible Figma plan.")).color(t.text_secondary));
            ui.add_space(8.0);
            ui.radio_value(&mut dialog.mode, Mode::NewDocument, tr("Open as a new document"));
            ui.add_enabled_ui(has_document, |ui| {
                ui.radio_value(&mut dialog.mode, Mode::Append, tr("Add pages to the current document"));
            });
            ui.label(egui::RichText::new(tr("Imports the whole Design file, including links to a selected layer. This creates a local copy; it does not sync changes back to Figma.")).color(t.text_secondary));
            if let Some(error) = &dialog.error {
                ui.add_space(8.0);
                ui.label(egui::RichText::new(error).color(t.danger));
            }
            ui.add_space(12.0);
            // Right to left: Import ends up rightmost, Cancel to its left.
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let valid = parse_design_url(&dialog.url).is_ok() && !dialog.token.trim().is_empty()
                    && (dialog.mode == Mode::NewDocument || has_document);
                start_clicked = crate::widgets::button(ui, tr("Import"), true, valid).clicked();
                cancel_clicked = crate::widgets::button(ui, tr("Cancel"), false, true).clicked();
            });
        });
        if cancel_clicked {
            cancel(app);
        } else if start_clicked {
            start(app, ctx);
        }
    }

    /// Open the dialog in its importing phase with a synthetic job at `stage` (`file`, `variables`,
    /// `images` or `retry`), for screenshots over the control channel. Nothing is downloaded.
    pub fn preview(app: &mut FigApp, stage: &str, completed: usize, total: usize) {
        let stage = match stage {
            "variables" => ImportStage::Variables,
            "images" => ImportStage::Images,
            "retry" => ImportStage::Retry,
            _ => ImportStage::File,
        };
        // The job keeps its own sender, so the empty channel stays connected until it is dropped.
        let (sender, receiver) = sync_channel(1);
        app.ui.figma_import = Some(Dialog {
            url: "https://www.figma.com/design/PREVIEW/Preview".into(),
            token: String::new(),
            mode: Mode::NewDocument,
            error: None,
            job: Some(Job {
                cancel: Arc::new(AtomicBool::new(false)),
                receiver,
                destination: Destination::NewDocument,
                progress: Progress { stage, completed, total },
                finished: None,
                _preview: Some(sender),
            }),
        });
    }

    pub fn inspect(app: &FigApp) -> Value {
        let Some(dialog) = &app.ui.figma_import else { return json!({"available": true, "open": false}) };
        json!({"available": true, "open": true, "loading": dialog.job.is_some(), "append": dialog.mode == Mode::Append,
            "progress": dialog.job.as_ref().map(|job| json!({"stage": progress_label(job.progress), "completed": job.progress.completed, "total": job.progress.total}))})
    }

    #[cfg(test)]
    mod tests {
        include!("figma_import_tests.rs");
    }
}
