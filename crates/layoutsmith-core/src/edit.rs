//! Design edits expressed as attribute changes.
//!
//! Moving a view means different things depending on its parent: a margin in
//! a FrameLayout, a bias or a new constraint in a ConstraintLayout, a new
//! position among its siblings in a LinearLayout. These functions translate
//! "put it there" into whatever the parent understands, touching as few
//! attributes as possible.

use crate::layout::{
    LayoutResult, Rect, Target, bias_attr, constraint_rules, params, relative_rules,
};
use crate::resources::Resources;
use crate::units::{Align, Dim, Gravity, format_dp, format_number};
use crate::widgets::{ContainerKind, classify, short_name};
use crate::xml::{APP_NS, Document, NodeId, id_name};

/// The kind of container a view sits in.
pub fn parent_kind(doc: &Document, id: NodeId) -> Option<ContainerKind> {
    classify(doc, doc.parent(id)?).container()
}

pub fn size_attr(axis: usize) -> &'static str {
    if axis == 0 {
        "android:layout_width"
    } else {
        "android:layout_height"
    }
}

pub fn set_size(doc: &mut Document, id: NodeId, axis: usize, dim: Dim) {
    doc.set_attr(id, size_attr(axis), &dim.to_attr());
}

/// Margins (outside a view) or padding (inside it).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Spacing {
    Margin,
    Padding,
}

impl Spacing {
    fn prefix(self) -> &'static str {
        match self {
            Spacing::Margin => "android:layout_margin",
            Spacing::Padding => "android:padding",
        }
    }

    /// Attribute names for one side, preferred spelling first.
    fn side_names(self, axis: usize, hi: bool) -> Vec<String> {
        let suffixes: &[&str] = match (axis, hi) {
            (0, false) => &["Start", "Left"],
            (0, true) => &["End", "Right"],
            (_, false) => &["Top"],
            (_, true) => &["Bottom"],
        };
        suffixes
            .iter()
            .map(|s| format!("{}{s}", self.prefix()))
            .collect()
    }

    fn shorthands(self) -> [String; 3] {
        let p = self.prefix();
        [
            p.to_string(),
            format!("{p}Horizontal"),
            format!("{p}Vertical"),
        ]
    }

    fn all_names(self) -> Vec<String> {
        let mut names = self.shorthands().to_vec();
        for (axis, hi) in [(0, false), (0, true), (1, false), (1, true)] {
            names.extend(self.side_names(axis, hi));
        }
        names
    }

    /// Current values as (left/top, right/bottom).
    fn current(
        self,
        doc: &Document,
        res: &Resources,
        id: NodeId,
        density: f32,
    ) -> ([f32; 2], [f32; 2]) {
        let p = params(doc, res, id, density);
        match self {
            Spacing::Margin => (p.margin_lo, p.margin_hi),
            Spacing::Padding => (p.padding_lo, p.padding_hi),
        }
    }
}

/// Sets a margin.
pub fn set_margin(
    doc: &mut Document,
    res: &Resources,
    density: f32,
    id: NodeId,
    axis: usize,
    hi: bool,
    value: f32,
) {
    set_spacing(doc, res, density, id, Spacing::Margin, axis, hi, value);
}

/// Sets one side of a margin or padding (in dp, clamped at zero, rounded).
/// Shorthands such as `layout_margin` are split into per-side attributes first
/// so the other sides keep their values.
#[allow(clippy::too_many_arguments)]
pub fn set_spacing(
    doc: &mut Document,
    res: &Resources,
    density: f32,
    id: NodeId,
    kind: Spacing,
    axis: usize,
    hi: bool,
    value: f32,
) {
    let (lo_now, hi_now) = kind.current(doc, res, id, density);
    let current = if hi { hi_now[axis] } else { lo_now[axis] };
    let value = value.max(0.0).round();
    if (value - current).abs() < 0.01 {
        return;
    }
    let has_shorthand = kind.shorthands().iter().any(|n| doc.has_attr(id, n));
    // Buttons and fields have default padding that disappears as soon as any
    // padding attribute is set, so write the defaults out explicitly.
    let implicit_defaults = kind == Spacing::Padding
        && !kind.all_names().iter().any(|n| doc.has_attr(id, n))
        && (lo_now != [0.0; 2] || hi_now != [0.0; 2]);
    if has_shorthand || implicit_defaults {
        for name in kind.all_names() {
            doc.remove_attr(id, &name);
        }
        for a in 0..2 {
            write_side(doc, id, kind, a, false, lo_now[a]);
            write_side(doc, id, kind, a, true, hi_now[a]);
        }
    }
    write_side(doc, id, kind, axis, hi, value);
}

fn write_side(doc: &mut Document, id: NodeId, kind: Spacing, axis: usize, hi: bool, value: f32) {
    let names = kind.side_names(axis, hi);
    // Keep using whichever spelling the file already uses.
    let mut targets: Vec<&String> = names.iter().filter(|n| doc.has_attr(id, n)).collect();
    if targets.is_empty() {
        targets.push(&names[0]);
    }
    for name in targets {
        if value == 0.0 {
            doc.remove_attr(id, name);
        } else {
            doc.set_attr(id, name, &format_dp(value));
        }
    }
}

pub fn clear_margins(doc: &mut Document, id: NodeId) {
    for name in Spacing::Margin.all_names() {
        doc.remove_attr(id, &name);
    }
}

