//! The central application and current-document settings dialog.

use egui::{Align, CornerRadius, Id, Layout, Rect, RichText, Sense, UiBuilder, vec2};
use figcraft_engine::Prefs;
use serde_json::json;

use crate::FigApp;
use crate::i18n::{self, tr, tr_noop, trf};
use crate::theme::{Tokens, semibold};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Category {
    #[default]
    General,
    Canvas,
    Collaboration,
}

impl Category {
    const ALL: [Self; 3] = [Self::General, Self::Canvas, Self::Collaboration];

    fn label(self) -> &'static str {
        match self {
            Self::General => tr("General"),
            Self::Canvas => tr("Canvas"),
            Self::Collaboration => tr("Collaboration"),
        }
    }
}

pub(crate) fn show(app: &mut FigApp, ctx: &egui::Context) {
    if !app.ui.preferences {
        return;
    }

    let t = Tokens::get(ctx);
    let screen = ctx.content_rect();
    let width = 720.0_f32.min((screen.width() - 32.0).max(280.0));
    let height = 640.0_f32.min((screen.height() - 32.0).max(280.0));
    let id = Id::new("preferences_modal");
    let mut open = true;
    egui::Modal::new(id).frame(egui::Frame::NONE).area(egui::Modal::default_area(id).constrain_to(screen)).show(ctx, |ui| {
        let origin = ui.min_rect().min;
        let bounds = Rect::from_min_size(origin, vec2(width, height));
        ui.set_min_size(bounds.size());
        ui.set_clip_rect(bounds);
        ui.painter().rect_filled(bounds, CornerRadius::same(8), t.panel);
        ui.painter().rect_stroke(bounds, CornerRadius::same(8), egui::Stroke::new(1.0, t.border), egui::StrokeKind::Inside);

        let header_rect = Rect::from_min_size(bounds.min, vec2(width, 56.0));
        let mut header = ui.new_child(UiBuilder::new().max_rect(header_rect).layout(Layout::left_to_right(Align::Center)));
        header.set_clip_rect(header_rect);
        header.add_space(22.0);
        header.label(RichText::new(tr("Preferences")).font(semibold(18.0)).color(t.text));
        header.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.add_space(12.0);
            let close = ui.add_sized(vec2(28.0, 28.0), egui::Button::new(RichText::new("×").size(18.0))).on_hover_text(tr("Close"));
            close.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, tr("Close")));
            if close.clicked() {
                open = false;
            }
        });
        ui.painter().hline(bounds.x_range(), header_rect.bottom(), egui::Stroke::new(1.0, t.border));

        let body_rect = Rect::from_min_max(bounds.left_bottom() + vec2(0.0, -height + 56.0), bounds.right_bottom());
        let nav_width = (width * 0.29).clamp(140.0, 190.0);
        let nav_rect = Rect::from_min_size(body_rect.min, vec2(nav_width, body_rect.height()));
        let content_rect = Rect::from_min_max(nav_rect.right_top() + vec2(20.0, 0.0), body_rect.right_bottom() - vec2(20.0, 0.0));
        let mut nav = ui.new_child(UiBuilder::new().max_rect(nav_rect.shrink2(vec2(10.0, 0.0))).layout(Layout::top_down(Align::Min)));
        nav.set_clip_rect(nav_rect);
        nav.add_space(14.0);
        for category in Category::ALL {
            let selected = app.ui.preferences_category == category;
            let (rect, response) = nav.allocate_exact_size(vec2(nav_width - 20.0, 38.0), Sense::click());
            if selected || response.hovered() {
                nav.painter().rect_filled(rect, CornerRadius::same(6), if selected { t.hover } else { t.field });
            }
            nav.painter().text(rect.left_center() + vec2(12.0, 0.0), egui::Align2::LEFT_CENTER, category.label(), semibold(13.0), t.text);
            response.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::SelectableLabel, true, selected, category.label()));
            if response.clicked() {
                app.ui.preferences_category = category;
            }
        }
        ui.painter().vline(nav_rect.right(), body_rect.y_range(), egui::Stroke::new(1.0, t.border));

        let mut content = ui.new_child(UiBuilder::new().max_rect(content_rect).layout(Layout::top_down(Align::Min)));
        content.set_clip_rect(content_rect);
        egui::ScrollArea::vertical().id_salt("preferences_content").max_height(content_rect.height()).auto_shrink([false, false]).show(
            &mut content,
            |ui| {
                ui.set_min_width((content_rect.width() - 24.0).max(180.0));
                ui.set_max_width(content_rect.width() - 20.0);
                ui.add_space(16.0);
                match app.ui.preferences_category {
                    Category::General => general(app, ui, &t),
                    Category::Canvas => canvas(app, ui, &t),
                    Category::Collaboration => collaboration(app, ui, &t),
                }
                ui.add_space(20.0);
            },
        );
    });
    if !open {
        app.ui.preferences = false;
    }
}

