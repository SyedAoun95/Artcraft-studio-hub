//! The hub window: an app launcher (icon grid, click to open an app with AI control), the AI-control
//! switch, connections to AI tools, and what agents did.

use crate::activity::{self, Entry};
use crate::apps::{self, App, Status, APPS};
use crate::config::{home, Config, PRODUCT};
use crate::connect::{self, Client, CLIENTS};
use eframe::egui::{self, pos2, vec2, Align2, Color32, CornerRadius, FontId, Rect, RichText, Sense, Stroke, TextureHandle};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

// Palette
const BG: Color32 = Color32::from_rgb(24, 25, 28);
const PANEL: Color32 = Color32::from_rgb(30, 31, 35);
const CARD: Color32 = Color32::from_rgb(37, 38, 43);
const HOVER: Color32 = Color32::from_rgb(45, 47, 53);
const LINE: Color32 = Color32::from_rgb(52, 54, 61);
const TEXT: Color32 = Color32::from_rgb(236, 237, 240);
const MUTED: Color32 = Color32::from_rgb(150, 154, 163);
const ACCENT: Color32 = Color32::from_rgb(47, 123, 245);
const GREEN: Color32 = Color32::from_rgb(52, 199, 89);
const AMBER: Color32 = Color32::from_rgb(255, 176, 32);
const RED: Color32 = Color32::from_rgb(255, 92, 92);

const ICONS: [(&str, &[u8], &[u8]); 7] = [
    ("photocraft", include_bytes!("../assets/icons/photocraft-192.png"), include_bytes!("../assets/icons/photocraft-48.png")),
    ("vectorcraft", include_bytes!("../assets/icons/vectorcraft-192.png"), include_bytes!("../assets/icons/vectorcraft-48.png")),
    ("lightcraft", include_bytes!("../assets/icons/lightcraft-192.png"), include_bytes!("../assets/icons/lightcraft-48.png")),
    ("designcraft", include_bytes!("../assets/icons/designcraft-192.png"), include_bytes!("../assets/icons/designcraft-48.png")),
    ("filmcraft", include_bytes!("../assets/icons/filmcraft-192.png"), include_bytes!("../assets/icons/filmcraft-48.png")),
    ("effectcraft", include_bytes!("../assets/icons/effectcraft-192.png"), include_bytes!("../assets/icons/effectcraft-48.png")),
    ("pdfcraft", include_bytes!("../assets/icons/pdfcraft-192.png"), include_bytes!("../assets/icons/pdfcraft-48.png")),
];
const HUB_ICON: &[u8] = include_bytes!("../assets/icons/studio-hub-96.png");

#[derive(Default)]
struct Shared {
    statuses: Vec<Status>,
    activity: Vec<Entry>,
    connected: Vec<(Client, bool, connect::Link)>, // (client, installed, link state)
    busy: Vec<String>,                   // app ids being opened or reopened
    message: Option<(String, bool, Instant)>,
}

struct Icons {
    large: Vec<TextureHandle>,
    small: Vec<TextureHandle>,
    hub: Option<TextureHandle>,
}

pub struct Hub {
    cfg: Config,
    shared: Arc<Mutex<Shared>>,
    icons: Icons,
}

fn decode(bytes: &[u8]) -> Option<egui::ColorImage> {
    let img = image::load_from_memory_with_format(bytes, image::ImageFormat::Png).ok()?.to_rgba8();
    let size = [img.width() as usize, img.height() as usize];
    Some(egui::ColorImage::from_rgba_unmultiplied(size, img.as_raw()))
}

fn texture(ctx: &egui::Context, name: &str, bytes: &[u8]) -> Option<TextureHandle> {
    decode(bytes).map(|img| ctx.load_texture(name, img, egui::TextureOptions::LINEAR))
}