/// Changes `layout_gravity` along one axis, keeping the other axis.
pub fn set_layout_gravity(doc: &mut Document, id: NodeId, axis: usize, align: Option<Align>) {
    let mut g = Gravity::parse(&doc.attr(id, "android:layout_gravity").unwrap_or_default());
    g.set_axis(axis, align);
    if g.is_empty() {
        doc.remove_attr(id, "android:layout_gravity");
    } else {
        doc.set_attr(id, "android:layout_gravity", &g.to_attr());
    }
}

/// Changes `gravity` (how a view arranges its own content) along one axis.
pub fn set_gravity(doc: &mut Document, id: NodeId, axis: usize, align: Option<Align>) {
    let mut g = Gravity::parse(&doc.attr(id, "android:gravity").unwrap_or_default());
    g.set_axis(axis, align);
    if g.is_empty() {
        doc.remove_attr(id, "android:gravity");
    } else {
        doc.set_attr(id, "android:gravity", &g.to_attr());
    }
}

fn set_bias(doc: &mut Document, id: NodeId, axis: usize, bias: f32) {
    if (bias - 0.5).abs() < 0.005 {
        doc.remove_attr(id, bias_attr(axis));
    } else {
        doc.set_attr(id, bias_attr(axis), &format_number(bias));
    }
}

fn absolute_attr(axis: usize) -> &'static str {
    if axis == 0 {
        "tools:layout_editor_absoluteX"
    } else {
        "tools:layout_editor_absoluteY"
    }
}

/// Moves a view by `delta` dp.
pub fn move_by(
    doc: &mut Document,
    res: &Resources,
    layout: &LayoutResult,
    id: NodeId,
    delta: [f32; 2],
) {
    for (a, &d) in delta.iter().enumerate() {
        if d != 0.0 {
            shift_edges(doc, res, layout, id, a, d, d);
        }
    }
}

/// Resizes a view to `target` (absolute dp), keeping its kind of anchoring:
/// a view pinned to its parent's end stays pinned there, and so on.
pub fn resize(
    doc: &mut Document,
    res: &Resources,
    layout: &LayoutResult,
    id: NodeId,
    target: Rect,
) {
    let Some(rect) = layout.rect(id) else { return };
    for a in 0..2 {
        let d_lo = target.pos(a) - rect.pos(a);
        let d_hi = target.pos(a) + target.len(a) - (rect.pos(a) + rect.len(a));
        if d_lo.abs() < 0.01 && d_hi.abs() < 0.01 {
            continue;
        }
        set_size(doc, id, a, Dim::Dp(target.len(a).max(0.0).round()));
        if let Some(ContainerKind::Linear { vertical }) = parent_kind(doc, id)
            && a == vertical as usize
        {
            // A weight would add free space on top of the new size.
            doc.remove_attr(id, "android:layout_weight");
        }
        shift_edges(doc, res, layout, id, a, d_lo, d_hi);
    }
}

/// Moves the low (left/top) and high (right/bottom) edges of a view along one
/// axis. Only the edge that the parent positions the view by matters; the
/// size is set separately.
fn shift_edges(
    doc: &mut Document,
    res: &Resources,
    layout: &LayoutResult,
    id: NodeId,
    a: usize,
    d_lo: f32,
    d_hi: f32,
) {
    let Some(parent) = doc.parent(id) else { return };
    let (Some(rect), Some(prect)) = (layout.rect(id), layout.rect(parent)) else {
        return;
    };
    let density = layout.density;
    let p = params(doc, res, id, density);
    let pp = params(doc, res, parent, density);
    let d_center = (d_lo + d_hi) / 2.0;
    // Offset from the parent's content edge, the value a start margin would need.
    let offset = rect.pos(a) - prect.pos(a) - pp.padding_lo[a];
    let margin = |doc: &mut Document, hi: bool, v: f32| set_margin(doc, res, density, id, a, hi, v);

    match classify(doc, parent).container() {
        Some(ContainerKind::Constraint) => {
            let anchors = layout.anchors.get(&id).copied().unwrap_or_default();
            match (anchors.lo[a], anchors.hi[a]) {
                (Some(lo), Some(hi)) => {
                    let new_len = rect.len(a) + d_hi - d_lo;
                    let start = lo + p.margin_lo[a];
                    let span = hi - p.margin_hi[a] - start - new_len;
                    if span > 0.5 {
                        let bias = ((rect.pos(a) + d_lo - start) / span).clamp(0.0, 1.0);
                        set_bias(doc, id, a, bias);
                    } else {
                        margin(doc, false, p.margin_lo[a] + d_lo);
                    }
                }
                (Some(_), None) => margin(doc, false, p.margin_lo[a] + d_lo),
                (None, Some(_)) => margin(doc, true, p.margin_hi[a] - d_hi),
                (None, None) => {
                    // Unconstrained views jump to the corner on a device; pin
                    // them to the parent where they are shown instead.
                    doc.ensure_namespace("app", APP_NS);
                    let attr = if a == 0 {
                        "app:layout_constraintStart_toStartOf"
                    } else {
                        "app:layout_constraintTop_toTopOf"
                    };
                    doc.set_attr(id, attr, "parent");
                    doc.remove_attr(id, absolute_attr(a));
                    margin(doc, false, offset + d_lo);
                }
            }
        }
        Some(ContainerKind::Relative) => {
            let rules: Vec<_> = relative_rules(doc, id)
                .into_iter()
                .filter(|r| r.axis == a)
                .collect();
            let centered = rules.iter().any(|r| r.target == Target::Center);
            let has_lo = rules
                .iter()
                .any(|r| r.target != Target::Center && !r.hi_side);
            let has_hi = rules.iter().any(|r| r.hi_side);
            if centered && !has_lo && !has_hi {
                // Centering ignores margins, so pin to the start instead.
                remove_center_rule(doc, id, a);
                margin(doc, false, offset + d_lo);
            } else if has_hi && !has_lo {
                margin(doc, true, p.margin_hi[a] - d_hi);
            } else {
                margin(doc, false, p.margin_lo[a] + d_lo);
            }
        }
        Some(ContainerKind::Linear { vertical }) if a != vertical as usize => {
            let align = p.layout_gravity.axis(a).or(pp.gravity.axis(a));
            match align {
                Some(Align::End) => margin(doc, true, p.margin_hi[a] - d_hi),
                Some(Align::Center) => margin(doc, false, p.margin_lo[a] + d_center),
                _ => margin(doc, false, p.margin_lo[a] + d_lo),
            }
        }
        // Along a LinearLayout's main axis the order decides the position;
        // the leading margin is the only thing to adjust.
        Some(ContainerKind::Linear { .. }) => margin(doc, false, p.margin_lo[a] + d_lo),
        _ => match p.layout_gravity.axis(a) {
            Some(Align::End) => margin(doc, true, p.margin_hi[a] - d_hi),
            Some(Align::Center) => margin(doc, false, p.margin_lo[a] + d_center),
            _ => margin(doc, false, p.margin_lo[a] + d_lo),
        },
    }
}

