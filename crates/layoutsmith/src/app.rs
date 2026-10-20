use std::path::{Path, PathBuf};
use std::rc::Rc;

use eframe::egui::{self, Key, KeyboardShortcut, Modifiers, RichText};
use layoutsmith_core::{NodeId, Resources, edit};

use crate::canvas::{self, CanvasState};
use crate::inspector::{self, InspectorState};
use crate::outline::{self, OutlineAction};
use crate::project::{EguiMeasurer, Project};

pub struct Device {
    pub name: &'static str,
    pub size: [f32; 2],
}

pub const DEVICES: [Device; 6] = [
    Device {
        name: "Phone",
        size: [360.0, 800.0],
    },
    Device {
        name: "Small phone",
        size: [320.0, 640.0],
    },
    Device {
        name: "Large phone",
        size: [411.0, 891.0],
    },
    Device {
        name: "Foldable",
        size: [673.0, 841.0],
    },
    Device {
        name: "Tablet",
        size: [800.0, 1280.0],
    },
    Device {
        name: "Desktop",
        size: [1280.0, 800.0],
    },
];

const NEW_LAYOUT: &str = include_str!("../assets/new_layout.xml");
const SAMPLE_LAYOUT: &str = include_str!("../../../samples/res/layout/activity_profile.xml");
const SAMPLE_VALUES: [&str; 4] = [
    include_str!("../../../samples/res/values/colors.xml"),
    include_str!("../../../samples/res/values/dimens.xml"),
    include_str!("../../../samples/res/values/strings.xml"),
    include_str!("../../../samples/res/values/themes.xml"),
];

/// Something that would throw away unsaved changes.
enum Pending {
    Open(PathBuf),
    New,
    Sample,
    Quit,
}

struct Message {
    text: String,
    error: bool,
    at: f64,
}