fn setup_style(ctx: &egui::Context) {
    // Use the system font (San Francisco / Segoe UI) when it is there; egui's own font stays as the fallback.
    let mut fonts = egui::FontDefinitions::default();
    let system_font = if cfg!(windows) { r"C:\Windows\Fonts\segoeui.ttf" } else { "/System/Library/Fonts/SFNS.ttf" };
    if let Ok(bytes) = std::fs::read(system_font) {
        fonts.font_data.insert("sf".into(), Arc::new(egui::FontData::from_owned(bytes)));
        if let Some(f) = fonts.families.get_mut(&egui::FontFamily::Proportional) {
            f.insert(0, "sf".into());
        }
    }
    ctx.set_fonts(fonts);
    let mut v = egui::Visuals::dark();
    v.panel_fill = BG;
    v.window_fill = PANEL;
    v.extreme_bg_color = Color32::from_rgb(20, 21, 24);
    v.override_text_color = Some(TEXT);
    v.widgets.inactive.weak_bg_fill = CARD;
    v.widgets.inactive.bg_fill = CARD;
    v.widgets.hovered.weak_bg_fill = HOVER;
    v.widgets.active.weak_bg_fill = LINE;
    for w in [&mut v.widgets.inactive, &mut v.widgets.hovered, &mut v.widgets.active, &mut v.widgets.noninteractive] {
        w.corner_radius = CornerRadius::same(8);
    }
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, LINE);
    v.selection.bg_fill = ACCENT;
    ctx.set_visuals(v);
    ctx.all_styles_mut(|s| {
        s.spacing.item_spacing = vec2(8.0, 8.0);
        s.spacing.button_padding = vec2(12.0, 5.0);
    });
}

impl Hub {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        setup_style(&cc.egui_ctx);
        let ctx = &cc.egui_ctx;
        let icons = Icons {
            large: ICONS.iter().filter_map(|(id, big, _)| texture(ctx, &format!("{id}-l"), big)).collect(),
            small: ICONS.iter().filter_map(|(id, _, small)| texture(ctx, &format!("{id}-s"), small)).collect(),
            hub: texture(ctx, "hub", HUB_ICON),
        };
        let cfg = Config::load();
        let shared = Arc::new(Mutex::new(Shared::default()));
        // Poll app status, connections and the activity log off the UI thread.
        let (s, ctx) = (shared.clone(), cc.egui_ctx.clone());
        std::thread::spawn(move || loop {
            let cfg = Config::load();
            let st = apps::statuses(&cfg);
            let act = activity::tail(60);
            let h = home();
            let conn: Vec<_> = CLIENTS.iter().map(|c| (*c, connect::available(*c, &h), connect::link(*c, &h))).collect();
            if let Ok(mut g) = s.lock() {
                g.statuses = st;
                g.activity = act;
                g.connected = conn;
            }
            ctx.request_repaint();
            std::thread::sleep(Duration::from_millis(1500));
        });
        Hub { cfg, shared, icons }
    }

    fn say(&self, text: impl Into<String>, err: bool) {
        if let Ok(mut g) = self.shared.lock() {
            g.message = Some((text.into(), err, Instant::now()));
        }
    }

    /// Run an app action (open / reopen) in the background with a spinner on its icon.
    fn run_bg(&self, idx: usize, ctx: &egui::Context, reopen: bool) {
        let app = &APPS[idx];
        let (s, cfg, ctx) = (self.shared.clone(), self.cfg.clone(), ctx.clone());
        if let Ok(mut g) = s.lock() {
            if g.busy.iter().any(|b| b == app.id) {
                return;
            }
            g.busy.push(app.id.into());
        }
        std::thread::spawn(move || {
            let r = if reopen { apps::reopen_with_agent(app, &cfg) } else { apps::launch(app, &cfg).map_err(|e| e.to_string()) };
            if let Ok(mut g) = s.lock() {
                g.busy.retain(|b| b != app.id);
                g.message = Some(match r {
                    Ok(_) => (format!("{} is open. Your AI agents can control it now.", app.name), false, Instant::now()),
                    Err(e) => (format!("{}: {e}", app.name), true, Instant::now()),
                });
            }
            ctx.request_repaint();
        });
    }

    fn click_app(&self, idx: usize, st: &Status, ctx: &egui::Context) {
        let app = &APPS[idx];
        if !st.installed {
            self.say(format!("{} is not installed.", app.name), true);
        } else if !st.window_open {
            self.run_bg(idx, ctx, false);
        } else if app.bundle().is_some() {
            if let Err(e) = apps::show(app) {
                self.say(format!("{}: {e}", app.name), true);
            } else if app.port.is_some() && !st.agent_ready {
                self.say(format!("{} was opened outside Studio Hub, so AI can't control it. Right-click it and choose “Reopen with AI control”.", app.name), true);
            }
        } else {
            self.say(format!("{} is already open.", app.name), false);
        }
    }
}

