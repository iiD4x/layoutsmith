//! The right panel: properties of the selected view, grouped by what a
//! designer wants to change (size, position, spacing, text, appearance), plus
//! a raw list of every attribute.

use eframe::egui::{self, Color32, ComboBox, DragValue, Layout, RichText, TextEdit, Ui, vec2};
use egui::color_picker::Alpha;
use layoutsmith_core::edit::{self, Spacing};
use layoutsmith_core::layout::{
    Attrs, Target, bias_attr, constraint_rules, params, relative_rules, text_style,
};
use layoutsmith_core::units::{Gravity, format_dp, format_number, format_sp};
use layoutsmith_core::widgets::{ContainerKind, ViewKind, classify, short_name};
use layoutsmith_core::xml::{APP_NS, TOOLS_NS, id_name};
use layoutsmith_core::{Align, Color, Dim, Document, NodeId, Resources};

use crate::canvas::to_color32;
use crate::project::Project;

const LABEL_WIDTH: f32 = 92.0;

#[derive(Default)]
pub struct InspectorState {
    new_name: String,
    new_value: String,
}

const COMMON_TYPES: [&str; 14] = [
    "TextView",
    "Button",
    "EditText",
    "ImageView",
    "ImageButton",
    "CheckBox",
    "Switch",
    "View",
    "LinearLayout",
    "FrameLayout",
    "RelativeLayout",
    "androidx.constraintlayout.widget.ConstraintLayout",
    "androidx.cardview.widget.CardView",
    "com.google.android.material.button.MaterialButton",
];

pub fn show(ui: &mut Ui, project: &mut Project, id: NodeId, state: &mut InspectorState) {
    egui::ScrollArea::vertical()
        .id_salt("inspector")
        .auto_shrink(false)
        .show(ui, |ui| {
            identity(ui, project, id);
            ui.separator();
            size_section(ui, project, id);
            position_section(ui, project, id);
            spacing_section(ui, project, id);
            if classify(&project.doc, id).shows_text() {
                text_section(ui, project, id);
            }
            appearance_section(ui, project, id);
            if classify(&project.doc, id).container().is_some() {
                container_section(ui, project, id);
            }
            all_attributes(ui, project, id, state);
        });
}

// ---- helpers ---------------------------------------------------------------

fn merge_key(id: NodeId, what: &str) -> String {
    format!("{id}:{what}")
}

fn pretty(attr: &str) -> &str {
    attr.split_once(':').map_or(attr, |(_, n)| n)
}

/// Sets (or with `None`, removes) an attribute as an undoable edit.
fn set_attr(project: &mut Project, id: NodeId, attr: &str, value: Option<String>) {
    let label = format!("Edit {}", pretty(attr));
    project.edit(&label, Some(&merge_key(id, attr)), |doc, _, _| {
        if let Some(ns) = attr.split_once(':').map(|(p, _)| p) {
            match ns {
                "app" => doc.ensure_namespace("app", APP_NS),
                "tools" => doc.ensure_namespace("tools", TOOLS_NS),
                _ => {}
            }
        }
        match value {
            Some(v) => doc.set_attr(id, attr, &v),
            None => {
                doc.remove_attr(id, attr);
            }
        }
    });
}

fn row(ui: &mut Ui, label: &str, add: impl FnOnce(&mut Ui)) {
    ui.horizontal(|ui| {
        ui.allocate_ui_with_layout(
            vec2(LABEL_WIDTH, 20.0),
            Layout::left_to_right(egui::Align::Center),
            |ui| {
                ui.set_min_width(LABEL_WIDTH);
                ui.label(label);
            },
        );
        add(ui);
    });
}

fn section(ui: &mut Ui, title: &str, add: impl FnOnce(&mut Ui)) {
    egui::CollapsingHeader::new(RichText::new(title).strong())
        .default_open(true)
        .show(ui, add);
}

/// A text field bound to an attribute. Clearing it removes the attribute once
/// the field loses focus (so typing over a value does not reorder attributes).
fn text_attr(
    ui: &mut Ui,
    project: &mut Project,
    id: NodeId,
    attr: &str,
    hint: &str,
    multiline: bool,
) {
    let current = project.doc.attr(id, attr);
    let mut value = current.clone().unwrap_or_default();
    let field = if multiline {
        TextEdit::multiline(&mut value).desired_rows(2)
    } else {
        TextEdit::singleline(&mut value)
    };
    let r = ui.add(field.hint_text(hint).desired_width(ui.available_width()));
    if r.changed() {
        set_attr(project, id, attr, Some(value.clone()));
    }
    if r.lost_focus() && value.is_empty() && current.is_some() {
        set_attr(project, id, attr, None);
    }
}

