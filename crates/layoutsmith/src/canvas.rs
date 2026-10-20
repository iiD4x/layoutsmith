//! The preview canvas: draws the laid-out views and turns mouse gestures into
//! edits (move, resize, reorder, move into another container).

use eframe::egui::text::{LayoutJob, TextFormat};
use eframe::egui::{
    self, Align2, Color32, CursorIcon, FontId, Pos2, Rect, Sense, Shape, Stroke, StrokeKind, Vec2,
    pos2, vec2,
};
use layoutsmith_core::layout::{Attrs, LayoutResult, Visibility, params, text_style};
use layoutsmith_core::widgets::{ContainerKind, ViewKind, classify, short_name};
use layoutsmith_core::{Align, Color, Document, NodeId, Rect as DpRect, edit};

use crate::project::Project;

pub const ACCENT: Color32 = Color32::from_rgb(0x1E, 0x88, 0xE5);
const DROP: Color32 = Color32::from_rgb(0x2E, 0xA0, 0x43);
const HANDLE_RADIUS: f32 = 5.0;
const MARGIN_FILL: Color32 = Color32::from_rgba_premultiplied(60, 36, 0, 60);
const DEFAULT_PRIMARY: Color = Color::rgb(0x67, 0x50, 0xA4);

pub struct CanvasState {
    pub zoom: f32,
    pub show_bounds: bool,
    /// Snap moves and resizes to an 8dp grid.
    pub snap: bool,
    drag: Option<Drag>,
}

impl Default for CanvasState {
    fn default() -> Self {
        Self {
            zoom: 1.0,
            show_bounds: true,
            snap: false,
            drag: None,
        }
    }
}

impl CanvasState {
    pub fn is_dragging(&self) -> bool {
        self.drag.is_some()
    }

    /// Abandons the current gesture and restores the document.
    pub fn cancel_drag(&mut self, project: &mut Project) {
        if let Some(drag) = self.drag.take() {
            project.preview(drag.origin_doc);
        }
    }

    /// Ends any gesture without touching the document (after it was replaced).
    pub fn forget_drag(&mut self) {
        self.drag = None;
    }
}

/// A resize handle: -1, 0 or 1 on each axis (0 = that axis is not resized).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Handle {
    x: i8,
    y: i8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    Move,
    Resize(Handle),
}

#[derive(Debug, Clone, Copy)]
struct DropTarget {
    parent: NodeId,
    index: usize,
    /// Into a different container (otherwise reordering within a LinearLayout).
    reparent: bool,
}

struct Drag {
    node: NodeId,
    mode: Mode,
    origin_doc: Document,
    origin_layout: LayoutResult,
    /// Pointer position at the press, in dp.
    start: [f32; 2],
    /// Snapped pointer movement, in dp.
    delta: [f32; 2],
    moved: bool,
    drop: Option<DropTarget>,
}

/// Maps dp coordinates to screen points.
#[derive(Clone, Copy)]
struct View {
    origin: Pos2,
    zoom: f32,
}

impl View {
    fn rect(&self, r: DpRect) -> Rect {
        Rect::from_min_size(self.point(r.x, r.y), vec2(r.w, r.h) * self.zoom)
    }

    fn point(&self, x: f32, y: f32) -> Pos2 {
        self.origin + vec2(x, y) * self.zoom
    }

    fn to_dp(self, p: Pos2) -> [f32; 2] {
        let v = (p - self.origin) / self.zoom;
        [v.x, v.y]
    }
}

pub fn to_color32(c: Color, alpha: f32) -> Color32 {
    Color32::from_rgba_unmultiplied(c.r, c.g, c.b, (c.a as f32 * alpha).round() as u8)
}