/// An iOS-style switch.
fn switch(ui: &mut egui::Ui, on: &mut bool) -> egui::Response {
    let size = vec2(40.0, 22.0);
    let (rect, mut resp) = ui.allocate_exact_size(size, Sense::click());
    if resp.clicked() {
        *on = !*on;
        resp.mark_changed();
    }
    let t = ui.ctx().animate_bool(resp.id, *on);
    let off_col = Color32::from_rgb(72, 75, 84);
    let col = Color32::from_rgb(
        (off_col.r() as f32 + (GREEN.r() as f32 - off_col.r() as f32) * t) as u8,
        (off_col.g() as f32 + (GREEN.g() as f32 - off_col.g() as f32) * t) as u8,
        (off_col.b() as f32 + (GREEN.b() as f32 - off_col.b() as f32) * t) as u8,
    );
    let r = rect.height() / 2.0;
    ui.painter().rect_filled(rect, CornerRadius::same(r as u8), col);
    let cx = rect.left() + r + (rect.width() - 2.0 * r) * t;
    ui.painter().circle_filled(pos2(cx, rect.center().y), r - 3.0, Color32::WHITE);
    resp
}

fn card<R>(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    egui::Frame::new()
        .fill(CARD)
        .stroke(Stroke::new(1.0, LINE))
        .corner_radius(CornerRadius::same(12))
        .inner_margin(egui::Margin::same(14))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            add(ui)
        })
        .inner
}

fn pill(ui: &mut egui::Ui, text: &str, color: Color32) {
    let galley = ui.painter().layout_no_wrap(text.to_string(), FontId::proportional(11.5), color);
    let size = galley.size() + vec2(18.0, 6.0);
    let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
    ui.painter().rect_filled(rect, CornerRadius::same(255), color.gamma_multiply(0.16));
    ui.painter().circle_filled(pos2(rect.left() + 8.0, rect.center().y), 3.0, color);
    ui.painter().galley(pos2(rect.left() + 14.0, rect.center().y - galley.size().y / 2.0), galley, color);
}

fn ago(ts: u64) -> String {
    let d = activity::now().saturating_sub(ts);
    match d {
        0..=59 => "just now".into(),
        60..=3599 => format!("{} min ago", d / 60),
        3600..=86399 => format!("{} h ago", d / 3600),
        _ => format!("{} d ago", d / 86400),
    }
}