fn dp_drag(ui: &mut Ui, value: &mut f32, suffix: &str) -> egui::Response {
    ui.add(
        DragValue::new(value)
            .speed(0.5)
            .range(0.0..=4000.0)
            .max_decimals(1)
            .suffix(suffix),
    )
}

/// A color swatch with a picker, the raw value, and the project's colors.
fn color_attr(ui: &mut Ui, project: &mut Project, id: NodeId, attr: &str) {
    let raw = project.doc.attr(id, attr);
    let resolved = raw.as_deref().and_then(|r| project.res.resolve_color(r));
    ui.horizontal(|ui| {
        let mut c = resolved.map_or(Color32::TRANSPARENT, |c| to_color32(c, 1.0));
        if egui::color_picker::color_edit_button_srgba(ui, &mut c, Alpha::OnlyBlend).changed() {
            let [r, g, b, a] = c.to_srgba_unmultiplied();
            set_attr(project, id, attr, Some(Color::argb(a, r, g, b).to_hex()));
        }
        let mut text = raw.clone().unwrap_or_default();
        let field = ui.add(
            TextEdit::singleline(&mut text)
                .desired_width(118.0)
                .hint_text("none"),
        );
        if field.changed() {
            set_attr(project, id, attr, Some(text.clone()));
        }
        if field.lost_focus() && text.is_empty() && raw.is_some() {
            set_attr(project, id, attr, None);
        }
        let names: Vec<String> = project.res.color_names().map(str::to_string).collect();
        if !names.is_empty() {
            ui.menu_button("⏷", |ui| {
                ui.set_min_width(180.0);
                for name in names {
                    let swatch = project.res.resolve_color(&format!("@color/{name}"));
                    let clicked = ui
                        .horizontal(|ui| {
                            let (rect, _) =
                                ui.allocate_exact_size(vec2(14.0, 14.0), egui::Sense::hover());
                            if let Some(s) = swatch {
                                ui.painter().rect_filled(rect, 2.0, to_color32(s, 1.0));
                            }
                            ui.button(format!("@color/{name}")).clicked()
                        })
                        .inner;
                    if clicked {
                        set_attr(project, id, attr, Some(format!("@color/{name}")));
                        ui.close();
                    }
                }
            })
            .response
            .on_hover_text("Project colors");
        }
        if raw.is_some() && ui.small_button("🗙").on_hover_text("Remove").clicked() {
            set_attr(project, id, attr, None);
        }
    });
}

// ---- sections --------------------------------------------------------------

fn identity(ui: &mut Ui, project: &mut Project, id: NodeId) {
    let doc = &project.doc;
    let name = doc.name(id).to_string();
    ui.heading(short_name(&name));
    if let Some(r) = project.layout.rect(id) {
        ui.label(
            RichText::new(format!(
                "{} × {} dp at ({}, {})",
                format_number(r.w),
                format_number(r.h),
                format_number(r.x),
                format_number(r.y)
            ))
            .weak(),
        );
    }
    row(ui, "Type", |ui| {
        let mut tag = name.clone();
        let r = ui.add(TextEdit::singleline(&mut tag).desired_width(ui.available_width() - 30.0));
        if r.changed() && !tag.trim().is_empty() && !tag.contains(char::is_whitespace) {
            project.edit("Change type", Some(&merge_key(id, "type")), |doc, _, _| {
                doc.set_name(id, &tag)
            });
        }
        ui.menu_button("⏷", |ui| {
            for t in COMMON_TYPES {
                if ui.button(short_name(t)).on_hover_text(t).clicked() {
                    project.edit("Change type", None, |doc, _, _| doc.set_name(id, t));
                    ui.close();
                }
            }
        });
    });
    row(ui, "ID", |ui| {
        let current = project.doc.attr(id, "android:id");
        let mut name = current
            .as_deref()
            .and_then(id_name)
            .unwrap_or_default()
            .to_string();
        let r = ui.add(
            TextEdit::singleline(&mut name)
                .hint_text("no id")
                .desired_width(ui.available_width()),
        );
        if r.changed() && !name.is_empty() {
            set_attr(
                project,
                id,
                "android:id",
                Some(format!("@+id/{}", name.trim())),
            );
        }
        if r.lost_focus() && name.is_empty() && current.is_some() {
            set_attr(project, id, "android:id", None);
        }
    });
    if let Some(style) = project.doc.attr(id, "style") {
        row(ui, "Style", |ui| {
            ui.label(RichText::new(style).monospace());
        });
    }
}