/// Draws the canvas and handles its gestures.
pub fn show(
    ui: &mut egui::Ui,
    project: &mut Project,
    state: &mut CanvasState,
    selected: &mut Option<NodeId>,
    hovered: &mut Option<NodeId>,
) {
    let zoom_delta = ui.input(|i| i.zoom_delta());
    if zoom_delta != 1.0 && ui.rect_contains_pointer(ui.max_rect()) {
        state.zoom = (state.zoom * zoom_delta).clamp(0.25, 4.0);
    }
    let zoom = state.zoom;
    let screen = project.screen;
    // Scrolling content (and overflowing views) extends the canvas downwards.
    let content_h = project
        .layout
        .rects
        .values()
        .map(|r| r.bottom())
        .fold(screen[1], f32::max)
        .min(screen[1] * 6.0);
    let device = vec2(screen[0], content_h) * zoom;
    let margin = 48.0;

    egui::ScrollArea::both().auto_shrink(false).show(ui, |ui| {
        let desired = (device + Vec2::splat(margin * 2.0)).max(ui.available_size());
        let (outer, response) = ui.allocate_exact_size(desired, Sense::click_and_drag());
        let view = View {
            origin: pos2(outer.center().x - device.x / 2.0, outer.min.y + margin),
            zoom,
        };
        let painter = ui.painter_at(outer);
        let screen_rect = view.rect(DpRect::new(0.0, 0.0, screen[0], screen[1]));
        let full_rect = view.rect(DpRect::new(0.0, 0.0, screen[0], content_h));

        painter.rect_filled(full_rect.expand(10.0), 18.0, Color32::from_gray(38));
        let window_bg = project
            .res
            .resolve_color("?android:attr/windowBackground")
            .unwrap_or(Color::rgb(255, 255, 255));
        painter.rect_filled(full_rect, 0.0, to_color32(window_bg, 1.0));

        let view_painter = painter.with_clip_rect(full_rect);
        for &id in &project.layout.order {
            if let Some(r) = project.layout.rect(id) {
                draw_view(
                    &view_painter,
                    project,
                    id,
                    view.rect(r),
                    zoom,
                    state.show_bounds,
                );
            }
        }
        if content_h > screen[1] {
            let y = screen_rect.bottom();
            painter.extend(Shape::dashed_line(
                &[pos2(full_rect.left(), y), pos2(full_rect.right(), y)],
                Stroke::new(1.0, Color32::from_rgb(0xE5, 0x39, 0x35)),
                6.0,
                4.0,
            ));
            painter.text(
                pos2(full_rect.right() + 14.0, y),
                Align2::LEFT_CENTER,
                "screen end",
                FontId::proportional(11.0),
                Color32::from_rgb(0xE5, 0x39, 0x35),
            );
        }

        handle_input(ui, &response, view, project, state, selected, hovered);
        draw_overlays(&painter, project, view, state, *selected, *hovered);
    });
}

fn handle_input(
    ui: &egui::Ui,
    response: &egui::Response,
    view: View,
    project: &mut Project,
    state: &mut CanvasState,
    selected: &mut Option<NodeId>,
    hovered: &mut Option<NodeId>,
) {
    let hit = |project: &Project, pos: Pos2| {
        let [x, y] = view.to_dp(pos);
        project.layout.hit_test(&project.doc, x, y, None)
    };

    if state.drag.is_none() {
        if let Some(pos) = response.hover_pos() {
            *hovered = hit(project, pos);
            if let Some(h) = selected.and_then(|s| handle_at(project, view, s, pos)) {
                ui.ctx().set_cursor_icon(resize_cursor(h));
            }
        } else if response.contains_pointer() {
            *hovered = None;
        }
    }

    if response.drag_started()
        && let Some(press) = ui.input(|i| i.pointer.press_origin())
    {
        let target = match selected.and_then(|s| handle_at(project, view, s, press)) {
            Some(h) => selected.map(|s| (s, Mode::Resize(h))),
            None => hit(project, press)
                .filter(|&n| project.doc.parent(n).is_some())
                .map(|n| (n, Mode::Move)),
        };
        if let Some((node, mode)) = target {
            *selected = Some(node);
            state.drag = Some(Drag {
                node,
                mode,
                origin_doc: project.doc.clone(),
                origin_layout: project.layout.clone(),
                start: view.to_dp(press),
                delta: [0.0; 2],
                moved: false,
                drop: None,
            });
        }
    }

    if response.dragged()
        && let Some(drag) = &mut state.drag
        && let Some(pos) = ui.input(|i| i.pointer.interact_pos())
    {
        update_drag(project, drag, view.to_dp(pos), state.snap, view.zoom);
        ui.ctx().set_cursor_icon(match drag.mode {
            Mode::Move => CursorIcon::Grabbing,
            Mode::Resize(h) => resize_cursor(h),
        });
    }

    if response.drag_stopped()
        && let Some(drag) = state.drag.take()
    {
        finish_drag(project, drag);
    }

    if response.clicked() {
        *selected = response
            .interact_pointer_pos()
            .and_then(|p| hit(project, p));
    }
}