impl Hub {
    /// One launcher tile: big icon, status badge, name and a short label underneath.
    fn tile(&mut self, ui: &mut egui::Ui, idx: usize, st: &Status, busy: bool) {
        let app: &App = &APPS[idx];
        let (rect, resp) = ui.allocate_exact_size(vec2(132.0, 150.0), Sense::click());
        let p = ui.painter_at(rect.expand(4.0));
        if resp.hovered() {
            p.rect_filled(rect, CornerRadius::same(16), HOVER);
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        let icon_rect = Rect::from_center_size(pos2(rect.center().x, rect.top() + 54.0), vec2(84.0, 84.0));
        let pressed = resp.is_pointer_button_down_on();
        let icon_rect = if pressed { icon_rect.shrink(3.0) } else { icon_rect };
        if let Some(tex) = self.icons.large.get(idx) {
            let tint = if st.installed { Color32::WHITE } else { Color32::from_white_alpha(90) };
            p.image(tex.id(), icon_rect, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), tint);
        }
        if busy {
            p.rect_filled(icon_rect.shrink(6.0), CornerRadius::same(18), Color32::from_black_alpha(140));
            egui::Spinner::new().size(26.0).color(Color32::WHITE).paint_at(ui, Rect::from_center_size(icon_rect.center(), vec2(26.0, 26.0)));
        }
        // Status badge: green = AI can control it, amber = open without AI control.
        let badge = if !st.window_open {
            None
        } else if app.port.is_none() || st.agent_ready {
            Some(GREEN)
        } else {
            Some(AMBER)
        };
        if let Some(c) = badge {
            let at = pos2(icon_rect.right() - 6.0, icon_rect.bottom() - 6.0);
            p.circle_filled(at, 9.0, BG);
            p.circle_filled(at, 6.5, c);
        }
        if !self.cfg.app_allowed(app.id) && self.cfg.agents_enabled {
            let at = pos2(icon_rect.left() + 6.0, icon_rect.bottom() - 6.0);
            p.circle_filled(at, 9.0, BG);
            // "AI not allowed" badge: a struck-through circle (drawn, not a font glyph).
            p.circle_stroke(at, 5.5, Stroke::new(1.6, MUTED));
            p.line_segment([at + vec2(-3.9, 3.9), at + vec2(3.9, -3.9)], Stroke::new(1.6, MUTED));
        }
        p.text(pos2(rect.center().x, icon_rect.bottom() + 16.0), Align2::CENTER_CENTER, app.name, FontId::proportional(14.0), TEXT);
        p.text(pos2(rect.center().x, icon_rect.bottom() + 34.0), Align2::CENTER_CENTER, app.short, FontId::proportional(11.5), MUTED);

        let state = if !st.installed {
            "Not installed".to_string()
        } else if busy {
            "Opening…".to_string()
        } else if !st.window_open {
            if app.port.is_none() { "Closed · AI can still work on PDF files".into() } else { "Closed · click to open with AI control".into() }
        } else if app.port.is_none() || st.agent_ready {
            "Open · AI can control it · click to bring to front".into()
        } else {
            "Open outside Studio Hub · AI can't control it · right-click to reopen with AI control".into()
        };
        let resp = resp.on_hover_ui(|ui| {
            ui.strong(format!("{}  ·  like {}", app.name, app.like));
            ui.label(RichText::new(app.what).color(MUTED));
            ui.label(state);
        });
        if resp.clicked() && !busy {
            let ctx = ui.ctx().clone();
            self.click_app(idx, st, &ctx);
        }
        resp.context_menu(|ui| {
            ui.set_min_width(220.0);
            ui.label(RichText::new(app.name).strong());
            ui.separator();
            if st.window_open {
                if app.bundle().is_some() && ui.button("Bring to front").clicked() {
                    let _ = apps::show(app);
                    ui.close();
                }
                if app.port.is_some() && !st.agent_ready && app.bundle().is_some() && ui.button("Reopen with AI control").clicked() {
                    let ctx = ui.ctx().clone();
                    self.run_bg(idx, &ctx, true);
                    ui.close();
                }
            } else if st.installed && ui.button("Open with AI control").clicked() {
                let ctx = ui.ctx().clone();
                self.run_bg(idx, &ctx, false);
                ui.close();
            }
            let mut allowed = !self.cfg.disabled_apps.iter().any(|d| d == app.id);
            if ui.checkbox(&mut allowed, "Allow AI control").changed() {
                self.cfg.disabled_apps.retain(|d| d != app.id);
                if !allowed {
                    self.cfg.disabled_apps.push(app.id.into());
                }
                let _ = self.cfg.save();
            }
        });
    }
}