fn size_section(ui: &mut Ui, project: &mut Project, id: NodeId) {
    section(ui, "Size", |ui| {
        let in_constraint = edit::parent_kind(&project.doc, id) == Some(ContainerKind::Constraint);
        for axis in 0..2 {
            let p = params(&project.doc, &project.res, id, project.layout.density);
            let dim = p.size[axis];
            let rendered = project.layout.rect(id).map_or(0.0, |r| r.len(axis));
            row(ui, if axis == 0 { "Width" } else { "Height" }, |ui| {
                // 0 = match parent, 1 = wrap content, 2 = fixed, 3 = match constraint
                let mut mode = match dim {
                    Dim::Match => 0,
                    Dim::Wrap => 1,
                    Dim::Dp(v) if v == 0.0 && in_constraint => 3,
                    Dim::Dp(_) => 2,
                };
                let before = mode;
                let names = ["match_parent", "wrap_content", "fixed", "match constraints"];
                ComboBox::from_id_salt(("size-mode", id, axis))
                    .width(120.0)
                    .selected_text(names[mode])
                    .show_ui(ui, |ui| {
                        for (i, n) in names.iter().enumerate() {
                            if i == 3 && !in_constraint {
                                continue;
                            }
                            ui.selectable_value(&mut mode, i, *n);
                        }
                    });
                if mode != before {
                    let new = match mode {
                        0 => Dim::Match,
                        1 => Dim::Wrap,
                        3 => Dim::Dp(0.0),
                        _ => Dim::Dp(rendered.round().max(1.0)),
                    };
                    project.edit("Change size", None, |doc, _, _| {
                        edit::set_size(doc, id, axis, new)
                    });
                }
                if let Dim::Dp(mut v) = dim
                    && mode == 2
                    && dp_drag(ui, &mut v, " dp").changed()
                {
                    project.edit(
                        "Change size",
                        Some(&merge_key(id, edit::size_attr(axis))),
                        |doc, _, _| edit::set_size(doc, id, axis, Dim::Dp(v.round())),
                    );
                }
            });
        }
        if let Some(ContainerKind::Linear { .. }) = edit::parent_kind(&project.doc, id) {
            let p = params(&project.doc, &project.res, id, project.layout.density);
            row(ui, "Weight", |ui| {
                let mut w = p.weight;
                if ui
                    .add(
                        DragValue::new(&mut w)
                            .speed(0.05)
                            .range(0.0..=100.0)
                            .max_decimals(2),
                    )
                    .changed()
                {
                    let value = (w > 0.0).then(|| format_number(w));
                    set_attr(project, id, "android:layout_weight", value);
                }
            });
        }
    });
}

/// Which alignment button is "on" for a view along one axis.
fn current_alignment(
    doc: &Document,
    res: &Resources,
    id: NodeId,
    axis: usize,
    density: f32,
) -> Option<Align> {
    let parent = doc.parent(id)?;
    let p = params(doc, res, id, density);
    match classify(doc, parent).container()? {
        ContainerKind::Constraint => {
            let rules: Vec<_> = constraint_rules(doc, id)
                .into_iter()
                .filter(|r| r.axis == axis)
                .collect();
            let lo = rules
                .iter()
                .any(|r| !r.hi_side && r.target == Target::Parent);
            let hi = rules
                .iter()
                .any(|r| r.hi_side && r.target == Target::Parent);
            let bias = doc
                .attr(id, bias_attr(axis))
                .and_then(|b| b.parse::<f32>().ok())
                .unwrap_or(0.5);
            match (lo, hi) {
                (true, true) if p.size[axis].is_zero() => Some(Align::Fill),
                (true, true) if (bias - 0.5).abs() < 0.01 => Some(Align::Center),
                (true, false) if rules.len() == 1 => Some(Align::Start),
                (false, true) if rules.len() == 1 => Some(Align::End),
                _ => None,
            }
        }
        ContainerKind::Relative => {
            if p.size[axis] == Dim::Match {
                return Some(Align::Fill);
            }
            let rules: Vec<_> = relative_rules(doc, id)
                .into_iter()
                .filter(|r| r.axis == axis)
                .collect();
            if rules.iter().any(|r| r.target == Target::Center) {
                Some(Align::Center)
            } else if rules
                .iter()
                .any(|r| r.target == Target::Parent && r.hi_side)
            {
                Some(Align::End)
            } else if rules.iter().all(|r| r.target == Target::Parent) {
                Some(Align::Start)
            } else {
                None
            }
        }
        ContainerKind::Linear { vertical } if axis == vertical as usize => {
            if p.weight > 0.0 && p.size[axis].is_zero() {
                return Some(Align::Fill);
            }
            let g = params(doc, res, parent, density).gravity;
            Some(g.axis(axis).unwrap_or(Align::Start))
        }
        _ => {
            if p.size[axis] == Dim::Match {
                Some(Align::Fill)
            } else {
                Some(p.layout_gravity.axis(axis).unwrap_or(Align::Start))
            }
        }
    }
}