fn resize_cursor(h: Handle) -> CursorIcon {
    match (h.x, h.y) {
        (0, _) => CursorIcon::ResizeVertical,
        (_, 0) => CursorIcon::ResizeHorizontal,
        (x, y) if x == y => CursorIcon::ResizeNwSe,
        _ => CursorIcon::ResizeNeSw,
    }
}

fn handles(r: Rect) -> impl Iterator<Item = (Handle, Pos2)> {
    (-1i8..=1)
        .flat_map(|y| (-1i8..=1).map(move |x| Handle { x, y }))
        .filter(|h| (h.x, h.y) != (0, 0))
        .map(move |h| {
            let x = egui::lerp(r.left()..=r.right(), (h.x as f32 + 1.0) / 2.0);
            let y = egui::lerp(r.top()..=r.bottom(), (h.y as f32 + 1.0) / 2.0);
            (h, pos2(x, y))
        })
}

fn handle_at(project: &Project, view: View, selected: NodeId, pos: Pos2) -> Option<Handle> {
    project.doc.parent(selected)?;
    let r = view.rect(project.layout.rect(selected)?);
    handles(r)
        .find(|(_, p)| p.distance(pos) <= HANDLE_RADIUS + 3.0)
        .map(|(h, _)| h)
}

/// The nearest container at or above `id`.
fn container_at(doc: &Document, id: NodeId) -> Option<NodeId> {
    let mut cur = Some(id);
    while let Some(c) = cur {
        if let Some(kind) = classify(doc, c).container() {
            // A ScrollView takes exactly one child.
            let full =
                matches!(kind, ContainerKind::Scroll { .. }) && !doc.element_children(c).is_empty();
            if !full {
                return Some(c);
            }
        }
        cur = doc.parent(c);
    }
    None
}

fn update_drag(project: &mut Project, drag: &mut Drag, pointer: [f32; 2], snap: bool, zoom: f32) {
    let raw = [pointer[0] - drag.start[0], pointer[1] - drag.start[1]];
    if !drag.moved && (raw[0].abs() + raw[1].abs()) * zoom < 4.0 {
        return;
    }
    drag.moved = true;
    let step = if snap { 8.0 } else { 1.0 };
    drag.delta = raw.map(|d| (d / step).round() * step);
    let delta = drag.delta;
    let (doc0, l0) = (&drag.origin_doc, &drag.origin_layout);
    let mut doc = doc0.clone();
    drag.drop = None;

    match drag.mode {
        Mode::Resize(h) => {
            let Some(r0) = l0.rect(drag.node) else { return };
            let mut r = r0;
            let snap_len = |len: f32| {
                if snap {
                    (len / step).round() * step
                } else {
                    len.round()
                }
            };
            if h.x != 0 {
                r.w = snap_len((r0.w + delta[0] * h.x as f32).max(0.0));
                if h.x < 0 {
                    r.x = r0.right() - r.w;
                }
            }
            if h.y != 0 {
                r.h = snap_len((r0.h + delta[1] * h.y as f32).max(0.0));
                if h.y < 0 {
                    r.y = r0.bottom() - r.h;
                }
            }
            edit::resize(&mut doc, &project.res, l0, drag.node, r);
        }
        Mode::Move => {
            let parent = doc0.parent(drag.node);
            let linear_parent = parent.filter(|&p| {
                matches!(
                    classify(doc0, p).container(),
                    Some(ContainerKind::Linear { .. })
                )
            });
            let target = l0
                .hit_test(doc0, pointer[0], pointer[1], Some(drag.node))
                .and_then(|h| container_at(doc0, h));
            if let Some(t) = target
                && Some(t) != parent
            {
                drag.drop = Some(DropTarget {
                    parent: t,
                    index: edit::drop_index(doc0, l0, t, drag.node, pointer),
                    reparent: true,
                });
            } else if let Some(p) = linear_parent {
                drag.drop = Some(DropTarget {
                    parent: p,
                    index: edit::drop_index(doc0, l0, p, drag.node, pointer),
                    reparent: false,
                });
            }
            // In a LinearLayout the order decides the position, so the view
            // only moves on drop; elsewhere it follows the pointer, unless it
            // is about to go into another container (then the ghost shows it).
            let leaving = drag.drop.is_some_and(|d| d.reparent);
            if linear_parent.is_none() && !leaving {
                edit::move_by(&mut doc, &project.res, l0, drag.node, delta);
            }
        }
    }
    project.preview(doc);
}