impl eframe::App for Hub {
    fn ui(&mut self, root: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = &root.ctx().clone();
        let (statuses, activity, connected, busy, message) = self.shared.lock()
            .map(|g| (g.statuses.clone(), g.activity.clone(), g.connected.clone(), g.busy.clone(), g.message.clone()))
            .unwrap_or_default();

        // ---- header
        egui::Panel::top("header").frame(egui::Frame::new().fill(PANEL).inner_margin(egui::Margin::symmetric(20, 14)).stroke(Stroke::new(1.0, LINE))).show(root, |ui| {
            ui.horizontal(|ui| {
                if let Some(t) = &self.icons.hub {
                    ui.add(egui::Image::new(t).fit_to_exact_size(vec2(38.0, 38.0)));
                }
                ui.vertical(|ui| {
                    ui.add_space(1.0);
                    ui.label(RichText::new(PRODUCT).size(19.0).strong());
                    ui.label(RichText::new("Your creative apps, one place, ready for AI").size(12.0).color(MUTED));
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let mut on = self.cfg.agents_enabled;
                    if switch(ui, &mut on).on_hover_text("When off, connected AI tools can't control any app").changed() {
                        self.cfg.agents_enabled = on;
                        if let Err(e) = self.cfg.save() {
                            self.say(format!("Could not save settings: {e}"), true);
                        }
                    }
                    ui.label(RichText::new(if self.cfg.agents_enabled { "AI control on" } else { "AI control off" })
                        .size(13.0).color(if self.cfg.agents_enabled { TEXT } else { MUTED }));
                });
            });
        });

        // ---- footer
        let ready = APPS.iter().zip(statuses.iter()).filter(|(a, s)| s.window_open && (a.port.is_none() || s.agent_ready)).count();
        egui::Panel::bottom("footer").frame(egui::Frame::new().fill(PANEL).inner_margin(egui::Margin::symmetric(20, 8)).stroke(Stroke::new(1.0, LINE))).show(root, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new("AI work folder").size(12.0).color(MUTED));
                ui.label(RichText::new(self.cfg.work_dir.display().to_string()).size(12.0));
                if ui.link(RichText::new("Open").size(12.0)).clicked() {
                    let _ = std::fs::create_dir_all(&self.cfg.work_dir);
                    let opener = if cfg!(windows) { "explorer" } else { "/usr/bin/open" };
                    let _ = std::process::Command::new(opener).arg(&self.cfg.work_dir).spawn();
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(RichText::new(format!("{ready} of {} apps ready for AI", APPS.len())).size(12.0).color(MUTED));
                });
            });
        });

        // ---- right column: connections and activity
        egui::Panel::right("side").resizable(false).exact_size(320.0)
            .frame(egui::Frame::new().fill(BG).inner_margin(egui::Margin { left: 0, right: 18, top: 18, bottom: 18 }))
            .show(root, |ui| {
                card(ui, |ui| {
                    ui.label(RichText::new("AI connections").size(15.0).strong());
                    ui.label(RichText::new("One connection gives your AI tool all seven apps.").size(12.0).color(MUTED));
                    ui.add_space(6.0);
                    for (client, installed, state) in &connected {
                        ui.horizontal(|ui| {
                            ui.set_min_height(30.0);
                            ui.label(RichText::new(client.label()).size(13.5));
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                let accent = |t: &str| egui::Button::new(RichText::new(t).color(Color32::WHITE).size(12.5)).fill(ACCENT);
                                if !installed {
                                    ui.label(RichText::new("Not installed").size(12.0).color(MUTED));
                                } else {
                                    match state {
                                        connect::Link::Connected => {
                                            if ui.small_button("Disconnect").clicked() {
                                                let r = connect::disconnect(*client, &home());
                                                self.say(r.clone().unwrap_or_else(|e| e), r.is_err());
                                            }
                                            pill(ui, "Connected", GREEN);
                                        }
                                        connect::Link::Stale => {
                                            if ui.add(accent("Fix")).on_hover_text("Connected to a copy of Studio Hub that moved, was deleted or ran from the disk image. Fix points it at this installed copy.").clicked() {
                                                let r = connect::connect(*client, &home());
                                                self.say(r.clone().unwrap_or_else(|e| e), r.is_err());
                                            }
                                            pill(ui, "Needs fixing", AMBER);
                                        }
                                        connect::Link::NotConnected => {
                                            if ui.add(accent("Connect")).clicked() {
                                                let r = connect::connect(*client, &home());
                                                self.say(r.clone().unwrap_or_else(|e| e), r.is_err());
                                            }
                                        }
                                    }
                                }
                            });
                        });
                    }
                    ui.horizontal(|ui| {
                        ui.set_min_height(30.0);
                        ui.label(RichText::new("Cursor, VS Code, others").size(13.5));
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.small_button("Copy setup").clicked() {
                                ctx.copy_text(connect::generic_snippet());
                                self.say("Setup copied. Paste it into your tool's MCP settings.", false);
                            }
                        });
                    });
                });
                ui.add_space(12.0);
                card(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("Recent AI activity").size(15.0).strong());
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if !activity.is_empty() && ui.link(RichText::new("Clear").size(12.0)).clicked() {
                                activity::clear();
                            }
                        });
                    });
                    ui.add_space(4.0);
                    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                        if activity.is_empty() {
                            ui.add_space(8.0);
                            ui.label(RichText::new("Nothing yet. When an AI agent uses an app, each step appears here.").size(12.0).color(MUTED));
                        }
                        for e in &activity {
                            ui.horizontal(|ui| {
                                let idx = APPS.iter().position(|a| a.id == e.app);
                                match idx.and_then(|i| self.icons.small.get(i)) {
                                    Some(t) => { ui.add(egui::Image::new(t).fit_to_exact_size(vec2(20.0, 20.0))); }
                                    None => { if let Some(t) = &self.icons.hub { ui.add(egui::Image::new(t).fit_to_exact_size(vec2(20.0, 20.0))); } }
                                }
                                ui.vertical(|ui| {
                                    ui.spacing_mut().item_spacing.y = 1.0;
                                    ui.label(RichText::new(e.tool.replace('_', " ")).size(12.5).color(if e.ok { TEXT } else { RED }));
                                    let who = if e.client.is_empty() { String::new() } else { format!(" · {}", e.client) };
                                    ui.label(RichText::new(format!("{}{who}", ago(e.ts))).size(11.0).color(MUTED));
                                    if !e.ok && !e.note.is_empty() {
                                        ui.label(RichText::new(&e.note).size(11.0).color(MUTED));
                                    }
                                });
                            });
                            ui.add_space(2.0);
                        }
                    });
                });
            });

        // ---- the launcher
        egui::CentralPanel::default().frame(egui::Frame::new().fill(BG).inner_margin(egui::Margin::same(22))).show(root, |ui| {
            if connect::running_from_disk_image() {
                egui::Frame::new().fill(AMBER.gamma_multiply(0.14)).corner_radius(CornerRadius::same(10)).inner_margin(egui::Margin::symmetric(12, 8)).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.label(RichText::new("You're running Studio Hub from the disk image. Drag Studio Hub and the apps into Applications, then open Studio Hub from Applications.").size(12.5).color(AMBER));
                });
                ui.add_space(10.0);
            }
            ui.label(RichText::new("Apps").size(22.0).strong());
            ui.label(RichText::new("Click an app to open it. Apps opened here can be controlled by your AI agents. Right-click for more.").size(12.5).color(MUTED));
            ui.add_space(10.0);
            if let Some((m, err, at)) = &message {
                if at.elapsed() < Duration::from_secs(9) {
                    egui::Frame::new().fill(if *err { AMBER.gamma_multiply(0.14) } else { GREEN.gamma_multiply(0.14) })
                        .corner_radius(CornerRadius::same(10)).inner_margin(egui::Margin::symmetric(12, 8)).show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            ui.label(RichText::new(m).size(12.5).color(if *err { AMBER } else { GREEN }));
                        });
                    ui.add_space(8.0);
                    ctx.request_repaint_after(Duration::from_millis(500));
                }
            }
            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = vec2(10.0, 12.0);
                    for i in 0..APPS.len() {
                        let st = statuses.get(i).cloned().unwrap_or_default();
                        let is_busy = busy.iter().any(|b| b == APPS[i].id);
                        self.tile(ui, i, &st, is_busy);
                    }
                });
                if !self.cfg.agents_enabled {
                    ui.add_space(14.0);
                    ui.label(RichText::new("AI control is off. Apps work normally, but connected AI tools can't control them until you switch it back on.").size(12.5).color(AMBER));
                }
            });
        });
    }
}

pub fn run() -> eframe::Result<()> {
    let mut viewport = egui::ViewportBuilder::default().with_title(PRODUCT).with_inner_size([1060.0, 680.0]).with_min_inner_size([860.0, 560.0]);
    if let Ok(img) = image::load_from_memory_with_format(HUB_ICON, image::ImageFormat::Png) {
        let img = img.to_rgba8();
        viewport = viewport.with_icon(Arc::new(egui::IconData { width: img.width(), height: img.height(), rgba: img.into_raw() }));
    }
    let options = eframe::NativeOptions { viewport, ..Default::default() };
    eframe::run_native(PRODUCT, options, Box::new(|cc| Ok(Box::new(Hub::new(cc)))))
}