pub struct App {
    project: Option<Project>,
    measurer: Rc<EguiMeasurer>,
    selected: Option<NodeId>,
    hovered: Option<NodeId>,
    canvas: CanvasState,
    inspector: InspectorState,
    device: usize,
    landscape: bool,
    show_xml: bool,
    xml_buffer: String,
    xml_revision: Option<u64>,
    xml_edited: bool,
    xml_error: Option<String>,
    message: Option<Message>,
    pending: Option<Pending>,
    allow_close: bool,
    title: String,
    /// Opened on the first frame: text can't be measured before fonts load.
    startup_path: Option<PathBuf>,
    /// Whether the first-frame window sizing has been done.
    window_fitted: bool,
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>, path: Option<PathBuf>) -> Self {
        Self {
            project: None,
            measurer: Rc::new(EguiMeasurer {
                ctx: cc.egui_ctx.clone(),
            }),
            selected: None,
            hovered: None,
            canvas: CanvasState::default(),
            inspector: InspectorState::default(),
            device: 0,
            landscape: false,
            show_xml: false,
            xml_buffer: String::new(),
            xml_revision: None,
            xml_edited: false,
            xml_error: None,
            message: None,
            pending: None,
            allow_close: false,
            title: String::new(),
            startup_path: path,
            window_fitted: false,
        }
    }

    fn screen(&self) -> [f32; 2] {
        let [w, h] = DEVICES[self.device].size;
        if self.landscape { [h, w] } else { [w, h] }
    }

    fn say(&mut self, ctx: &egui::Context, text: impl Into<String>, error: bool) {
        self.message = Some(Message {
            text: text.into(),
            error,
            at: ctx.input(|i| i.time),
        });
    }

    fn set_project(&mut self, project: Project) {
        self.selected = project.doc.root();
        self.hovered = None;
        self.canvas.forget_drag();
        self.xml_revision = None;
        self.xml_edited = false;
        self.xml_error = None;
        self.project = Some(project);
    }

    fn open(&mut self, path: &Path, ctx: &egui::Context) {
        match Project::open(path, self.measurer.clone(), self.screen()) {
            Ok(project) => {
                let colors = project.res.colors.len();
                self.set_project(project);
                let detail = if colors > 0 {
                    format!(" ({colors} colors from res/values)")
                } else {
                    String::new()
                };
                self.say(ctx, format!("Opened {}{detail}", path.display()), false);
            }
            Err(e) => self.say(ctx, e, true),
        }
    }

    fn open_sample(&mut self, ctx: &egui::Context) {
        let mut project = Project::untitled(SAMPLE_LAYOUT, self.measurer.clone(), self.screen());
        let mut res = Resources::default();
        for values in SAMPLE_VALUES {
            res.add_values_xml(values);
        }
        project.res = res;
        project.relayout();
        self.set_project(project);
        self.say(
            ctx,
            "Sample layout opened. Save it anywhere to keep your changes.",
            false,
        );
    }

    fn is_dirty(&self) -> bool {
        self.project.as_ref().is_some_and(Project::is_dirty)
    }

    /// Runs `action` now, or asks first if it would lose unsaved changes.
    fn request(&mut self, action: Pending, ctx: &egui::Context) {
        if self.is_dirty() {
            self.pending = Some(action);
        } else {
            self.perform(action, ctx);
        }
    }

    fn perform(&mut self, action: Pending, ctx: &egui::Context) {
        match action {
            Pending::Open(path) => self.open(&path, ctx),
            Pending::New => {
                let project = Project::untitled(NEW_LAYOUT, self.measurer.clone(), self.screen());
                self.set_project(project);
            }
            Pending::Sample => self.open_sample(ctx),
            Pending::Quit => {
                self.allow_close = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }
    }

    /// File dialogs are owned by the main window, so it is disabled while
    /// they are open (on Windows an ownerless dialog leaves it clickable).
    fn open_dialog(&mut self, ctx: &egui::Context, frame: &eframe::Frame) {
        let mut dialog = rfd::FileDialog::new()
            .set_parent(frame)
            .add_filter("Android layout", &["xml"]);
        if let Some(dir) = self
            .project
            .as_ref()
            .and_then(|p| p.path.as_ref())
            .and_then(|p| p.parent())
        {
            dialog = dialog.set_directory(dir);
        }
        if let Some(path) = dialog.pick_file() {
            self.request(Pending::Open(path), ctx);
        }
    }

    /// Returns whether the file was saved.
    fn save(&mut self, ctx: &egui::Context, frame: &eframe::Frame, choose_path: bool) -> bool {
        let Some(project) = &mut self.project else {
            return false;
        };
        let path = match (&project.path, choose_path) {
            (Some(p), false) => p.clone(),
            _ => {
                let mut dialog = rfd::FileDialog::new()
                    .set_parent(frame)
                    .add_filter("Android layout", &["xml"])
                    .set_file_name(project.file_name());
                if let Some(dir) = project.path.as_ref().and_then(|p| p.parent()) {
                    dialog = dialog.set_directory(dir);
                }
                match dialog.save_file() {
                    Some(p) => p,
                    None => return false,
                }
            }
        };
        match project.save_as(&path) {
            Ok(()) => {
                self.say(ctx, format!("Saved {}", path.display()), false);
                true
            }
            Err(e) => {
                self.say(ctx, e, true);
                false
            }
        }
    }

    /// The default window is too tall for laptop screens at 125-150 %
    /// scaling (common on Windows), which would hide the status bar under the
    /// taskbar; maximize there instead. Waits until the monitor size is known.
    fn fit_window(&mut self, ctx: &egui::Context) {
        if self.window_fitted {
            return;
        }
        if let Some(monitor) = ctx.input(|i| i.viewport().monitor_size) {
            self.window_fitted = true;
            if monitor.x < 1500.0 || monitor.y < 1000.0 {
                ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(true));
            }
        }
    }

    /// Drops selection and hover that no longer point at a view (after undo,
    /// delete or a raw XML edit).
    fn fix_selection(&mut self) {
        let Some(project) = &self.project else {
            self.selected = None;
            self.hovered = None;
            return;
        };
        let alive = |id: &NodeId| project.doc.is_attached(*id) && project.doc.is_element(*id);
        if !self.selected.as_ref().is_some_and(alive) {
            self.selected = None;
        }
        if !self.hovered.as_ref().is_some_and(alive) {
            self.hovered = None;
        }
    }

    fn undo(&mut self) {
        if let Some(p) = &mut self.project
            && !self.canvas.is_dragging()
        {
            p.undo();
        }
        self.fix_selection();
    }

    fn redo(&mut self) {
        if let Some(p) = &mut self.project
            && !self.canvas.is_dragging()
        {
            p.redo();
        }
        self.fix_selection();
    }

    fn duplicate_selected(&mut self) {
        if let (Some(p), Some(id)) = (&mut self.project, self.selected) {
            outline::duplicate(p, &mut self.selected, id);
        }
    }

    fn delete_selected(&mut self) {
        if let (Some(p), Some(id)) = (&mut self.project, self.selected) {
            outline::delete(p, &mut self.selected, id);
        }
        self.fix_selection();
    }

    fn handle_shortcuts(&mut self, ctx: &egui::Context, frame: &eframe::Frame) {
        let cmd = |key| KeyboardShortcut::new(Modifiers::COMMAND, key);
        let cmd_shift = |key| KeyboardShortcut::new(Modifiers::COMMAND | Modifiers::SHIFT, key);
        let pressed = |s: KeyboardShortcut| ctx.input_mut(|i| i.consume_shortcut(&s));

        if pressed(cmd(Key::O)) {
            self.open_dialog(ctx, frame);
        }
        if pressed(cmd(Key::N)) {
            self.request(Pending::New, ctx);
        }
        if pressed(cmd_shift(Key::S)) {
            self.save(ctx, frame, true);
        } else if pressed(cmd(Key::S)) {
            self.save(ctx, frame, false);
        }
        if ctx.egui_wants_keyboard_input() {
            return; // Let text fields have their keys (and their own undo).
        }
        if pressed(cmd_shift(Key::Z)) || pressed(cmd(Key::Y)) {
            self.redo();
        } else if pressed(cmd(Key::Z)) {
            self.undo();
        }
        if pressed(cmd(Key::D)) {
            self.duplicate_selected();
        }
        let (delete, escape, shift) = ctx.input(|i| {
            (
                i.key_pressed(Key::Delete) || i.key_pressed(Key::Backspace),
                i.key_pressed(Key::Escape),
                i.modifiers.shift,
            )
        });
        if delete {
            self.delete_selected();
        }
        if escape {
            if let (true, Some(p)) = (self.canvas.is_dragging(), &mut self.project) {
                self.canvas.cancel_drag(p);
            } else if let (Some(p), Some(id)) = (&self.project, self.selected) {
                // Escape walks up to the parent, like in Android Studio.
                self.selected = p.doc.parent(id).or(Some(id));
            }
        }
        let step = if shift { 8.0 } else { 1.0 };
        let nudges = [
            (Key::ArrowLeft, [-step, 0.0]),
            (Key::ArrowRight, [step, 0.0]),
            (Key::ArrowUp, [0.0, -step]),
            (Key::ArrowDown, [0.0, step]),
        ];
        for (key, delta) in nudges {
            if ctx.input(|i| i.key_pressed(key))
                && let (Some(p), Some(id)) = (&mut self.project, self.selected)
                && p.doc.parent(id).is_some()
                && !self.canvas.is_dragging()
            {
                let merge = format!("nudge:{id}");
                p.edit("Nudge", Some(&merge), |doc, res, layout| {
                    edit::move_by(doc, res, layout, id, delta)
                });
            }
        }
    }

    fn handle_window_events(&mut self, ctx: &egui::Context) {
        let dropped: Vec<PathBuf> = ctx.input(|i| {
            i.raw
                .dropped_files
                .iter()
                .map(|f| f.path().to_path_buf())
                .filter(|p| !p.as_os_str().is_empty())
                .collect()
        });
        if let Some(path) = dropped.into_iter().next() {
            self.request(Pending::Open(path), ctx);
        }
        if ctx.input(|i| i.viewport().close_requested()) && !self.allow_close && self.is_dirty() {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.pending = Some(Pending::Quit);
        }
        let title = match &self.project {
            Some(p) => format!(
                "{}{} — layoutsmith",
                if p.is_dirty() { "• " } else { "" },
                p.file_name()
            ),
            None => "layoutsmith".to_string(),
        };
        if title != self.title {
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title.clone()));
            self.title = title;
        }
    }

    fn menu_bar(&mut self, ui: &mut egui::Ui, frame: &eframe::Frame) {
        let ctx = ui.ctx().clone();
        egui::MenuBar::new().ui(ui, |ui| {
            ui.menu_button("File", |ui| {
                if ui.button("New layout").clicked() {
                    self.request(Pending::New, &ctx);
                }
                if ui.button("Open…            Ctrl+O").clicked() {
                    ui.close();
                    self.open_dialog(&ctx, frame);
                }
                if ui.button("Open sample").clicked() {
                    self.request(Pending::Sample, &ctx);
                }
                ui.separator();
                let has_project = self.project.is_some();
                if ui
                    .add_enabled(has_project, egui::Button::new("Save              Ctrl+S"))
                    .clicked()
                {
                    ui.close();
                    self.save(&ctx, frame, false);
                }
                if ui
                    .add_enabled(has_project, egui::Button::new("Save as…  Ctrl+Shift+S"))
                    .clicked()
                {
                    ui.close();
                    self.save(&ctx, frame, true);
                }
                let has_path = self.project.as_ref().is_some_and(|p| p.path.is_some());
                if ui
                    .add_enabled(has_path, egui::Button::new("Reload resources"))
                    .clicked()
                    && let Some(p) = &mut self.project
                {
                    p.reload_resources();
                    let n = p.res.colors.len() + p.res.dimens.len() + p.res.strings.len();
                    self.say(&ctx, format!("Reloaded {n} values"), false);
                }
                ui.separator();
                if ui.button("Quit").clicked() {
                    self.request(Pending::Quit, &ctx);
                }
            });
            ui.menu_button("Edit", |ui| {
                let (undo, redo) = match &self.project {
                    Some(p) => (
                        p.history.undo_label().map(|l| format!("Undo {l}")),
                        p.history.redo_label().map(|l| format!("Redo {l}")),
                    ),
                    None => (None, None),
                };
                if ui
                    .add_enabled(
                        undo.is_some(),
                        egui::Button::new(undo.unwrap_or_else(|| "Undo".into()))
                            .shortcut_text("Ctrl+Z"),
                    )
                    .clicked()
                {
                    self.undo();
                }
                if ui
                    .add_enabled(
                        redo.is_some(),
                        egui::Button::new(redo.unwrap_or_else(|| "Redo".into()))
                            .shortcut_text("Ctrl+Shift+Z"),
                    )
                    .clicked()
                {
                    self.redo();
                }
                ui.separator();
                let can_edit = self
                    .selected
                    .zip(self.project.as_ref())
                    .is_some_and(|(s, p)| p.doc.parent(s).is_some());
                if ui
                    .add_enabled(
                        can_edit,
                        egui::Button::new("Duplicate").shortcut_text("Ctrl+D"),
                    )
                    .clicked()
                {
                    self.duplicate_selected();
                }
                if ui
                    .add_enabled(can_edit, egui::Button::new("Delete").shortcut_text("Del"))
                    .clicked()
                {
                    self.delete_selected();
                }
                if ui
                    .add_enabled(
                        can_edit,
                        egui::Button::new("Select parent").shortcut_text("Esc"),
                    )
                    .clicked()
                    && let (Some(p), Some(id)) = (&self.project, self.selected)
                {
                    self.selected = p.doc.parent(id);
                }
            });
            ui.menu_button("View", |ui| {
                for (i, d) in DEVICES.iter().enumerate() {
                    let label = format!("{}  ({}×{})", d.name, d.size[0], d.size[1]);
                    ui.radio_value(&mut self.device, i, label);
                }
                ui.checkbox(&mut self.landscape, "Landscape");
                ui.separator();
                ui.checkbox(&mut self.canvas.show_bounds, "Show view bounds");
                ui.checkbox(&mut self.canvas.snap, "Snap to 8dp grid");
                ui.checkbox(&mut self.show_xml, "Show XML");
                ui.separator();
                if ui.button("Zoom in").clicked() {
                    self.canvas.zoom = (self.canvas.zoom * 1.25).min(4.0);
                }
                if ui.button("Zoom out").clicked() {
                    self.canvas.zoom = (self.canvas.zoom / 1.25).max(0.25);
                }
                if ui.button("Actual size").clicked() {
                    self.canvas.zoom = 1.0;
                }
            });
            ui.menu_button("Help", |ui| {
                ui.label(RichText::new("Canvas").strong());
                ui.label("Click to select, drag to move, drag the handles to resize.");
                ui.label("Drop a view onto another container to move it inside.");
                ui.label("In a LinearLayout, dragging reorders the children.");
                ui.label("Arrow keys nudge by 1dp (Shift: 8dp). Esc selects the parent.");
                ui.label("Ctrl + mouse wheel zooms.");
            });
        });
    }

    fn status_bar(&mut self, ui: &mut egui::Ui) {
        let now = ui.input(|i| i.time);
        ui.horizontal(|ui| {
            if let Some(p) = &self.project {
                let path = p
                    .path
                    .as_ref()
                    .map_or_else(|| "untitled".to_string(), |p| p.display().to_string());
                ui.label(RichText::new(path).small());
                if p.is_dirty() {
                    ui.label(
                        RichText::new("• modified")
                            .small()
                            .color(egui::Color32::from_rgb(0xF5, 0x7C, 0x00)),
                    );
                }
            }
            if let Some(m) = &self.message
                && now - m.at < 6.0
            {
                let color = if m.error {
                    ui.visuals().error_fg_color
                } else {
                    ui.visuals().weak_text_color()
                };
                ui.separator();
                ui.label(RichText::new(&m.text).small().color(color));
                ui.ctx()
                    .request_repaint_after(std::time::Duration::from_secs(1));
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.small_button("+").clicked() {
                    self.canvas.zoom = (self.canvas.zoom * 1.25).min(4.0);
                }
                if ui
                    .small_button(format!("{:.0}%", self.canvas.zoom * 100.0))
                    .on_hover_text("Actual size")
                    .clicked()
                {
                    self.canvas.zoom = 1.0;
                }
                if ui.small_button("−").clicked() {
                    self.canvas.zoom = (self.canvas.zoom / 1.25).max(0.25);
                }
                ui.separator();
                ui.toggle_value(&mut self.show_xml, "XML");
                ui.toggle_value(&mut self.canvas.snap, "Snap 8dp");
                ui.separator();
                egui::ComboBox::from_id_salt("device")
                    .selected_text(DEVICES[self.device].name)
                    .show_ui(ui, |ui| {
                        for (i, d) in DEVICES.iter().enumerate() {
                            ui.selectable_value(&mut self.device, i, d.name);
                        }
                    });
                if ui
                    .selectable_label(self.landscape, "⟲")
                    .on_hover_text("Landscape")
                    .clicked()
                {
                    self.landscape = !self.landscape;
                }
            });
        });
    }

    fn xml_panel(&mut self, ui: &mut egui::Ui) {
        let Some(project) = &mut self.project else {
            return;
        };
        // The editor works with "\n" (egui inserts it for Enter and strips
        // "\r" from pastes), so CRLF files are edited as LF and converted back.
        let crlf = project.doc.newline() == "\r\n";
        if !self.xml_edited && self.xml_revision != Some(project.revision) {
            let xml = project.doc.to_xml();
            self.xml_buffer = if crlf { xml.replace("\r\n", "\n") } else { xml };
            self.xml_revision = Some(project.revision);
        }
        ui.horizontal(|ui| {
            ui.strong("XML");
            if ui
                .add_enabled(self.xml_edited, egui::Button::new("Apply"))
                .clicked()
            {
                let source = if crlf {
                    self.xml_buffer.replace("\r\n", "\n").replace('\n', "\r\n")
                } else {
                    self.xml_buffer.clone()
                };
                match project.replace_source(&source) {
                    Ok(()) => {
                        self.xml_edited = false;
                        self.xml_error = None;
                        self.selected = project.doc.root();
                    }
                    Err(e) => self.xml_error = Some(e),
                }
            }
            if ui
                .add_enabled(self.xml_edited, egui::Button::new("Revert"))
                .clicked()
            {
                self.xml_edited = false;
                self.xml_error = None;
                self.xml_revision = None;
            }
            if let Some(e) = &self.xml_error {
                ui.colored_label(ui.visuals().error_fg_color, e);
            } else if !self.xml_edited {
                ui.label(
                    RichText::new("Edit the source directly, then Apply.")
                        .small()
                        .weak(),
                );
            }
        });
        egui::ScrollArea::vertical()
            .id_salt("xml-scroll")
            .show(ui, |ui| {
                let r = ui.add(
                    egui::TextEdit::multiline(&mut self.xml_buffer)
                        .code_editor()
                        .desired_width(f32::INFINITY)
                        .desired_rows(12),
                );
                if r.changed() {
                    self.xml_edited = true;
                }
            });
    }

    fn welcome(&mut self, ui: &mut egui::Ui, frame: &eframe::Frame) {
        let ctx = ui.ctx().clone();
        ui.vertical_centered(|ui| {
            ui.add_space(ui.available_height() * 0.25);
            ui.heading(RichText::new("layoutsmith").size(34.0));
            ui.label("Open an Android layout XML file to start designing.");
            ui.add_space(16.0);
            if ui
                .button(RichText::new("Open layout…").size(16.0))
                .clicked()
            {
                self.open_dialog(&ctx, frame);
            }
            ui.add_space(6.0);
            if ui.button("New layout").clicked() {
                self.request(Pending::New, &ctx);
            }
            ui.add_space(6.0);
            if ui.button("Try the sample").clicked() {
                self.request(Pending::Sample, &ctx);
            }
            ui.add_space(16.0);
            ui.label(RichText::new("…or drop a file onto this window.").weak());
        });
    }

    fn pending_dialog(&mut self, ctx: &egui::Context, frame: &eframe::Frame) {
        if self.pending.is_none() {
            return;
        }
        let name = self
            .project
            .as_ref()
            .map(Project::file_name)
            .unwrap_or_default();
        let mut choice = None;
        egui::Modal::new(egui::Id::new("unsaved")).show(ctx, |ui| {
            ui.set_max_width(340.0);
            ui.heading("Unsaved changes");
            ui.label(format!("Save the changes to {name} first?"));
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if ui.button("Save").clicked() {
                    choice = Some(0);
                }
                if ui.button("Don't save").clicked() {
                    choice = Some(1);
                }
                if ui.button("Cancel").clicked() {
                    choice = Some(2);
                }
            });
        });
        match choice {
            Some(0) => {
                if self.save(ctx, frame, false)
                    && let Some(action) = self.pending.take()
                {
                    self.perform(action, ctx);
                }
            }
            Some(1) => {
                if let Some(action) = self.pending.take() {
                    self.perform(action, ctx);
                }
            }
            Some(_) => self.pending = None,
            None => {}
        }
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        let frame: &eframe::Frame = frame;
        let ctx = ui.ctx().clone();
        self.fit_window(&ctx);
        if let Some(path) = self.startup_path.take() {
            self.open(&path, &ctx);
        }
        self.handle_window_events(&ctx);
        self.handle_shortcuts(&ctx, frame);
        self.fix_selection();
        let screen = self.screen();
        if let Some(p) = &mut self.project {
            p.set_screen(screen);
        }

        egui::Panel::top("menu").show(ui, |ui| self.menu_bar(ui, frame));
        egui::Panel::bottom("status").show(ui, |ui| self.status_bar(ui));

        if self.project.is_some() {
            egui::Panel::left("outline")
                .default_size(270.0)
                .show(ui, |ui| {
                    let Some(project) = &mut self.project else {
                        return;
                    };
                    let action = outline::show(ui, project, &mut self.selected, &mut self.hovered);
                    if let Some(OutlineAction::Open(path)) = action {
                        self.request(Pending::Open(path), &ctx);
                    }
                });
            self.fix_selection();
            egui::Panel::right("inspector")
                .default_size(340.0)
                .show(ui, |ui| {
                    let Some(project) = &mut self.project else {
                        return;
                    };
                    match self.selected {
                        Some(id) => inspector::show(ui, project, id, &mut self.inspector),
                        None => {
                            ui.add_space(12.0);
                            ui.label(
                                RichText::new("Select a view on the canvas or in the tree.").weak(),
                            );
                        }
                    }
                });
            if self.show_xml {
                egui::Panel::bottom("xml")
                    .resizable(true)
                    .default_size(240.0)
                    .show(ui, |ui| self.xml_panel(ui));
            }
            self.fix_selection();
        }

        egui::CentralPanel::default().show(ui, |ui| match &mut self.project {
            Some(project) => {
                canvas::show(
                    ui,
                    project,
                    &mut self.canvas,
                    &mut self.selected,
                    &mut self.hovered,
                );
            }
            None => self.welcome(ui, frame),
        });

        self.pending_dialog(&ctx, frame);
    }
}
