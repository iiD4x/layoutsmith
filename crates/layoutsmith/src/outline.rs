//! The left panel: the view tree, the palette and the other layouts in the
//! same folder.

use std::path::PathBuf;

use eframe::egui::{self, Button, RichText};
use egui::collapsing_header::CollapsingState;
use layoutsmith_core::edit::{self, NewView};
use layoutsmith_core::layout::text_style;
use layoutsmith_core::widgets::{ContainerKind, classify, short_name};
use layoutsmith_core::xml::id_name;
use layoutsmith_core::{Document, NodeId};

use crate::project::Project;

pub enum OutlineAction {
    Open(PathBuf),
}

enum TreeAction {
    Select(NodeId),
    Hover(NodeId),
    Add(NodeId, NewView),
    Duplicate(NodeId),
    Delete(NodeId),
    Shift(NodeId, isize),
}

pub fn show(
    ui: &mut egui::Ui,
    project: &mut Project,
    selected: &mut Option<NodeId>,
    hovered: &mut Option<NodeId>,
) -> Option<OutlineAction> {
    let mut actions = Vec::new();
    let sel = *selected;
    let movable = sel.filter(|&s| project.doc.parent(s).is_some());

    ui.horizontal(|ui| {
        ui.menu_button("➕ Add view", |ui| {
            palette(ui, sel.or(project.doc.root()), &mut actions);
        });
        ui.add_space(4.0);
        if ui
            .add_enabled(movable.is_some(), Button::new("⬆"))
            .on_hover_text("Move up")
            .clicked()
        {
            actions.push(TreeAction::Shift(movable.unwrap(), -1));
        }
        if ui
            .add_enabled(movable.is_some(), Button::new("⬇"))
            .on_hover_text("Move down")
            .clicked()
        {
            actions.push(TreeAction::Shift(movable.unwrap(), 1));
        }
        if ui
            .add_enabled(movable.is_some(), Button::new("📋"))
            .on_hover_text("Duplicate (Ctrl+D)")
            .clicked()
        {
            actions.push(TreeAction::Duplicate(movable.unwrap()));
        }
        if ui
            .add_enabled(movable.is_some(), Button::new("🗑"))
            .on_hover_text("Delete (Del)")
            .clicked()
        {
            actions.push(TreeAction::Delete(movable.unwrap()));
        }
    });
    ui.separator();

    let mut open = None;
    let folder = layouts_in_folder(project);
    let tree_height = if folder.len() > 1 {
        ui.available_height() * 0.68
    } else {
        ui.available_height()
    };
    egui::ScrollArea::vertical()
        .id_salt("outline-tree")
        .max_height(tree_height)
        .auto_shrink([false, true])
        .show(ui, |ui| {
            if let Some(root) = project.doc.root() {
                node_ui(ui, project, root, sel, &mut actions);
            }
        });

    if folder.len() > 1 {
        ui.separator();
        ui.label(RichText::new("Layouts in this folder").strong());
        egui::ScrollArea::vertical()
            .id_salt("outline-files")
            .show(ui, |ui| {
                for path in folder {
                    let name = path
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .into_owned();
                    let current = project.path.as_deref() == Some(path.as_path());
                    if ui.selectable_label(current, name).clicked() && !current {
                        open = Some(OutlineAction::Open(path));
                    }
                }
            });
    }

    let mut hover = None;
    for action in actions {
        match action {
            TreeAction::Select(id) => *selected = Some(id),
            TreeAction::Hover(id) => hover = Some(id),
            TreeAction::Add(target, kind) => add_view(project, selected, target, kind),
            TreeAction::Duplicate(id) => duplicate(project, selected, id),
            TreeAction::Delete(id) => delete(project, selected, id),
            TreeAction::Shift(id, by) => shift(project, id, by),
        }
    }
    if hover.is_some() {
        *hovered = hover;
    }
    open
}

fn palette(ui: &mut egui::Ui, target: Option<NodeId>, actions: &mut Vec<TreeAction>) {
    let Some(target) = target else { return };
    for kind in NewView::ALL {
        if ui.button(kind.label()).clicked() {
            actions.push(TreeAction::Add(target, kind));
            ui.close();
        }
    }
}

fn node_label(doc: &Document, id: NodeId, project_text: Option<String>) -> String {
    let mut label = short_name(doc.name(id)).to_string();
    if let Some(name) = doc.attr(id, "android:id").as_deref().and_then(id_name) {
        label.push_str("  #");
        label.push_str(name);
    }
    if let Some(text) = project_text.filter(|t| !t.is_empty()) {
        let snippet: String = text.chars().take(18).collect();
        let ellipsis = if text.chars().count() > 18 { "…" } else { "" };
        label.push_str(&format!("  “{}{ellipsis}”", snippet.replace('\n', " ")));
    }
    label
}