fn remove_center_rule(doc: &mut Document, id: NodeId, axis: usize) {
    if doc.attr(id, "android:layout_centerInParent").as_deref() == Some("true") {
        doc.remove_attr(id, "android:layout_centerInParent");
        let other = if axis == 0 {
            "android:layout_centerVertical"
        } else {
            "android:layout_centerHorizontal"
        };
        doc.set_attr(id, other, "true");
    }
    let own = if axis == 0 {
        "android:layout_centerHorizontal"
    } else {
        "android:layout_centerVertical"
    };
    doc.remove_attr(id, own);
}

/// Removes every RelativeLayout rule that positions the view along `axis`.
fn clear_relative_axis(doc: &mut Document, id: NodeId, axis: usize) {
    remove_center_rule(doc, id, axis);
    for rule in relative_rules(doc, id) {
        if rule.axis == axis {
            doc.remove_attr(id, &rule.attr);
        }
    }
}

/// Removes every constraint (and bias) along `axis`.
fn clear_constraint_axis(doc: &mut Document, id: NodeId, axis: usize) {
    for rule in constraint_rules(doc, id) {
        if rule.axis == axis {
            doc.remove_attr(id, &rule.attr);
        }
    }
    doc.remove_attr(id, bias_attr(axis));
    doc.remove_attr(id, absolute_attr(axis));
}

/// Aligns a view inside its parent along one axis. `Align::Fill` stretches it.
pub fn align(
    doc: &mut Document,
    res: &Resources,
    layout: &LayoutResult,
    id: NodeId,
    axis: usize,
    align: Align,
) {
    let Some(parent) = doc.parent(id) else { return };
    let density = layout.density;
    match classify(doc, parent).container() {
        Some(ContainerKind::Constraint) => {
            doc.ensure_namespace("app", APP_NS);
            clear_constraint_axis(doc, id, axis);
            let (lo, hi) = if axis == 0 {
                (
                    "app:layout_constraintStart_toStartOf",
                    "app:layout_constraintEnd_toEndOf",
                )
            } else {
                (
                    "app:layout_constraintTop_toTopOf",
                    "app:layout_constraintBottom_toBottomOf",
                )
            };
            if align != Align::End {
                doc.set_attr(id, lo, "parent");
            }
            if align != Align::Start {
                doc.set_attr(id, hi, "parent");
            }
            let size = params(doc, res, id, density).size[axis];
            if align == Align::Fill {
                set_size(doc, id, axis, Dim::Dp(0.0));
            } else if size.is_zero() || size == Dim::Match {
                set_size(doc, id, axis, Dim::Wrap);
            }
        }
        Some(ContainerKind::Relative) => {
            clear_relative_axis(doc, id, axis);
            let (start, end, center) = if axis == 0 {
                (
                    "android:layout_alignParentStart",
                    "android:layout_alignParentEnd",
                    "android:layout_centerHorizontal",
                )
            } else {
                (
                    "android:layout_alignParentTop",
                    "android:layout_alignParentBottom",
                    "android:layout_centerVertical",
                )
            };
            match align {
                Align::Start => doc.set_attr(id, start, "true"),
                Align::End => doc.set_attr(id, end, "true"),
                Align::Center => doc.set_attr(id, center, "true"),
                Align::Fill => set_size(doc, id, axis, Dim::Match),
            }
            if axis == 0 {
                // Old files spell it left/right; drop those duplicates.
                doc.remove_attr(id, "android:layout_alignParentLeft");
                doc.remove_attr(id, "android:layout_alignParentRight");
            }
        }
        Some(ContainerKind::Linear { vertical }) if axis == vertical as usize => {
            // Along the main axis children are stacked, so alignment is the
            // parent's gravity and "fill" means taking the remaining space.
            match align {
                Align::Fill => {
                    set_size(doc, id, axis, Dim::Dp(0.0));
                    doc.set_attr(id, "android:layout_weight", "1");
                }
                a => set_gravity(
                    doc,
                    parent,
                    axis,
                    if a == Align::Start { None } else { Some(a) },
                ),
            }
        }
        _ => match align {
            Align::Fill => {
                set_size(doc, id, axis, Dim::Match);
                set_layout_gravity(doc, id, axis, None);
            }
            a => set_layout_gravity(doc, id, axis, Some(a)),
        },
    }
}