fn section(ui: &mut egui::Ui, t: &Tokens, title: &str) {
    ui.label(RichText::new(title).font(semibold(14.0)).color(t.text));
    ui.add_space(8.0);
}

fn general(app: &mut FigApp, ui: &mut egui::Ui, t: &Tokens) {
    section(ui, t, tr("General"));
    let prefs = app.session.prefs.clone();
    let mut toggles = Vec::new();
    for (label, value, command) in [
        (tr("Keep tool selected after use"), prefs.keep_tool, "pref.keepTool"),
        (tr("Highlight layers on hover"), prefs.highlight_on_hover, "pref.highlightHover"),
        (tr("Rename duplicated layers"), prefs.rename_duplicates, "pref.renameDuplicates"),
        (tr("Show dimensions on objects"), prefs.show_dimensions, "pref.showDimensions"),
        (tr("Use number keys for opacity"), prefs.number_keys_opacity, "pref.numberKeysOpacity"),
        (tr("Reduce motion"), prefs.reduce_motion, "pref.reduceMotion"),
    ] {
        if crate::widgets::checkbox(ui, value, label) {
            toggles.push(command);
        }
    }
    if app.services.reduce_motion() {
        crate::widgets::label(ui, tr("Your system asks for reduced motion: animations stay off."), t.text_secondary);
    }
    for command in toggles {
        app.run(command, json!({}));
    }

    ui.add_space(18.0);
    section(ui, t, tr("Appearance"));
    ui.label(tr("Theme"));
    ui.horizontal_wrapped(|ui| {
        for (label, command) in [(tr_noop("Light"), "view.themeLight"), (tr_noop("Dark"), "view.themeDark"), (tr_noop("System"), "view.themeSystem")]
        {
            if ui.selectable_label(prefs.theme.eq_ignore_ascii_case(label), tr(label)).clicked() {
                app.run(command, json!({}));
            }
        }
    });
    ui.add_space(12.0);
    // Each language by its own name; System follows the OS (the browser on the web).
    ui.label(tr("Language"));
    egui::ComboBox::from_id_salt("preferences_language")
        .selected_text(
            i18n::languages()
                .iter()
                .find(|l| l.code.eq_ignore_ascii_case(&prefs.language))
                .map_or_else(|| tr("System").to_owned(), |l| l.name.to_owned()),
        )
        .width(ui.available_width().min(280.0))
        .show_ui(ui, |ui| {
            for (code, name) in [("system", tr("System").to_owned())]
                .into_iter()
                .chain(i18n::languages().iter().filter(|l| crate::cjk::can_show(l.code)).map(|l| (l.code, l.name.to_owned())))
            {
                if ui.selectable_label(prefs.language.eq_ignore_ascii_case(code), name).clicked() {
                    app.run("pref.language", json!({"language": code}));
                    ui.ctx().request_repaint();
                }
            }
        });
    if let Some(lang) = i18n::current().filter(|l| l.draft) {
        let note = trf("{language}: draft translation, waiting for a native speaker's review.", &[("language", &lang.name)]);
        crate::widgets::label(ui, &note, t.text_secondary);
    }
}