fn position_section(ui: &mut Ui, project: &mut Project, id: NodeId) {
    let Some(parent) = project.doc.parent(id) else {
        return;
    };
    let parent_kind = classify(&project.doc, parent).container();
    section(ui, "Position", |ui| {
        let how = match parent_kind {
            Some(ContainerKind::Constraint) => "Aligning sets constraints to the parent.",
            Some(ContainerKind::Relative) => "Aligning sets alignParent / center rules.",
            Some(ContainerKind::Linear { .. }) => {
                "Across the list this sets layout_gravity; along it, the parent's gravity (Fill = take the remaining space)."
            }
            _ => "Aligning sets layout_gravity.",
        };
        ui.label(RichText::new(how).small().weak());
        for axis in 0..2 {
            let current =
                current_alignment(&project.doc, &project.res, id, axis, project.layout.density);
            let names = if axis == 0 {
                ["Start", "Center", "End", "Fill"]
            } else {
                ["Top", "Middle", "Bottom", "Fill"]
            };
            row(
                ui,
                if axis == 0 { "Horizontal" } else { "Vertical" },
                |ui| {
                    for (name, align) in names.into_iter().zip([
                        Align::Start,
                        Align::Center,
                        Align::End,
                        Align::Fill,
                    ]) {
                        if ui.selectable_label(current == Some(align), name).clicked() {
                            project.edit(&format!("Align {name}"), None, |doc, res, layout| {
                                edit::align(doc, res, layout, id, axis, align)
                            });
                        }
                    }
                },
            );
        }

        let rules = match parent_kind {
            Some(ContainerKind::Constraint) => constraint_rules(&project.doc, id),
            Some(ContainerKind::Relative) => relative_rules(&project.doc, id),
            _ => Vec::new(),
        };
        if !rules.is_empty() {
            ui.add_space(4.0);
            ui.label(
                RichText::new(if parent_kind == Some(ContainerKind::Constraint) {
                    "Constraints"
                } else {
                    "Rules"
                })
                .small(),
            );
            for rule in rules {
                ui.horizontal(|ui| {
                    if ui.small_button("🗙").on_hover_text("Remove").clicked() {
                        set_attr(project, id, &rule.attr, None);
                    }
                    let value = project.doc.attr(id, &rule.attr).unwrap_or_default();
                    ui.label(
                        RichText::new(format!(
                            "{} = {value}",
                            pretty(&rule.attr).trim_start_matches("layout_")
                        ))
                        .monospace()
                        .small(),
                    );
                });
            }
        }
        if parent_kind == Some(ContainerKind::Constraint) {
            let anchors = project.layout.anchors.get(&id).copied().unwrap_or_default();
            for axis in 0..2 {
                if anchors.lo[axis].is_none() || anchors.hi[axis].is_none() {
                    continue;
                }
                let attr = bias_attr(axis);
                let mut bias = project
                    .doc
                    .attr(id, attr)
                    .and_then(|b| b.parse::<f32>().ok())
                    .unwrap_or(0.5);
                row(ui, if axis == 0 { "H bias" } else { "V bias" }, |ui| {
                    if ui
                        .add(egui::Slider::new(&mut bias, 0.0..=1.0).step_by(0.01))
                        .changed()
                    {
                        let value = ((bias - 0.5).abs() >= 0.005).then(|| format_number(bias));
                        set_attr(project, id, attr, value);
                    }
                });
            }
        }
    });
}