/// The index among `parent`'s children (not counting `moving`) at which a
/// view dropped at `point` lands. Meaningful for LinearLayouts.
pub fn drop_index(
    doc: &Document,
    layout: &LayoutResult,
    parent: NodeId,
    moving: NodeId,
    point: [f32; 2],
) -> usize {
    let axis = match classify(doc, parent).container() {
        Some(ContainerKind::Linear { vertical }) => vertical as usize,
        _ => 1,
    };
    doc.element_children(parent)
        .into_iter()
        .filter(|&c| c != moving)
        .filter(|&c| {
            layout
                .rect(c)
                .is_some_and(|r| r.pos(axis) + r.len(axis) / 2.0 < point[axis])
        })
        .count()
}

/// Moves a view to another position among its LinearLayout siblings.
pub fn reorder(doc: &mut Document, id: NodeId, index: usize) {
    if let Some(parent) = doc.parent(id) {
        doc.move_element(id, parent, index);
    }
}

/// Moves a view into another container. `index` is its position among the
/// new siblings; `top_left` is where its corner should end up (absolute dp),
/// used by containers that position children freely.
pub fn reparent(
    doc: &mut Document,
    res: &Resources,
    layout: &LayoutResult,
    id: NodeId,
    new_parent: NodeId,
    index: usize,
    top_left: [f32; 2],
) {
    if doc.is_ancestor_or_self(id, new_parent) {
        return;
    }
    let density = layout.density;
    let new_kind = classify(doc, new_parent).container();
    let rect = layout.rect(id);
    if doc.parent(id) != Some(new_parent) {
        bridge_references(doc, id);
    }
    strip_parent_specific(doc, id);
    clear_margins(doc, id);
    // 0dp only means something inside a ConstraintLayout or with a weight.
    if new_kind != Some(ContainerKind::Constraint) {
        let size = params(doc, res, id, density).size;
        for (a, dim) in size.iter().enumerate() {
            if dim.is_zero()
                && let Some(r) = rect
            {
                set_size(doc, id, a, Dim::Dp(r.len(a).round()));
            }
        }
    }
    doc.move_element(id, new_parent, index);

    if matches!(new_kind, Some(ContainerKind::Linear { .. })) {
        return;
    }
    let (Some(prect), pp) = (
        layout.rect(new_parent),
        params(doc, res, new_parent, density),
    ) else {
        return;
    };
    if new_kind == Some(ContainerKind::Constraint) {
        doc.ensure_namespace("app", APP_NS);
        doc.set_attr(id, "app:layout_constraintStart_toStartOf", "parent");
        doc.set_attr(id, "app:layout_constraintTop_toTopOf", "parent");
    }
    for (a, &corner) in top_left.iter().enumerate() {
        let offset = corner - prect.pos(a) - pp.padding_lo[a];
        set_margin(doc, res, density, id, a, false, offset);
    }
}

/// Removes layout parameters that only mean something to the current parent.
fn strip_parent_specific(doc: &mut Document, id: NodeId) {
    let names: Vec<String> = doc
        .element(id)
        .map(|e| e.attrs.iter().map(|a| a.name.clone()).collect())
        .unwrap_or_default();
    for name in names {
        let specific = name.starts_with("app:layout_constraint")
            || name.starts_with("tools:layout_editor_absolute")
            || name == "android:layout_weight"
            || name == "android:layout_gravity"
            || relative_rule_attr(&name);
        if specific {
            doc.remove_attr(id, &name);
        }
    }
}

fn relative_rule_attr(name: &str) -> bool {
    let Some(rest) = name.strip_prefix("android:layout_") else {
        return false;
    };
    rest.starts_with("alignParent")
        || rest.starts_with("center")
        || rest.starts_with("to")
        || rest.starts_with("align")
        || rest == "below"
        || rest == "above"
}

/// Removes a view. The root element cannot be deleted.
pub fn delete(doc: &mut Document, id: NodeId) -> bool {
    if doc.parent(id).is_none() {
        return false;
    }
    bridge_references(doc, id);
    doc.detach(id);
    true
}

/// `app:layout_constraintTop_toBottomOf` -> (`Top`, `Bottom`).
fn constraint_sides(attr: &str) -> Option<(&str, &str)> {
    attr.strip_prefix("app:layout_constraint")?
        .strip_suffix("Of")?
        .split_once("_to")
}