fn canvas(app: &mut FigApp, ui: &mut egui::Ui, t: &Tokens) {
    section(ui, t, tr("Canvas"));
    let view = app.session.ds().ok().map(|ds| ds.view.clone());
    if let Some(view) = view {
        let mut changes = Vec::new();
        for (label, value, command) in [
            (tr("Pixel grid"), view.pixel_grid, "view.pixelGrid"),
            (tr("Layout grids"), view.layout_grids, "view.layoutGrids"),
            (tr("Snap to pixel grid"), view.snap_pixel, "view.snapPixel"),
            (tr("Snap to objects"), view.snap_objects, "view.snapObjects"),
        ] {
            if crate::widgets::checkbox(ui, value, label) {
                changes.push((command, !value));
            }
        }
        for (command, on) in changes {
            app.run(command, json!({"on": on}));
        }
        crate::widgets::label(ui, tr("These view and snapping options apply to the current document."), t.text_secondary);
    } else {
        ui.add_enabled_ui(false, |ui| {
            for label in ["Pixel grid", "Layout grids", "Snap to pixel grid", "Snap to objects"] {
                crate::widgets::checkbox(ui, false, tr(label));
            }
        });
        crate::widgets::label(ui, tr("Open a document to change its canvas options."), t.text_secondary);
    }

    ui.add_space(18.0);
    section(ui, t, tr("Nudge amount"));
    let prefs = app.session.prefs.clone();
    ui.horizontal_wrapped(|ui| {
        ui.label(tr("Small"));
        let mut small = prefs.nudge_small;
        let a = ui.add(egui::DragValue::new(&mut small).range(0.1..=100.0));
        ui.label(tr("Large"));
        let mut big = prefs.nudge_big;
        let b = ui.add(egui::DragValue::new(&mut big).range(1.0..=1000.0));
        if a.changed() || b.changed() {
            app.run("pref.nudge", json!({"small": small, "big": big}));
        }
    });
}

fn collaboration(app: &mut FigApp, ui: &mut egui::Ui, t: &Tokens) {
    section(ui, t, tr("Collaboration"));
    let prefs: Prefs = app.session.prefs.clone();
    // Where the app registers itself as the opener of invite links (Windows).
    if let Some(on) = app.session.invite_links_on() {
        if crate::widgets::checkbox(ui, on, tr("Open invite links in FigCraft")) {
            app.run("collab.registerLinks", json!({"on": !on}));
        }
        crate::widgets::label(ui, tr("figcraft:// links from a browser, chat or email open this app and join the session."), t.text_secondary);
    }

    ui.add_space(18.0);
    ui.label(tr("Your name in comments"));
    let name_width = ui.available_width().min(360.0);
    if let Some(name) = crate::widgets::text_field(ui, Id::new("pref_user_name"), &prefs.user_name, name_width, tr("You")) {
        app.run("pref.userName", json!({"name": name}));
    }
    ui.add_space(14.0);
    ui.label(tr("Server for online sharing"));
    // Where Share ▸ Online keeps files and join codes are asked; no key here
    // (preferences are saved as text).
    ui.horizontal_wrapped(|ui| {
        let name = tr("Server for online sharing");
        let field_width = (ui.available_width() - 72.0).clamp(120.0, 360.0);
        if let Some(url) = crate::widgets::text_field_named(ui, Id::new("pref_publish_server"), &prefs.publish_server, field_width, name) {
            app.run("pref.publishServer", json!({"url": url}));
        }
        let usual = prefs.publish_server == figcraft_engine::live::DEFAULT_SERVER;
        if crate::widgets::button(ui, tr("Reset"), false, !usual).on_hover_text(tr("Back to the FigCraft server")).clicked() {
            app.run("pref.publishServer", json!({}));
        }
    });
    if prefs.publish_server.is_empty() {
        crate::widgets::label(ui, tr("Online sharing is off: files are shared on the local network only."), t.text_secondary);
    }
}