fn spacing_section(ui: &mut Ui, project: &mut Project, id: NodeId) {
    section(ui, "Spacing", |ui| {
        for (kind, title) in [(Spacing::Margin, "Margin"), (Spacing::Padding, "Padding")] {
            if kind == Spacing::Margin && project.doc.parent(id).is_none() {
                continue;
            }
            let p = params(&project.doc, &project.res, id, project.layout.density);
            let (lo, hi) = match kind {
                Spacing::Margin => (p.margin_lo, p.margin_hi),
                Spacing::Padding => (p.padding_lo, p.padding_hi),
            };
            row(ui, title, |ui| {
                ui.spacing_mut().item_spacing.x = 3.0;
                let sides = [
                    ("S", 0, false, lo[0]),
                    ("T", 1, false, lo[1]),
                    ("E", 0, true, hi[0]),
                    ("B", 1, true, hi[1]),
                ];
                for (letter, axis, is_hi, value) in sides {
                    let mut v = value;
                    let side = match letter {
                        "S" => "start",
                        "T" => "top",
                        "E" => "end",
                        _ => "bottom",
                    };
                    let r = ui
                        .add(
                            DragValue::new(&mut v)
                                .speed(0.5)
                                .range(0.0..=999.0)
                                .max_decimals(0)
                                .prefix(format!("{letter} ")),
                        )
                        .on_hover_text(format!("{title} {side} (dp)"));
                    if r.changed() {
                        let key = merge_key(id, &format!("{title}{letter}"));
                        project.edit(
                            &format!("Change {}", title.to_lowercase()),
                            Some(&key),
                            |doc, res, layout| {
                                edit::set_spacing(
                                    doc,
                                    res,
                                    layout.density,
                                    id,
                                    kind,
                                    axis,
                                    is_hi,
                                    v,
                                )
                            },
                        );
                    }
                }
            });
        }
    });
}

fn text_section(ui: &mut Ui, project: &mut Project, id: NodeId) {
    section(ui, "Text", |ui| {
        let kind = classify(&project.doc, id);
        let style = text_style(&project.doc, &project.res, id, project.layout.density);
        row(ui, "Text", |ui| {
            text_attr(ui, project, id, "android:text", "text", true)
        });
        if let Some(raw) = project.doc.attr(id, "android:text")
            && raw.starts_with("@string/")
        {
            row(ui, "", |ui| {
                ui.label(
                    RichText::new(project.res.resolve_string(&raw))
                        .weak()
                        .italics(),
                );
            });
        }
        if kind == ViewKind::EditText || project.doc.has_attr(id, "android:hint") {
            row(ui, "Hint", |ui| {
                text_attr(ui, project, id, "android:hint", "hint", false)
            });
        }
        row(ui, "Size", |ui| {
            let mut size = style.size;
            if ui
                .add(
                    DragValue::new(&mut size)
                        .speed(0.25)
                        .range(1.0..=400.0)
                        .max_decimals(1)
                        .suffix(" sp"),
                )
                .changed()
            {
                set_attr(
                    project,
                    id,
                    "android:textSize",
                    Some(format_sp(size.round())),
                );
            }
        });
        row(ui, "Color", |ui| {
            color_attr(ui, project, id, "android:textColor")
        });
        row(ui, "Style", |ui| {
            let (mut bold, mut italic) = (style.bold, style.italic);
            let b = ui
                .toggle_value(&mut bold, RichText::new("B").strong())
                .on_hover_text("Bold");
            let i = ui
                .toggle_value(&mut italic, RichText::new("I").italics())
                .on_hover_text("Italic");
            if b.changed() || i.changed() {
                let value = match (bold, italic) {
                    (true, true) => Some("bold|italic".to_string()),
                    (true, false) => Some("bold".to_string()),
                    (false, true) => Some("italic".to_string()),
                    (false, false) => None,
                };
                set_attr(project, id, "android:textStyle", value);
            }
            let mut caps = project.doc.attr(id, "android:textAllCaps").as_deref() == Some("true");
            if ui
                .toggle_value(&mut caps, "AA")
                .on_hover_text("All caps")
                .changed()
            {
                set_attr(
                    project,
                    id,
                    "android:textAllCaps",
                    caps.then(|| "true".to_string()),
                );
            }
        });
        row(ui, "Gravity", |ui| gravity_grid(ui, project, id));
    });
}

