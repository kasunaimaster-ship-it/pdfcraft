//! The phone layout of the browser build: a slim top bar and a bottom bar of large buttons in
//! place of the desktop tab strip, mode bar and side panels, which do not fit a phone.
//!
//! It applies only to the browser build in a window narrower than [`PHONE_WIDTH`] points, so
//! desktop windows and desktop browsers are untouched. Everything reachable from the desktop
//! layout stays reachable: "All tools" opens the command palette, which finds every command.

use crate::PdfCraftApp;
use crate::theme::{self, Tokens};

/// Windows narrower than this (in points at zoom 1) get the phone layout.
const PHONE_WIDTH: f32 = 700.0;
const TOP_HEIGHT: f32 = 48.0;
const BOTTOM_HEIGHT: f32 = 68.0;
const BUTTON_HEIGHT: f32 = 48.0;

/// Whether the phone layout is in use.
pub(crate) fn active(ctx: &egui::Context) -> bool {
    cfg!(target_arch = "wasm32") && ctx.input(|i| i.viewport_rect().width()) * ctx.zoom_factor() < PHONE_WIDTH
}

/// The slim bar at the top: the name, and a way back to Home while a document is open.
pub(crate) fn top_bar(app: &mut PdfCraftApp, ui: &mut egui::Ui) {
    let t = Tokens::get(ui.ctx());
    egui::Panel::top("phone_top").exact_size(TOP_HEIGHT).frame(egui::Frame::NONE.fill(t.titlebar).inner_margin(egui::Margin::symmetric(14, 0))).show(
        ui,
        |ui| {
            ui.horizontal_centered(|ui| {
                ui.label(egui::RichText::new("PdfCraft").font(theme::semibold(18.0)).color(t.text));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if app.active.is_some() && ui.add_sized([84.0, 36.0], egui::Button::new("Home")).clicked() {
                        app.active = None;
                    }
                });
            });
        },
    );
}

/// What a bottom-bar button does.
#[derive(Clone, Copy)]
enum Action {
    Open,
    Translate,
    AllTools,
}

/// The bar of large buttons at the bottom.
pub(crate) fn bottom_bar(app: &mut PdfCraftApp, ui: &mut egui::Ui) {
    let t = Tokens::get(ui.ctx());
    let mut chosen = None;
    egui::Panel::bottom("phone_bottom")
        .exact_size(BOTTOM_HEIGHT)
        .frame(egui::Frame::NONE.fill(t.titlebar).inner_margin(egui::Margin::symmetric(10, 0)))
        .show(ui, |ui| {
            let gap = 8.0;
            let width = ((ui.available_width() - gap * 2.0) / 3.0).max(40.0);
            ui.spacing_mut().item_spacing.x = gap;
            ui.horizontal_centered(|ui| {
                for (label, action) in [("Open PDF", Action::Open), ("Translate", Action::Translate), ("All tools", Action::AllTools)] {
                    let text = egui::RichText::new(label).font(theme::semibold(15.0));
                    if ui.add_sized([width, BUTTON_HEIGHT], egui::Button::new(text)).clicked() {
                        chosen = Some(action);
                    }
                }
            });
        });
    match chosen {
        Some(Action::Open) => app.open_dialog(),
        Some(Action::Translate) => {
            // With no document open the command refuses (and says so): ask for a file instead.
            if !app.execute("export.translate") {
                app.open_dialog();
            }
        }
        Some(Action::AllTools) => {
            app.execute("view.palette");
        }
        None => {}
    }
}