fn node_ui(
    ui: &mut egui::Ui,
    project: &Project,
    id: NodeId,
    selected: Option<NodeId>,
    actions: &mut Vec<TreeAction>,
) {
    let doc = &project.doc;
    let kids = doc.element_children(id);
    if kids.is_empty() {
        ui.horizontal(|ui| {
            ui.add_space(ui.spacing().indent);
            header(ui, project, id, selected, actions);
        });
        return;
    }
    let state_id = ui.make_persistent_id(("outline-node", id));
    let mut state = CollapsingState::load_with_default_open(ui.ctx(), state_id, true);
    if selected.is_some_and(|s| s != id && doc.is_ancestor_or_self(id, s)) {
        state.set_open(true);
    }
    state
        .show_header(ui, |ui| header(ui, project, id, selected, actions))
        .body(|ui| {
            for c in kids {
                node_ui(ui, project, c, selected, actions);
            }
        });
}

fn header(
    ui: &mut egui::Ui,
    project: &Project,
    id: NodeId,
    selected: Option<NodeId>,
    actions: &mut Vec<TreeAction>,
) {
    let doc = &project.doc;
    let kind = classify(doc, id);
    let text = kind
        .shows_text()
        .then(|| text_style(doc, &project.res, id, 1.0).text);
    let mut label = RichText::new(node_label(doc, id, text));
    if kind.container().is_some() {
        label = label.strong();
    }
    let response = ui.selectable_label(selected == Some(id), label);
    if response.clicked() {
        actions.push(TreeAction::Select(id));
    }
    if response.hovered() {
        actions.push(TreeAction::Hover(id));
    }
    let is_root = doc.parent(id).is_none();
    response.context_menu(|ui| {
        ui.menu_button("Add view", |ui| palette(ui, Some(id), actions));
        if !is_root {
            if ui.button("Move up").clicked() {
                actions.push(TreeAction::Shift(id, -1));
                ui.close();
            }
            if ui.button("Move down").clicked() {
                actions.push(TreeAction::Shift(id, 1));
                ui.close();
            }
            if ui.button("Duplicate").clicked() {
                actions.push(TreeAction::Duplicate(id));
                ui.close();
            }
            if ui.button("Delete").clicked() {
                actions.push(TreeAction::Delete(id));
                ui.close();
            }
        }
    });
}

/// Adds a view into `target` if it is a container, otherwise after it.
pub fn add_view(
    project: &mut Project,
    selected: &mut Option<NodeId>,
    target: NodeId,
    kind: NewView,
) {
    let doc = &project.doc;
    let (parent, index) = match classify(doc, target).container() {
        // A ScrollView holds a single child; add into that child instead.
        Some(ContainerKind::Scroll { .. }) => match doc.element_children(target).first() {
            Some(&child) if classify(doc, child).container().is_some() => (child, usize::MAX),
            Some(_) => return,
            None => (target, usize::MAX),
        },
        Some(_) => (target, usize::MAX),
        None => {
            let Some(parent) = doc.parent(target) else {
                return;
            };
            let index = doc
                .element_children(parent)
                .iter()
                .position(|&c| c == target)
                .unwrap_or(0)
                + 1;
            (parent, index)
        }
    };
    let mut created = None;
    project.edit(&format!("Add {}", kind.label()), None, |doc, _, _| {
        created = Some(edit::insert_view(doc, parent, index, kind));
    });
    if created.is_some() {
        *selected = created;
    }
}

pub fn duplicate(project: &mut Project, selected: &mut Option<NodeId>, id: NodeId) {
    let mut copy = None;
    project.edit("Duplicate", None, |doc, _, _| {
        copy = edit::duplicate(doc, id)
    });
    if copy.is_some() {
        *selected = copy;
    }
}

pub fn delete(project: &mut Project, selected: &mut Option<NodeId>, id: NodeId) {
    let parent = project.doc.parent(id);
    project.edit("Delete", None, |doc, _, _| {
        edit::delete(doc, id);
    });
    if *selected == Some(id) {
        *selected = parent;
    }
}

fn shift(project: &mut Project, id: NodeId, by: isize) {
    let Some(parent) = project.doc.parent(id) else {
        return;
    };
    let siblings = project.doc.element_children(parent);
    let Some(i) = siblings.iter().position(|&c| c == id) else {
        return;
    };
    let to = i as isize + by;
    if to < 0 || to >= siblings.len() as isize {
        return;
    }
    project.edit(
        if by < 0 { "Move up" } else { "Move down" },
        None,
        |doc, _, _| {
            edit::reorder(doc, id, to as usize);
        },
    );
}

fn layouts_in_folder(project: &Project) -> Vec<PathBuf> {
    let Some(dir) = project.path.as_ref().and_then(|p| p.parent()) else {
        return Vec::new();
    };
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut files: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "xml"))
        .collect();
    files.sort();
    files
}