/// A 3×3 grid of buttons for `android:gravity` (how content sits inside).
fn gravity_grid(ui: &mut Ui, project: &mut Project, id: NodeId) {
    let g = Gravity::parse(&project.doc.attr(id, "android:gravity").unwrap_or_default());
    let aligns = [Align::Start, Align::Center, Align::End];
    let glyphs = [["↖", "⬆", "↗"], ["⬅", "•", "➡"], ["↙", "⬇", "↘"]];
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing = vec2(2.0, 2.0);
        for (vi, v) in aligns.into_iter().enumerate() {
            ui.horizontal(|ui| {
                for (hi, h) in aligns.into_iter().enumerate() {
                    let on = g.h == Some(h) && g.v == Some(v);
                    let button =
                        egui::Button::selectable(on, glyphs[vi][hi]).min_size(vec2(24.0, 20.0));
                    if ui.add(button).clicked() {
                        let value = Gravity {
                            h: Some(h),
                            v: Some(v),
                        }
                        .to_attr();
                        set_attr(project, id, "android:gravity", Some(value));
                    }
                }
            });
        }
    });
    if project.doc.has_attr(id, "android:gravity")
        && ui
            .small_button("🗙")
            .on_hover_text("Reset gravity")
            .clicked()
    {
        set_attr(project, id, "android:gravity", None);
    }
}

fn appearance_section(ui: &mut Ui, project: &mut Project, id: NodeId) {
    section(ui, "Appearance", |ui| {
        let kind = classify(&project.doc, id);
        let name = short_name(project.doc.name(id)).to_string();
        row(ui, "Background", |ui| {
            color_attr(ui, project, id, "android:background")
        });
        if kind == ViewKind::Button || name == "FloatingActionButton" {
            // Material buttons ignore android:background; their color is a tint.
            let attr = if name.starts_with("Material")
                || name == "FloatingActionButton"
                || project.doc.has_attr(id, "app:backgroundTint")
            {
                "app:backgroundTint"
            } else {
                "android:backgroundTint"
            };
            row(ui, "Tint", |ui| color_attr(ui, project, id, attr));
        }
        if name.contains("CardView") {
            row(ui, "Card color", |ui| {
                color_attr(ui, project, id, "app:cardBackgroundColor")
            });
            for (label, attr) in [
                ("Corner radius", "app:cardCornerRadius"),
                ("Card elevation", "app:cardElevation"),
            ] {
                let a = Attrs {
                    doc: &project.doc,
                    res: &project.res,
                    id,
                };
                let mut v = a.dimension(attr, project.layout.density).unwrap_or(0.0);
                row(ui, label, |ui| {
                    if dp_drag(ui, &mut v, " dp").changed() {
                        set_attr(project, id, attr, Some(format_dp(v.round())));
                    }
                });
            }
        }
        if kind == ViewKind::Image {
            let src_attr = if project.doc.has_attr(id, "app:srcCompat") {
                "app:srcCompat"
            } else {
                "android:src"
            };
            row(ui, "Source", |ui| {
                text_attr(ui, project, id, src_attr, "@drawable/…", false)
            });
            row(ui, "Scale type", |ui| {
                let current = project
                    .doc
                    .attr(id, "android:scaleType")
                    .unwrap_or_default();
                let mut chosen = current.clone();
                ComboBox::from_id_salt(("scale", id))
                    .selected_text(if current.is_empty() {
                        "fitCenter (default)"
                    } else {
                        &current
                    })
                    .show_ui(ui, |ui| {
                        for t in [
                            "fitCenter",
                            "centerCrop",
                            "centerInside",
                            "center",
                            "fitXY",
                            "fitStart",
                            "fitEnd",
                            "matrix",
                        ] {
                            ui.selectable_value(&mut chosen, t.to_string(), t);
                        }
                    });
                if chosen != current {
                    set_attr(project, id, "android:scaleType", Some(chosen));
                }
            });
            row(ui, "Tint", |ui| color_attr(ui, project, id, "app:tint"));
        }
        row(ui, "Alpha", |ui| {
            let mut alpha = project
                .doc
                .attr(id, "android:alpha")
                .and_then(|a| a.parse::<f32>().ok())
                .unwrap_or(1.0);
            if ui
                .add(egui::Slider::new(&mut alpha, 0.0..=1.0).step_by(0.05))
                .changed()
            {
                set_attr(
                    project,
                    id,
                    "android:alpha",
                    (alpha < 0.999).then(|| format_number(alpha)),
                );
            }
        });
        row(ui, "Visibility", |ui| {
            let current = project
                .doc
                .attr(id, "android:visibility")
                .unwrap_or_else(|| "visible".into());
            let mut chosen = current.clone();
            ComboBox::from_id_salt(("visibility", id))
                .selected_text(&current)
                .show_ui(ui, |ui| {
                    for v in ["visible", "invisible", "gone"] {
                        ui.selectable_value(&mut chosen, v.to_string(), v);
                    }
                });
            if chosen != current {
                set_attr(
                    project,
                    id,
                    "android:visibility",
                    (chosen != "visible").then_some(chosen),
                );
            }
        });
        row(ui, "Elevation", |ui| {
            let a = Attrs {
                doc: &project.doc,
                res: &project.res,
                id,
            };
            let mut v = a
                .dimension("android:elevation", project.layout.density)
                .unwrap_or(0.0);
            if dp_drag(ui, &mut v, " dp").changed() {
                set_attr(
                    project,
                    id,
                    "android:elevation",
                    (v >= 0.5).then(|| format_dp(v.round())),
                );
            }
        });
    });
}