/// Before a view leaves its ConstraintLayout or RelativeLayout, re-attaches
/// the siblings positioned relative to it to whatever it was attached to, so
/// they stay roughly where they were instead of jumping to the corner.
pub fn bridge_references(doc: &mut Document, id: NodeId) {
    let Some(parent) = doc.parent(id) else { return };
    let Some(name) = doc
        .attr(id, "android:id")
        .and_then(|v| id_name(&v).map(str::to_string))
    else {
        return;
    };
    let me = Target::Sibling(name);
    let siblings: Vec<NodeId> = doc
        .element_children(parent)
        .into_iter()
        .filter(|&c| c != id)
        .collect();
    match classify(doc, parent).container() {
        Some(ContainerKind::Constraint) => {
            let own = constraint_rules(doc, id);
            for sib in siblings {
                for rule in constraint_rules(doc, sib)
                    .into_iter()
                    .filter(|r| r.target == me)
                {
                    doc.remove_attr(sib, &rule.attr);
                    // Prefer what the edge it was attached to was attached to.
                    let replacement = own
                        .iter()
                        .filter(|r| r.axis == rule.axis)
                        .find(|r| r.hi_side == rule.target_hi)
                        .or_else(|| own.iter().find(|r| r.axis == rule.axis));
                    let (Some(rep), Some((side, _))) = (replacement, constraint_sides(&rule.attr))
                    else {
                        continue;
                    };
                    let (Some((_, target_side)), Some(value)) =
                        (constraint_sides(&rep.attr), doc.attr(id, &rep.attr))
                    else {
                        continue;
                    };
                    let points_back = id_name(&value).is_some_and(|n| {
                        doc.attr(sib, "android:id").as_deref().and_then(id_name) == Some(n)
                    });
                    if !points_back {
                        doc.set_attr(
                            sib,
                            &format!("app:layout_constraint{side}_to{target_side}Of"),
                            &value,
                        );
                    }
                }
            }
        }
        Some(ContainerKind::Relative) => {
            for sib in siblings {
                for rule in relative_rules(doc, sib)
                    .into_iter()
                    .filter(|r| r.target == me)
                {
                    match doc.attr(id, &rule.attr) {
                        // `below="@id/moved"` becomes the moved view's own `below`.
                        Some(value) => doc.set_attr(sib, &rule.attr, &value),
                        None => {
                            doc.remove_attr(sib, &rule.attr);
                        }
                    }
                }
            }
        }
        _ => {}
    }
}

/// Inserts a copy right after the view, with fresh ids.
pub fn duplicate(doc: &mut Document, id: NodeId) -> Option<NodeId> {
    let parent = doc.parent(id)?;
    let index = doc.element_children(parent).iter().position(|&c| c == id)? + 1;
    let indent = doc.indent_of(id);
    let copy = doc.clone_subtree(id);
    doc.insert_element(copy, parent, index, &indent);
    for n in doc.descendants(copy) {
        if let Some(v) = doc.attr(n, "android:id")
            && let Some(name) = id_name(&v)
        {
            let fresh = unique_id(doc, name);
            doc.set_attr(n, "android:id", &format!("@+id/{fresh}"));
        }
    }
    Some(copy)
}

/// `title` -> `title2`, `button3` -> `button4`, skipping ids already used.
pub fn unique_id(doc: &Document, base: &str) -> String {
    let stem = base.trim_end_matches(|c: char| c.is_ascii_digit());
    let stem = if stem.is_empty() { base } else { stem };
    (2..)
        .map(|i| format!("{stem}{i}"))
        .find(|c| doc.find_by_android_id(c).is_none())
        .unwrap()
}

/// Views offered by the palette.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NewView {
    TextView,
    Button,
    EditText,
    ImageView,
    CheckBox,
    Switch,
    View,
    Space,
    VerticalLinearLayout,
    HorizontalLinearLayout,
    FrameLayout,
    ConstraintLayout,
}

impl NewView {
    pub const ALL: [NewView; 12] = [
        NewView::TextView,
        NewView::Button,
        NewView::EditText,
        NewView::ImageView,
        NewView::CheckBox,
        NewView::Switch,
        NewView::View,
        NewView::Space,
        NewView::VerticalLinearLayout,
        NewView::HorizontalLinearLayout,
        NewView::FrameLayout,
        NewView::ConstraintLayout,
    ];

    pub fn label(self) -> &'static str {
        match self {
            NewView::TextView => "TextView",
            NewView::Button => "Button",
            NewView::EditText => "EditText",
            NewView::ImageView => "ImageView",
            NewView::CheckBox => "CheckBox",
            NewView::Switch => "Switch",
            NewView::View => "View (box)",
            NewView::Space => "Space",
            NewView::VerticalLinearLayout => "LinearLayout (vertical)",
            NewView::HorizontalLinearLayout => "LinearLayout (horizontal)",
            NewView::FrameLayout => "FrameLayout",
            NewView::ConstraintLayout => "ConstraintLayout",
        }
    }

    fn tag(self) -> &'static str {
        match self {
            NewView::TextView => "TextView",
            NewView::Button => "Button",
            NewView::EditText => "EditText",
            NewView::ImageView => "ImageView",
            NewView::CheckBox => "CheckBox",
            NewView::Switch => "Switch",
            NewView::View => "View",
            NewView::Space => "Space",
            NewView::VerticalLinearLayout | NewView::HorizontalLinearLayout => "LinearLayout",
            NewView::FrameLayout => "FrameLayout",
            NewView::ConstraintLayout => "androidx.constraintlayout.widget.ConstraintLayout",
        }
    }
}