fn finish_drag(project: &mut Project, drag: Drag) {
    if !drag.moved {
        project.preview(drag.origin_doc);
        return;
    }
    let mut label = match drag.mode {
        Mode::Resize(_) => "Resize",
        Mode::Move => "Move",
    };
    if let (Mode::Move, Some(drop)) = (drag.mode, drag.drop) {
        let mut doc = drag.origin_doc.clone();
        if drop.reparent {
            let r0 = drag.origin_layout.rect(drag.node).unwrap_or_default();
            let top_left = [r0.x + drag.delta[0], r0.y + drag.delta[1]];
            edit::reparent(
                &mut doc,
                &project.res,
                &drag.origin_layout,
                drag.node,
                drop.parent,
                drop.index,
                top_left,
            );
            label = "Move into container";
        } else {
            edit::reorder(&mut doc, drag.node, drop.index);
            label = "Reorder";
        }
        project.preview(doc);
    }
    project.commit_preview(&drag.origin_doc, label);
}

// ---- drawing ---------------------------------------------------------------

fn draw_view(
    painter: &egui::Painter,
    project: &Project,
    id: NodeId,
    r: Rect,
    zoom: f32,
    bounds: bool,
) {
    let (doc, res) = (&project.doc, &project.res);
    let density = project.layout.density;
    let a = Attrs { doc, res, id };
    let p = params(doc, res, id, density);
    let bounds_stroke = Stroke::new(1.0, Color32::from_rgba_unmultiplied(120, 120, 140, 70));
    if p.visibility == Visibility::Invisible {
        painter.extend(Shape::dashed_line(&rect_path(r), bounds_stroke, 4.0, 3.0));
        return;
    }
    let kind = classify(doc, id);
    let name = short_name(doc.name(id));
    let alpha = a
        .get("android:alpha")
        .and_then(|v| v.trim().parse::<f32>().ok())
        .unwrap_or(1.0)
        .clamp(0.0, 1.0);
    let col = |c: Color| to_color32(c, alpha);
    let background = a.get("android:background");
    let bg_color = background.as_deref().and_then(|b| res.resolve_color(b));
    let tint = a
        .color("app:backgroundTint")
        .or_else(|| a.color("android:backgroundTint"));
    let content = Rect::from_min_max(
        r.min + vec2(p.padding_lo[0], p.padding_lo[1]) * zoom,
        r.max - vec2(p.padding_hi[0], p.padding_hi[1]) * zoom,
    );

    // The background, remembered so text can pick a readable color.
    let mut fill: Option<Color> = None;
    if name.contains("CardView") {
        let c = a
            .color("app:cardBackgroundColor")
            .unwrap_or(Color::rgb(255, 255, 255));
        let radius = a.dimension("app:cardCornerRadius", density).unwrap_or(4.0) * zoom;
        let elevation = a.dimension("app:cardElevation", density).unwrap_or(2.0);
        if elevation > 0.0 {
            let shadow = r.translate(vec2(0.0, elevation.min(8.0) * zoom * 0.5));
            painter.rect_filled(shadow, radius, Color32::from_black_alpha(36));
        }
        painter.rect_filled(r, radius, col(c));
        fill = Some(c);
    } else if kind == ViewKind::Button && bg_color.is_none() && background.is_none() {
        let c = tint
            .or_else(|| res.resolve_color("?attr/colorPrimary"))
            .unwrap_or(DEFAULT_PRIMARY);
        let body = r.shrink2(vec2(0.0, 4.0 * zoom));
        painter.rect_filled(body, (body.height() / 2.0).min(20.0 * zoom), col(c));
        fill = Some(c);
    } else if name == "FloatingActionButton" {
        let c = tint
            .or_else(|| res.resolve_color("?attr/colorSecondary"))
            .or_else(|| res.resolve_color("?attr/colorAccent"))
            .unwrap_or(Color::rgb(0xD0, 0xBC, 0xFF));
        painter.rect_filled(r, r.width().min(r.height()) * 0.3, col(c));
        let s = r.width().min(r.height()) * 0.2;
        let ink = Stroke::new(
            2.0 * zoom,
            if c.luminance() < 0.5 {
                Color32::WHITE
            } else {
                Color32::BLACK
            },
        );
        painter.line_segment([r.center() - vec2(s, 0.0), r.center() + vec2(s, 0.0)], ink);
        painter.line_segment([r.center() - vec2(0.0, s), r.center() + vec2(0.0, s)], ink);
        fill = Some(c);
    } else if let Some(c) = bg_color {
        let c = tint.unwrap_or(c);
        painter.rect_filled(r, 0.0, col(c));
        fill = Some(c);
    } else if let Some(drawable) = &background {
        // A drawable we cannot render: show where it goes.
        painter.rect_filled(r, 0.0, Color32::from_gray(232).gamma_multiply(alpha));
        if r.height() > 16.0 && r.width() > 60.0 {
            painter.with_clip_rect(r).text(
                r.left_top() + vec2(3.0, 2.0),
                Align2::LEFT_TOP,
                drawable,
                FontId::monospace(9.0),
                Color32::from_gray(140),
            );
        }
    }

    match kind {
        k if k.shows_text() => {
            draw_text_view(painter, project, id, kind, r, content, zoom, fill, alpha)
        }
        ViewKind::Image if name != "FloatingActionButton" => {
            draw_image(painter, &a, r, zoom, fill.is_some(), alpha)
        }
        ViewKind::Progress => {
            let c = res
                .resolve_color("?attr/colorPrimary")
                .unwrap_or(DEFAULT_PRIMARY);
            let stroke = Stroke::new(3.0 * zoom, col(c));
            if name.contains("Linear")
                || name == "SeekBar"
                || name == "Slider"
                || r.width() > r.height() * 2.5
            {
                let y = r.center().y;
                painter.line_segment(
                    [pos2(content.left(), y), pos2(content.right(), y)],
                    Stroke::new(3.0 * zoom, Color32::from_gray(210)),
                );
                let mid = egui::lerp(content.left()..=content.right(), 0.4);
                painter.line_segment([pos2(content.left(), y), pos2(mid, y)], stroke);
            } else {
                painter.circle_stroke(r.center(), r.width().min(r.height()) * 0.35, stroke);
            }
        }
        ViewKind::Include => {
            painter.extend(Shape::dashed_line(
                &rect_path(r),
                Stroke::new(1.0, Color32::from_gray(120)),
                5.0,
                3.0,
            ));
            let layout = a.get("layout").unwrap_or_default();
            painter.with_clip_rect(r).text(
                r.center(),
                Align2::CENTER_CENTER,
                format!("<include {layout}>"),
                FontId::proportional(11.0 * zoom.max(0.8)),
                Color32::from_gray(110),
            );
        }
        ViewKind::Other => {
            painter.rect_filled(r, 2.0, Color32::from_rgba_unmultiplied(150, 150, 170, 40));
            painter.with_clip_rect(r).text(
                r.center(),
                Align2::CENTER_CENTER,
                name,
                FontId::proportional(11.0 * zoom.max(0.8)),
                Color32::from_gray(90),
            );
        }
        _ => {}
    }

    if bounds {
        painter.rect_stroke(r, 0.0, bounds_stroke, StrokeKind::Inside);
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_text_view(
    painter: &egui::Painter,
    project: &Project,
    id: NodeId,
    kind: ViewKind,
    r: Rect,
    content: Rect,
    zoom: f32,
    fill: Option<Color>,
    alpha: f32,
) {
    let (doc, res) = (&project.doc, &project.res);
    let density = project.layout.density;
    let style = text_style(doc, res, id, density);
    let name = short_name(doc.name(id));
    let primary = res
        .resolve_color("?attr/colorPrimary")
        .unwrap_or(DEFAULT_PRIMARY);
    let on_dark = fill.is_some_and(|bg| bg.a > 128 && bg.luminance() < 0.55);
    let default_color = if style.is_hint {
        Color32::from_black_alpha(110)
    } else if on_dark {
        Color32::WHITE
    } else {
        Color32::from_black_alpha(222)
    };
    let color = style
        .color
        .map_or(default_color.gamma_multiply(alpha), |c| {
            to_color32(c, alpha)
        });

    let mut text_rect = content;
    match kind {
        ViewKind::Check if name.contains("Switch") => {
            let track = Rect::from_center_size(
                pos2(content.right() - 20.0 * zoom, content.center().y),
                vec2(36.0, 20.0) * zoom,
            );
            painter.rect_stroke(
                track,
                10.0 * zoom,
                Stroke::new(2.0 * zoom, Color32::from_gray(120)),
                StrokeKind::Inside,
            );
            painter.circle_filled(
                track.left_center() + vec2(10.0 * zoom, 0.0),
                6.0 * zoom,
                Color32::from_gray(120),
            );
            text_rect.max.x -= 44.0 * zoom;
        }
        ViewKind::Check => {
            let center = pos2(content.left() + 12.0 * zoom, content.center().y);
            let stroke = Stroke::new(2.0 * zoom, to_color32(primary, alpha));
            if name.contains("Radio") {
                painter.circle_stroke(center, 8.0 * zoom, stroke);
            } else {
                painter.rect_stroke(
                    Rect::from_center_size(center, Vec2::splat(16.0 * zoom)),
                    2.0 * zoom,
                    stroke,
                    StrokeKind::Inside,
                );
            }
            text_rect.min.x += 32.0 * zoom;
        }
        ViewKind::EditText => {
            let y = r.bottom() - 6.0 * zoom;
            painter.line_segment(
                [
                    pos2(r.left() + 4.0 * zoom, y),
                    pos2(r.right() - 4.0 * zoom, y),
                ],
                Stroke::new(1.0 * zoom.max(1.0), Color32::from_gray(130)),
            );
        }
        _ => {}
    }
    if style.text.is_empty() || text_rect.width() <= 1.0 {
        return;
    }

    let gravity = params(doc, res, id, density).gravity;
    let (h_default, v_default) = match kind {
        ViewKind::Button => (Align::Center, Align::Center),
        ViewKind::EditText | ViewKind::Check => (Align::Start, Align::Center),
        _ => (Align::Start, Align::Start),
    };
    let mut job = LayoutJob::single_section(
        style.text.clone(),
        TextFormat {
            font_id: FontId::proportional(style.size * zoom),
            color,
            italics: style.italic,
            ..Default::default()
        },
    );
    job.wrap.max_width = text_rect.width();
    // Lines line up inside the text block the same way the block sits in the view.
    let h = gravity.h.unwrap_or(h_default);
    job.halign = match h {
        Align::Center => egui::Align::Center,
        Align::End => egui::Align::RIGHT,
        _ => egui::Align::LEFT,
    };
    let galley = painter.layout_job(job);
    let size = galley.size();
    // With centered or right alignment the galley is anchored at its center
    // or right edge.
    let x = match h {
        Align::Center => text_rect.center().x,
        Align::End => text_rect.right(),
        _ => text_rect.left(),
    };
    let y = match gravity.v.unwrap_or(v_default) {
        Align::Center => text_rect.center().y - size.y / 2.0,
        Align::End => text_rect.bottom() - size.y,
        _ => text_rect.top(),
    };
    let clipped = painter.with_clip_rect(r.intersect(painter.clip_rect()));
    if style.bold {
        clipped.galley(pos2(x + 0.6 * zoom, y), galley.clone(), color);
    }
    clipped.galley(pos2(x, y), galley, color);
}

fn draw_image(
    painter: &egui::Painter,
    a: &Attrs<'_>,
    r: Rect,
    zoom: f32,
    has_background: bool,
    alpha: f32,
) {
    let src = a.get("android:src").or_else(|| a.get("app:srcCompat"));
    if let Some(c) = src.as_deref().and_then(|s| a.res.resolve_color(s)) {
        painter.rect_filled(r, 0.0, to_color32(c, alpha));
        return;
    }
    if !has_background {
        painter.rect_filled(r, 0.0, Color32::from_gray(225).gamma_multiply(alpha));
    }
    let ink = a
        .color("app:tint")
        .or_else(|| a.color("android:tint"))
        .map_or(Color32::from_gray(150), |c| to_color32(c, alpha));
    let s = r.width().min(r.height()) * 0.5;
    if s < 6.0 {
        return;
    }
    // A little landscape: two hills and a sun.
    let c = r.center();
    let base = c.y + s * 0.35;
    painter.add(Shape::convex_polygon(
        vec![
            pos2(c.x - s * 0.5, base),
            pos2(c.x - s * 0.1, c.y - s * 0.15),
            pos2(c.x + s * 0.3, base),
        ],
        ink,
        Stroke::NONE,
    ));
    painter.add(Shape::convex_polygon(
        vec![
            pos2(c.x, base),
            pos2(c.x + s * 0.25, c.y + s * 0.05),
            pos2(c.x + s * 0.5, base),
        ],
        ink,
        Stroke::NONE,
    ));
    painter.circle_filled(pos2(c.x + s * 0.25, c.y - s * 0.25), s * 0.1, ink);
    if let Some(src) = src
        && r.height() > 56.0 * zoom.min(1.0)
        && r.width() > 70.0
    {
        let label = src.rsplit('/').next().unwrap_or(&src).to_string();
        painter.with_clip_rect(r).text(
            pos2(c.x, r.bottom() - 3.0),
            Align2::CENTER_BOTTOM,
            label,
            FontId::proportional(10.0),
            Color32::from_gray(110),
        );
    }
}

fn rect_path(r: Rect) -> [Pos2; 5] {
    [
        r.left_top(),
        r.right_top(),
        r.right_bottom(),
        r.left_bottom(),
        r.left_top(),
    ]
}

fn draw_overlays(
    painter: &egui::Painter,
    project: &Project,
    view: View,
    state: &CanvasState,
    selected: Option<NodeId>,
    hovered: Option<NodeId>,
) {
    let layout = &project.layout;
    if let Some(h) = hovered.filter(|&h| Some(h) != selected)
        && let Some(r) = layout.rect(h)
    {
        painter.rect_stroke(
            view.rect(r),
            0.0,
            Stroke::new(1.5, ACCENT.gamma_multiply(0.6)),
            StrokeKind::Outside,
        );
    }

    if let Some(drag) = &state.drag {
        draw_drag_feedback(painter, view, drag);
    }

    let Some(sel) = selected else { return };
    let Some(dp) = layout.rect(sel) else { return };
    let r = view.rect(dp);
    let doc = &project.doc;

    if let Some(parent) = doc.parent(sel)
        && let Some(pr) = layout.rect(parent)
    {
        painter.extend(Shape::dashed_line(
            &rect_path(view.rect(pr)),
            Stroke::new(1.0, Color32::from_rgb(0x8E, 0x24, 0xAA)),
            5.0,
            3.0,
        ));
    }

    // Margins.
    let p = params(doc, &project.res, sel, layout.density);
    let z = view.zoom;
    let margin_rects = [
        Rect::from_min_max(
            pos2(r.left() - p.margin_lo[0] * z, r.top()),
            pos2(r.left(), r.bottom()),
        ),
        Rect::from_min_max(
            pos2(r.right(), r.top()),
            pos2(r.right() + p.margin_hi[0] * z, r.bottom()),
        ),
        Rect::from_min_max(
            pos2(r.left(), r.top() - p.margin_lo[1] * z),
            pos2(r.right(), r.top()),
        ),
        Rect::from_min_max(
            pos2(r.left(), r.bottom()),
            pos2(r.right(), r.bottom() + p.margin_hi[1] * z),
        ),
    ];
    for m in margin_rects {
        if m.width() > 0.0 && m.height() > 0.0 {
            painter.rect_filled(m, 0.0, MARGIN_FILL);
        }
    }

    // Constraints and RelativeLayout rules: a line to each anchor.
    if let Some(anchors) = layout.anchors.get(&sel) {
        let stroke = Stroke::new(1.5, ACCENT);
        let (cx, cy) = (r.center().x, r.center().y);
        let link = |from: Pos2, to: Pos2| {
            if from.distance(to) > 0.5 {
                painter.line_segment([from, to], stroke);
            }
            painter.circle_filled(to, 3.0, ACCENT);
        };
        if let Some(x) = anchors.lo[0] {
            link(pos2(r.left(), cy), pos2(view.point(x, 0.0).x, cy));
        }
        if let Some(x) = anchors.hi[0] {
            link(pos2(r.right(), cy), pos2(view.point(x, 0.0).x, cy));
        }
        if let Some(y) = anchors.lo[1] {
            link(pos2(cx, r.top()), pos2(cx, view.point(0.0, y).y));
        }
        if let Some(y) = anchors.hi[1] {
            link(pos2(cx, r.bottom()), pos2(cx, view.point(0.0, y).y));
        }
    }

    painter.rect_stroke(r, 0.0, Stroke::new(2.0, ACCENT), StrokeKind::Outside);
    if doc.parent(sel).is_some() && !state.is_dragging() {
        for (_, pos) in handles(r) {
            painter.circle_filled(pos, HANDLE_RADIUS, Color32::WHITE);
            painter.circle_stroke(pos, HANDLE_RADIUS, Stroke::new(1.5, ACCENT));
        }
    }
    let label = format!(
        "{} × {}",
        layoutsmith_core::units::format_number(dp.w),
        layoutsmith_core::units::format_number(dp.h)
    );
    let pos = pos2(r.center().x, r.bottom() + 10.0);
    let galley = painter.layout_no_wrap(label, FontId::proportional(11.0), Color32::WHITE);
    let pill = Rect::from_center_size(
        pos + vec2(0.0, galley.size().y / 2.0),
        galley.size() + vec2(10.0, 4.0),
    );
    painter.rect_filled(pill, 4.0, ACCENT);
    painter.galley(pill.center() - galley.size() / 2.0, galley, Color32::WHITE);
}

fn draw_drag_feedback(painter: &egui::Painter, view: View, drag: &Drag) {
    let l0 = &drag.origin_layout;
    let doc0 = &drag.origin_doc;
    let Some(drop) = drag.drop else { return };
    if let Some(r0) = l0.rect(drag.node) {
        // Ghost of the view where it would land.
        let ghost = DpRect::new(r0.x + drag.delta[0], r0.y + drag.delta[1], r0.w, r0.h);
        painter.rect_filled(view.rect(ghost), 0.0, ACCENT.gamma_multiply(0.25));
    }
    let Some(pr) = l0.rect(drop.parent) else {
        return;
    };
    if drop.reparent {
        painter.rect_stroke(
            view.rect(pr),
            0.0,
            Stroke::new(2.5, DROP),
            StrokeKind::Inside,
        );
    }
    let Some(ContainerKind::Linear { vertical }) = classify(doc0, drop.parent).container() else {
        return;
    };
    let axis = vertical as usize;
    let siblings: Vec<DpRect> = doc0
        .element_children(drop.parent)
        .into_iter()
        .filter(|&c| c != drag.node)
        .filter_map(|c| l0.rect(c))
        .collect();
    let at = match (siblings.get(drop.index), siblings.last()) {
        (Some(s), _) => s.pos(axis) - 1.0,
        (None, Some(last)) => last.pos(axis) + last.len(axis) + 1.0,
        (None, None) => pr.pos(axis) + 4.0,
    };
    let (a, b) = if vertical {
        (view.point(pr.x, at), view.point(pr.right(), at))
    } else {
        (view.point(at, pr.y), view.point(at, pr.bottom()))
    };
    painter.line_segment([a, b], Stroke::new(3.0, DROP));
}