fn container_section(ui: &mut Ui, project: &mut Project, id: NodeId) {
    let kind = classify(&project.doc, id).container();
    section(ui, "Children", |ui| {
        if let Some(ContainerKind::Linear { vertical }) = kind {
            row(ui, "Orientation", |ui| {
                let mut v = vertical;
                ui.selectable_value(&mut v, false, "Horizontal");
                ui.selectable_value(&mut v, true, "Vertical");
                if v != vertical {
                    set_attr(
                        project,
                        id,
                        "android:orientation",
                        Some(if v { "vertical" } else { "horizontal" }.to_string()),
                    );
                }
            });
            ui.label(
                RichText::new("Gravity places the children inside this layout.")
                    .small()
                    .weak(),
            );
            row(ui, "Gravity", |ui| gravity_grid(ui, project, id));
        } else if kind == Some(ContainerKind::Frame) || kind == Some(ContainerKind::Relative) {
            row(ui, "Gravity", |ui| gravity_grid(ui, project, id));
        }
        let count = project.doc.element_children(id).len();
        ui.label(
            RichText::new(format!(
                "{count} child view{}",
                if count == 1 { "" } else { "s" }
            ))
            .weak(),
        );
    });
}

fn all_attributes(ui: &mut Ui, project: &mut Project, id: NodeId, state: &mut InspectorState) {
    egui::CollapsingHeader::new(RichText::new("All attributes").strong())
        .default_open(false)
        .show(ui, |ui| {
            let attrs: Vec<(String, String)> = project
                .doc
                .element(id)
                .map(|e| {
                    e.attrs
                        .iter()
                        .map(|a| (a.name.clone(), a.value()))
                        .collect()
                })
                .unwrap_or_default();
            egui::Grid::new(("attrs", id))
                .num_columns(3)
                .striped(true)
                .show(ui, |ui| {
                    for (name, value) in attrs {
                        ui.label(RichText::new(&name).monospace().small());
                        let mut v = value.clone();
                        if ui
                            .add(TextEdit::singleline(&mut v).desired_width(140.0))
                            .changed()
                        {
                            set_attr(project, id, &name, Some(v));
                        }
                        if ui.small_button("🗙").on_hover_text("Remove").clicked() {
                            set_attr(project, id, &name, None);
                        }
                        ui.end_row();
                    }
                });
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.add(
                    TextEdit::singleline(&mut state.new_name)
                        .hint_text("android:…")
                        .desired_width(120.0),
                );
                ui.add(
                    TextEdit::singleline(&mut state.new_value)
                        .hint_text("value")
                        .desired_width(90.0),
                );
                let valid = !state.new_name.trim().is_empty()
                    && !state.new_name.contains(char::is_whitespace);
                if ui.add_enabled(valid, egui::Button::new("Add")).clicked() {
                    let name = state.new_name.trim().to_string();
                    set_attr(project, id, &name, Some(state.new_value.clone()));
                    project.history.seal();
                    state.new_name.clear();
                    state.new_value.clear();
                }
            });
        });
}