/// Adds a new view to `parent` at `index` (clamped to the end) and returns it.
pub fn insert_view(doc: &mut Document, parent: NodeId, index: usize, kind: NewView) -> NodeId {
    let in_constraint = classify(doc, parent).container() == Some(ContainerKind::Constraint);
    // match_parent is discouraged inside a ConstraintLayout.
    let full = if in_constraint {
        "200dp"
    } else {
        "match_parent"
    };
    let (w, h, extra): (&str, &str, &[(&str, &str)]) = match kind {
        NewView::TextView => (
            "wrap_content",
            "wrap_content",
            &[("android:text", "TextView")],
        ),
        NewView::Button => (
            "wrap_content",
            "wrap_content",
            &[("android:text", "Button")],
        ),
        NewView::EditText => (
            full,
            "wrap_content",
            &[
                ("android:hint", "Enter text"),
                ("android:inputType", "text"),
            ],
        ),
        NewView::ImageView => (
            "64dp",
            "64dp",
            &[
                ("android:src", "@android:drawable/ic_menu_gallery"),
                ("android:contentDescription", "Image"),
            ],
        ),
        NewView::CheckBox => (
            "wrap_content",
            "wrap_content",
            &[("android:text", "CheckBox")],
        ),
        NewView::Switch => (
            "wrap_content",
            "wrap_content",
            &[("android:text", "Switch")],
        ),
        NewView::View => ("64dp", "64dp", &[("android:background", "#CCCCCC")]),
        NewView::Space => ("16dp", "16dp", &[]),
        NewView::VerticalLinearLayout => (full, "120dp", &[("android:orientation", "vertical")]),
        NewView::HorizontalLinearLayout => {
            (full, "120dp", &[("android:orientation", "horizontal")])
        }
        NewView::FrameLayout | NewView::ConstraintLayout => (full, "120dp", &[]),
    };
    let tag = kind.tag();
    let short = short_name(tag);
    let base = format!("{}{}", short[..1].to_lowercase(), &short[1..]);
    let id_value = if doc.find_by_android_id(&base).is_none() {
        format!("@+id/{base}")
    } else {
        format!("@+id/{}", unique_id(doc, &base))
    };
    let mut attrs: Vec<(&str, &str)> = vec![
        ("android:id", id_value.as_str()),
        ("android:layout_width", w),
        ("android:layout_height", h),
    ];
    attrs.extend_from_slice(extra);
    if in_constraint {
        doc.ensure_namespace("app", APP_NS);
        attrs.push(("app:layout_constraintStart_toStartOf", "parent"));
        attrs.push(("app:layout_constraintTop_toTopOf", "parent"));
    }
    let node = doc.create_element(tag, &attrs);
    let count = doc.element_children(parent).len();
    doc.insert_element(node, parent, index.min(count), "");
    node
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::{ApproxTextMeasurer, layout};

    fn setup(xml: &str) -> (Document, Resources, LayoutResult) {
        let doc = Document::parse(xml).unwrap();
        let res = Resources::default();
        let l = layout(&doc, &res, &ApproxTextMeasurer, [360.0, 640.0], 2.0);
        (doc, res, l)
    }

    fn relayout(doc: &Document, res: &Resources) -> LayoutResult {
        layout(doc, res, &ApproxTextMeasurer, [360.0, 640.0], 2.0)
    }

    const FRAME: &str = r#"<FrameLayout xmlns:android="http://schemas.android.com/apk/res/android"
    android:layout_width="match_parent"
    android:layout_height="match_parent">

    <View
        android:id="@+id/box"
        android:layout_width="40dp"
        android:layout_height="40dp"
        android:layout_margin="8dp" />

    <View
        android:id="@+id/corner"
        android:layout_width="40dp"
        android:layout_height="40dp"
        android:layout_gravity="bottom|end" />
</FrameLayout>
"#;

    #[test]
    fn move_in_frame_layout_splits_shorthand_margin() {
        let (mut doc, res, l) = setup(FRAME);
        let box_ = doc.find_by_android_id("box").unwrap();
        move_by(&mut doc, &res, &l, box_, [12.0, 0.0]);
        let r = relayout(&doc, &res).rect(box_).unwrap();
        assert_eq!((r.x, r.y), (20.0, 8.0));
        assert_eq!(
            doc.attr(box_, "android:layout_marginStart").as_deref(),
            Some("20dp")
        );
        assert_eq!(
            doc.attr(box_, "android:layout_marginTop").as_deref(),
            Some("8dp")
        );
        assert!(!doc.has_attr(box_, "android:layout_margin"));
    }

    #[test]
    fn padding_keeps_button_defaults() {
        let (mut doc, res, _) =
            setup(r#"<FrameLayout><Button android:id="@+id/b" android:text="Go"/></FrameLayout>"#);
        let b = doc.find_by_android_id("b").unwrap();
        set_spacing(&mut doc, &res, 2.0, b, Spacing::Padding, 0, false, 24.0);
        assert_eq!(doc.attr(b, "android:paddingStart").as_deref(), Some("24dp"));
        assert_eq!(doc.attr(b, "android:paddingEnd").as_deref(), Some("16dp"));
        assert_eq!(doc.attr(b, "android:paddingTop").as_deref(), Some("10dp"));
    }

    #[test]
    fn move_end_aligned_view_uses_end_margins() {
        let (mut doc, res, l) = setup(FRAME);
        let corner = doc.find_by_android_id("corner").unwrap();
        move_by(&mut doc, &res, &l, corner, [-10.0, -20.0]);
        let r = relayout(&doc, &res).rect(corner).unwrap();
        assert_eq!((r.x, r.y), (310.0, 580.0));
        assert_eq!(
            doc.attr(corner, "android:layout_marginEnd").as_deref(),
            Some("10dp")
        );
        assert_eq!(
            doc.attr(corner, "android:layout_marginBottom").as_deref(),
            Some("20dp")
        );
    }

    #[test]
    fn resize_from_left_edge_keeps_right_edge() {
        let (mut doc, res, l) = setup(FRAME);
        let box_ = doc.find_by_android_id("box").unwrap();
        let r = l.rect(box_).unwrap();
        resize(
            &mut doc,
            &res,
            &l,
            box_,
            Rect::new(r.x - 4.0, r.y, r.w + 4.0, r.h + 10.0),
        );
        let after = relayout(&doc, &res).rect(box_).unwrap();
        assert_eq!(after, Rect::new(4.0, 8.0, 44.0, 50.0));
        assert_eq!(
            doc.attr(box_, "android:layout_width").as_deref(),
            Some("44dp")
        );
    }

    const CONSTRAINT: &str = r#"<androidx.constraintlayout.widget.ConstraintLayout xmlns:android="http://schemas.android.com/apk/res/android"
    xmlns:app="http://schemas.android.com/apk/res-auto"
    xmlns:tools="http://schemas.android.com/tools"
    android:layout_width="match_parent"
    android:layout_height="match_parent">

    <View
        android:id="@+id/centered"
        android:layout_width="60dp"
        android:layout_height="60dp"
        app:layout_constraintStart_toStartOf="parent"
        app:layout_constraintEnd_toEndOf="parent"
        app:layout_constraintTop_toTopOf="parent" />

    <View
        android:id="@+id/free"
        android:layout_width="20dp"
        android:layout_height="20dp"
        tools:layout_editor_absoluteX="50dp"
        tools:layout_editor_absoluteY="70dp" />
</androidx.constraintlayout.widget.ConstraintLayout>
"#;

    #[test]
    fn move_centered_constraint_view_changes_bias() {
        let (mut doc, res, l) = setup(CONSTRAINT);
        let v = doc.find_by_android_id("centered").unwrap();
        move_by(&mut doc, &res, &l, v, [-75.0, 30.0]);
        assert_eq!(
            doc.attr(v, "app:layout_constraintHorizontal_bias")
                .as_deref(),
            Some("0.25")
        );
        assert_eq!(
            doc.attr(v, "android:layout_marginTop").as_deref(),
            Some("30dp")
        );
        let r = relayout(&doc, &res).rect(v).unwrap();
        assert_eq!((r.x, r.y), (75.0, 30.0));
    }

    #[test]
    fn move_unconstrained_view_adds_constraints() {
        let (mut doc, res, l) = setup(CONSTRAINT);
        let v = doc.find_by_android_id("free").unwrap();
        move_by(&mut doc, &res, &l, v, [10.0, 0.0]);
        assert_eq!(
            doc.attr(v, "app:layout_constraintStart_toStartOf")
                .as_deref(),
            Some("parent")
        );
        assert_eq!(
            doc.attr(v, "android:layout_marginStart").as_deref(),
            Some("60dp")
        );
        assert!(!doc.has_attr(v, "tools:layout_editor_absoluteX"));
        let r = relayout(&doc, &res).rect(v).unwrap();
        assert_eq!((r.x, r.y), (60.0, 70.0));
    }

    #[test]
    fn align_in_constraint_layout() {
        let (mut doc, res, l) = setup(CONSTRAINT);
        let v = doc.find_by_android_id("centered").unwrap();
        align(&mut doc, &res, &l, v, 0, Align::End);
        assert!(!doc.has_attr(v, "app:layout_constraintStart_toStartOf"));
        assert_eq!(
            doc.attr(v, "app:layout_constraintEnd_toEndOf").as_deref(),
            Some("parent")
        );
        align(&mut doc, &res, &l, v, 1, Align::Fill);
        assert_eq!(doc.attr(v, "android:layout_height").as_deref(), Some("0dp"));
        let r = relayout(&doc, &res).rect(v).unwrap();
        assert_eq!(r, Rect::new(300.0, 0.0, 60.0, 640.0));
    }

    const LINEAR: &str = r#"<LinearLayout xmlns:android="http://schemas.android.com/apk/res/android"
    android:layout_width="match_parent"
    android:layout_height="match_parent"
    android:orientation="vertical">

    <TextView
        android:id="@+id/one"
        android:layout_width="wrap_content"
        android:layout_height="wrap_content"
        android:text="One" />

    <TextView
        android:id="@+id/two"
        android:layout_width="wrap_content"
        android:layout_height="wrap_content"
        android:text="Two" />

    <FrameLayout
        android:id="@+id/frame"
        android:layout_width="match_parent"
        android:layout_height="200dp" />
</LinearLayout>
"#;

    #[test]
    fn reorder_and_align_in_linear_layout() {
        let (mut doc, res, l) = setup(LINEAR);
        let root = doc.root().unwrap();
        let two = doc.find_by_android_id("two").unwrap();
        let idx = drop_index(&doc, &l, root, two, [10.0, 1.0]);
        assert_eq!(idx, 0);
        reorder(&mut doc, two, idx);
        assert!(doc.to_xml().find("@+id/two").unwrap() < doc.to_xml().find("@+id/one").unwrap());
        align(&mut doc, &res, &l, two, 0, Align::Center);
        assert_eq!(
            doc.attr(two, "android:layout_gravity").as_deref(),
            Some("center_horizontal")
        );
        align(&mut doc, &res, &l, two, 1, Align::Center);
        assert_eq!(
            doc.attr(root, "android:gravity").as_deref(),
            Some("center_vertical")
        );
    }

    #[test]
    fn reparent_into_frame_places_at_drop_point() {
        let (mut doc, res, l) = setup(LINEAR);
        let one = doc.find_by_android_id("one").unwrap();
        let frame = doc.find_by_android_id("frame").unwrap();
        let frame_rect = l.rect(frame).unwrap();
        reparent(
            &mut doc,
            &res,
            &l,
            one,
            frame,
            0,
            [30.0, frame_rect.y + 40.0],
        );
        assert_eq!(doc.parent(one), Some(frame));
        let xml = doc.to_xml();
        assert!(xml.contains(
            "        android:layout_height=\"200dp\" >\n        <TextView\n            android:id=\"@+id/one\""
        ), "{xml}");
        let r = relayout(&doc, &res).rect(one).unwrap();
        let fr = relayout(&doc, &res).rect(frame).unwrap();
        assert_eq!((r.x, r.y - fr.y), (30.0, 40.0));
        assert_eq!(Document::parse(&xml).unwrap().to_xml(), xml);
    }

    #[test]
    fn deleting_keeps_dependent_views_attached() {
        let xml = r#"<androidx.constraintlayout.widget.ConstraintLayout android:layout_width="match_parent" android:layout_height="match_parent">
    <View android:id="@+id/a" android:layout_width="10dp" android:layout_height="40dp"
        app:layout_constraintTop_toTopOf="parent" app:layout_constraintStart_toStartOf="parent"/>
    <View android:id="@+id/b" android:layout_width="10dp" android:layout_height="20dp"
        app:layout_constraintTop_toBottomOf="@id/a" app:layout_constraintStart_toStartOf="parent"/>
    <View android:id="@+id/c" android:layout_width="10dp" android:layout_height="20dp"
        app:layout_constraintTop_toBottomOf="@id/b" app:layout_constraintStart_toEndOf="@id/b"/>
</androidx.constraintlayout.widget.ConstraintLayout>"#;
        let (mut doc, res, _) = setup(xml);
        let b = doc.find_by_android_id("b").unwrap();
        let c = doc.find_by_android_id("c").unwrap();
        assert!(delete(&mut doc, b));
        assert_eq!(
            doc.attr(c, "app:layout_constraintTop_toBottomOf")
                .as_deref(),
            Some("@id/a")
        );
        assert_eq!(
            doc.attr(c, "app:layout_constraintStart_toStartOf")
                .as_deref(),
            Some("parent")
        );
        assert!(!doc.has_attr(c, "app:layout_constraintStart_toEndOf"));
        assert_eq!(relayout(&doc, &res).rect(c).unwrap().y, 40.0);
    }

    #[test]
    fn deleting_in_relative_layout_bridges_rules() {
        let xml = r#"<RelativeLayout android:layout_width="match_parent" android:layout_height="match_parent">
    <View android:id="@+id/a" android:layout_width="10dp" android:layout_height="40dp"/>
    <View android:id="@+id/b" android:layout_width="10dp" android:layout_height="20dp" android:layout_below="@id/a"/>
    <View android:id="@+id/c" android:layout_width="10dp" android:layout_height="20dp" android:layout_below="@id/b"
        android:layout_toEndOf="@id/b"/>
</RelativeLayout>"#;
        let (mut doc, _, _) = setup(xml);
        let b = doc.find_by_android_id("b").unwrap();
        let c = doc.find_by_android_id("c").unwrap();
        delete(&mut doc, b);
        assert_eq!(
            doc.attr(c, "android:layout_below").as_deref(),
            Some("@id/a")
        );
        assert!(!doc.has_attr(c, "android:layout_toEndOf"));
    }

    #[test]
    fn duplicate_gets_fresh_id() {
        let (mut doc, _, _) = setup(LINEAR);
        let one = doc.find_by_android_id("one").unwrap();
        let copy = duplicate(&mut doc, one).unwrap();
        assert_eq!(doc.attr(copy, "android:id").as_deref(), Some("@+id/one2"));
        let root = doc.root().unwrap();
        assert_eq!(doc.element_children(root)[1], copy);
    }

    #[test]
    fn insert_into_constraint_layout_adds_constraints() {
        let (mut doc, _, _) = setup(CONSTRAINT);
        let root = doc.root().unwrap();
        let b = insert_view(&mut doc, root, usize::MAX, NewView::Button);
        assert_eq!(doc.attr(b, "android:id").as_deref(), Some("@+id/button"));
        assert_eq!(
            doc.attr(b, "app:layout_constraintTop_toTopOf").as_deref(),
            Some("parent")
        );
        let b2 = insert_view(&mut doc, root, 0, NewView::Button);
        assert_eq!(doc.attr(b2, "android:id").as_deref(), Some("@+id/button2"));
        let xml = doc.to_xml();
        assert_eq!(Document::parse(&xml).unwrap().to_xml(), xml);
    }
}
