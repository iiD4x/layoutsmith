//! A simplified version of Android's measure and layout passes.
//!
//! It is not pixel-exact (fonts, drawables and themes are approximated), but it
//! follows the same rules as the framework for the common containers so the
//! preview responds to edits the way a device would.

use std::collections::HashMap;

use crate::resources::Resources;
use crate::units::{Align, Color, Dim, Gravity};
use crate::widgets::{ContainerKind, ViewKind, classify, short_name};
use crate::xml::{Document, NodeId, id_name};

/// Pixels per dp used for `px` values (a 420 dpi phone).
pub const DEFAULT_DENSITY: f32 = 2.625;

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Rect {
    pub const fn new(x: f32, y: f32, w: f32, h: f32) -> Self {
        Self { x, y, w, h }
    }

    pub fn right(&self) -> f32 {
        self.x + self.w
    }

    pub fn bottom(&self) -> f32 {
        self.y + self.h
    }

    pub fn contains(&self, px: f32, py: f32) -> bool {
        px >= self.x && px <= self.right() && py >= self.y && py <= self.bottom()
    }

    /// Start position along axis 0 (x) or 1 (y).
    pub fn pos(&self, axis: usize) -> f32 {
        if axis == 0 { self.x } else { self.y }
    }

    /// Length along axis 0 (width) or 1 (height).
    pub fn len(&self, axis: usize) -> f32 {
        if axis == 0 { self.w } else { self.h }
    }
}

/// Android's `MeasureSpec`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Spec {
    Exactly(f32),
    AtMost(f32),
    Unspecified,
}

pub trait TextMeasurer {
    /// Size of `text` at `size` dp, wrapped at `max_width` when given.
    fn measure(&self, text: &str, size: f32, bold: bool, max_width: Option<f32>) -> [f32; 2];
}

/// Character-count based text measurement, close enough for Roboto.
pub struct ApproxTextMeasurer;

impl TextMeasurer for ApproxTextMeasurer {
    fn measure(&self, text: &str, size: f32, bold: bool, max_width: Option<f32>) -> [f32; 2] {
        let char_w = size * if bold { 0.58 } else { 0.53 };
        let line_h = size * 1.17;
        let mut width = 0f32;
        let mut lines = 0usize;
        for line in text.split('\n') {
            let w = line.chars().count() as f32 * char_w;
            match max_width {
                Some(max) if w > max && max > char_w => {
                    lines += (w / max).ceil() as usize;
                    width = width.max(max);
                }
                _ => {
                    lines += 1;
                    width = width.max(w);
                }
            }
        }
        [width.ceil(), lines.max(1) as f32 * line_h]
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Visibility {
    Visible,
    Invisible,
    Gone,
}

/// The layout-relevant attributes of a view, resolved to dp.
#[derive(Debug, Clone, PartialEq)]
pub struct Params {
    pub size: [Dim; 2],
    /// Left and top margins.
    pub margin_lo: [f32; 2],
    /// Right and bottom margins.
    pub margin_hi: [f32; 2],
    pub padding_lo: [f32; 2],
    pub padding_hi: [f32; 2],
    pub min: [f32; 2],
    pub weight: f32,
    pub layout_gravity: Gravity,
    pub gravity: Gravity,
    pub visibility: Visibility,
}

impl Params {
    pub fn margins(&self, axis: usize) -> f32 {
        self.margin_lo[axis] + self.margin_hi[axis]
    }

    pub fn padding(&self, axis: usize) -> f32 {
        self.padding_lo[axis] + self.padding_hi[axis]
    }
}

/// Reads a view's attributes, falling back to its `style`.
pub struct Attrs<'a> {
    pub doc: &'a Document,
    pub res: &'a Resources,
    pub id: NodeId,
}

impl Attrs<'_> {
    pub fn get(&self, name: &str) -> Option<String> {
        self.doc.attr(self.id, name).or_else(|| {
            let style = self.doc.attr(self.id, "style")?;
            self.res.style_item(&style, name)
        })
    }

    pub fn dimension(&self, name: &str, density: f32) -> Option<f32> {
        self.res.resolve_dimension(&self.get(name)?, density)
    }

    pub fn color(&self, name: &str) -> Option<Color> {
        self.res.resolve_color(&self.get(name)?)
    }
}

pub fn params(doc: &Document, res: &Resources, id: NodeId, density: f32) -> Params {
    let a = Attrs { doc, res, id };
    let dim = |name: &str| -> Option<Dim> {
        Some(match a.get(name)?.trim() {
            "match_parent" | "fill_parent" => Dim::Match,
            "wrap_content" => Dim::Wrap,
            other => Dim::Dp(res.resolve_dimension(other, density)?),
        })
    };
    // A <merge> root or a root without sizes fills the screen.
    let default_dim = if doc.parent(id).is_none() {
        Dim::Match
    } else {
        Dim::Wrap
    };
    let d = |name: &str| a.dimension(name, density);
    let sides = |prefix: &str| {
        let mut lo = [
            d(&format!("{prefix}Left")).unwrap_or(0.0),
            d(&format!("{prefix}Top")).unwrap_or(0.0),
        ];
        let mut hi = [
            d(&format!("{prefix}Right")).unwrap_or(0.0),
            d(&format!("{prefix}Bottom")).unwrap_or(0.0),
        ];
        if let Some(v) = d(&format!("{prefix}Start")) {
            lo[0] = v;
        }
        if let Some(v) = d(&format!("{prefix}End")) {
            hi[0] = v;
        }
        if let Some(v) = d(&format!("{prefix}Horizontal")) {
            lo[0] = v;
            hi[0] = v;
        }
        if let Some(v) = d(&format!("{prefix}Vertical")) {
            lo[1] = v;
            hi[1] = v;
        }
        if let Some(v) = d(prefix) {
            lo = [v, v];
            hi = [v, v];
        }
        (lo, hi)
    };

    let kind = classify(doc, id);
    let (margin_lo, margin_hi) = sides("android:layout_margin");
    let (mut padding_lo, mut padding_hi) = sides("android:padding");
    let has_padding = doc.element(id).is_some_and(|e| {
        e.attrs
            .iter()
            .any(|at| at.name.starts_with("android:padding"))
    });
    let mut min = [
        d("android:minWidth").unwrap_or(0.0),
        d("android:minHeight").unwrap_or(0.0),
    ];
    // Framework defaults that make buttons and fields look like themselves.
    match kind {
        ViewKind::Button => {
            if !has_padding {
                padding_lo = [16.0, 10.0];
                padding_hi = [16.0, 10.0];
            }
            min = [min[0].max(64.0), min[1].max(48.0)];
        }
        ViewKind::EditText => {
            if !has_padding {
                padding_lo = [4.0, 10.0];
                padding_hi = [4.0, 10.0];
            }
            min[1] = min[1].max(48.0);
        }
        ViewKind::Check => min[1] = min[1].max(48.0),
        _ => {}
    }

    let visibility = match a
        .get("tools:visibility")
        .or_else(|| a.get("android:visibility"))
        .as_deref()
    {
        Some("gone") => Visibility::Gone,
        Some("invisible") => Visibility::Invisible,
        _ => Visibility::Visible,
    };

    Params {
        size: [
            dim("android:layout_width").unwrap_or(default_dim),
            dim("android:layout_height").unwrap_or(default_dim),
        ],
        margin_lo,
        margin_hi,
        padding_lo,
        padding_hi,
        min,
        weight: a
            .get("android:layout_weight")
            .and_then(|w| w.trim().parse().ok())
            .unwrap_or(0.0),
        layout_gravity: Gravity::parse(&a.get("android:layout_gravity").unwrap_or_default()),
        gravity: Gravity::parse(&a.get("android:gravity").unwrap_or_default()),
        visibility,
    }
}

/// Text content and appearance of a text-showing view.
#[derive(Debug, Clone, PartialEq)]
pub struct TextStyle {
    pub text: String,
    pub is_hint: bool,
    pub size: f32,
    pub bold: bool,
    pub italic: bool,
    pub color: Option<Color>,
}

pub fn text_style(doc: &Document, res: &Resources, id: NodeId, density: f32) -> TextStyle {
    let a = Attrs { doc, res, id };
    let get = |name: &str| {
        a.get(name).or_else(|| {
            let appearance = a.get("android:textAppearance")?;
            res.style_item(&appearance, name)
        })
    };
    let mut is_hint = false;
    let mut text = get("tools:text")
        .or_else(|| get("android:text"))
        .map(|t| res.resolve_string(&t))
        .unwrap_or_default();
    if text.is_empty()
        && let Some(hint) = get("tools:hint").or_else(|| get("android:hint"))
    {
        text = res.resolve_string(&hint);
        is_hint = true;
    }
    if get("android:textAllCaps").as_deref() == Some("true") {
        text = text.to_uppercase();
    }
    let style = get("android:textStyle").unwrap_or_default();
    TextStyle {
        text,
        is_hint,
        size: get("android:textSize")
            .and_then(|v| res.resolve_dimension(&v, density))
            .unwrap_or(14.0),
        bold: style.contains("bold"),
        italic: style.contains("italic"),
        color: get(if is_hint {
            "android:textColorHint"
        } else {
            "android:textColor"
        })
        .and_then(|v| res.resolve_color(&v)),
    }
}

/// Where a ConstraintLayout or RelativeLayout child is attached, in absolute
/// dp, before margins. `None` means that side is free.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Anchors {
    pub lo: [Option<f32>; 2],
    pub hi: [Option<f32>; 2],
}

impl Anchors {
    fn offset(self, by: [f32; 2]) -> Self {
        Self {
            lo: [self.lo[0].map(|v| v + by[0]), self.lo[1].map(|v| v + by[1])],
            hi: [self.hi[0].map(|v| v + by[0]), self.hi[1].map(|v| v + by[1])],
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct LayoutResult {
    /// Absolute position of every laid-out view, in dp.
    pub rects: HashMap<NodeId, Rect>,
    /// Views in paint order (parents before children).
    pub order: Vec<NodeId>,
    pub anchors: HashMap<NodeId, Anchors>,
    pub density: f32,
}

impl LayoutResult {
    pub fn rect(&self, id: NodeId) -> Option<Rect> {
        self.rects.get(&id).copied()
    }

    /// The topmost view under a point, ignoring `exclude` and its subtree.
    pub fn hit_test(
        &self,
        doc: &Document,
        x: f32,
        y: f32,
        exclude: Option<NodeId>,
    ) -> Option<NodeId> {
        self.order.iter().rev().copied().find(|&id| {
            exclude.is_none_or(|e| !doc.is_ancestor_or_self(e, id))
                && self.rects[&id].contains(x, y)
        })
    }
}

/// Lays out the document on a screen of `screen` dp.
pub fn layout(
    doc: &Document,
    res: &Resources,
    text: &dyn TextMeasurer,
    screen: [f32; 2],
    density: f32,
) -> LayoutResult {
    let mut engine = Engine {
        doc,
        res,
        text,
        density,
        params: HashMap::new(),
        measured: HashMap::new(),
        local: HashMap::new(),
        local_anchors: HashMap::new(),
        result: LayoutResult {
            density,
            ..LayoutResult::default()
        },
    };
    if let Some(root) = doc.root() {
        let p = engine.params(root);
        let spec = [0, 1].map(|a| child_spec(Spec::Exactly(screen[a]), p.margins(a), p.size[a]));
        engine.measure(root, spec);
        if p.visibility != Visibility::Gone {
            engine.place(root, p.margin_lo[0], p.margin_lo[1]);
        }
    }
    engine.result
}

/// Android's `getChildMeasureSpec`.
pub fn child_spec(parent: Spec, used: f32, dim: Dim) -> Spec {
    let avail = |s: f32| (s - used).max(0.0);
    match dim {
        Dim::Dp(d) => Spec::Exactly(d.max(0.0)),
        Dim::Match => match parent {
            Spec::Exactly(s) => Spec::Exactly(avail(s)),
            Spec::AtMost(s) => Spec::AtMost(avail(s)),
            Spec::Unspecified => Spec::Unspecified,
        },
        Dim::Wrap => match parent {
            Spec::Exactly(s) | Spec::AtMost(s) => Spec::AtMost(avail(s)),
            Spec::Unspecified => Spec::Unspecified,
        },
    }
}

fn resolve(desired: f32, spec: Spec) -> f32 {
    match spec {
        Spec::Exactly(s) => s,
        Spec::AtMost(s) => desired.min(s),
        Spec::Unspecified => desired,
    }
}

fn align_pos(align: Align, lo: f32, hi: f32, len: f32, m_lo: f32, m_hi: f32) -> f32 {
    match align {
        Align::Start | Align::Fill => lo + m_lo,
        Align::End => hi - m_hi - len,
        Align::Center => lo + (hi - lo - len) / 2.0 + m_lo - m_hi,
    }
}

/// One positioning rule of a RelativeLayout or ConstraintLayout child.
#[derive(Debug, Clone, PartialEq)]
pub struct Rule {
    pub attr: String,
    pub axis: usize,
    /// Constrains the child's right/bottom edge (otherwise its left/top edge).
    pub hi_side: bool,
    pub target: Target,
    /// Attaches to the target's right/bottom edge.
    pub target_hi: bool,
    /// RelativeLayout's `toRightOf`-style rules also add the target's margin.
    pub outer: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Target {
    Parent,
    Sibling(String),
    /// `centerInParent` / `centerHorizontal` / `centerVertical`.
    Center,
}

fn target_of(value: &str) -> Option<Target> {
    if value == "parent" {
        Some(Target::Parent)
    } else {
        id_name(value).map(|n| Target::Sibling(n.to_string()))
    }
}

/// The positioning rules of a RelativeLayout child.
pub fn relative_rules(doc: &Document, id: NodeId) -> Vec<Rule> {
    let mut rules = Vec::new();
    let Some(e) = doc.element(id) else {
        return rules;
    };
    for attr in &e.attrs {
        let Some(name) = attr.name.strip_prefix("android:layout_") else {
            continue;
        };
        let value = attr.value();
        let is_true = value == "true";
        // (axis, hi_side, target_hi, outer)
        let sibling = match name {
            "toRightOf" | "toEndOf" => Some((0, false, true, true)),
            "toLeftOf" | "toStartOf" => Some((0, true, false, true)),
            "below" => Some((1, false, true, true)),
            "above" => Some((1, true, false, true)),
            "alignLeft" | "alignStart" => Some((0, false, false, false)),
            "alignRight" | "alignEnd" => Some((0, true, true, false)),
            "alignTop" | "alignBaseline" => Some((1, false, false, false)),
            "alignBottom" => Some((1, true, true, false)),
            _ => None,
        };
        let parent = match name {
            "alignParentLeft" | "alignParentStart" => Some((0, false)),
            "alignParentRight" | "alignParentEnd" => Some((0, true)),
            "alignParentTop" => Some((1, false)),
            "alignParentBottom" => Some((1, true)),
            _ => None,
        };
        let rule = |axis, hi_side, target, target_hi, outer| Rule {
            attr: attr.name.clone(),
            axis,
            hi_side,
            target,
            target_hi,
            outer,
        };
        if let Some((axis, hi_side, target_hi, outer)) = sibling {
            if let Some(target) = target_of(&value) {
                rules.push(rule(axis, hi_side, target, target_hi, outer));
            }
        } else if let Some((axis, hi)) = parent {
            if is_true {
                rules.push(rule(axis, hi, Target::Parent, hi, false));
            }
        } else if is_true {
            match name {
                "centerInParent" => {
                    rules.push(rule(0, false, Target::Center, false, false));
                    rules.push(rule(1, false, Target::Center, false, false));
                }
                "centerHorizontal" => rules.push(rule(0, false, Target::Center, false, false)),
                "centerVertical" => rules.push(rule(1, false, Target::Center, false, false)),
                _ => {}
            }
        }
    }
    rules
}

/// Side keyword of a constraint attribute: (axis, is right/bottom side).
fn constraint_side(s: &str) -> Option<(usize, bool)> {
    Some(match s {
        "Left" | "Start" => (0, false),
        "Right" | "End" => (0, true),
        "Top" | "Baseline" => (1, false),
        "Bottom" => (1, true),
        _ => return None,
    })
}

/// The constraints of a ConstraintLayout child, e.g.
/// `app:layout_constraintStart_toEndOf="@id/title"`.
pub fn constraint_rules(doc: &Document, id: NodeId) -> Vec<Rule> {
    let mut rules = Vec::new();
    let Some(e) = doc.element(id) else {
        return rules;
    };
    for attr in &e.attrs {
        let Some(rest) = attr
            .name
            .strip_prefix("app:layout_constraint")
            .and_then(|r| r.strip_suffix("Of"))
        else {
            continue;
        };
        let Some((side, target_side)) = rest.split_once("_to") else {
            continue;
        };
        let (Some((axis, hi_side)), Some((target_axis, target_hi))) =
            (constraint_side(side), constraint_side(target_side))
        else {
            continue;
        };
        if axis != target_axis {
            continue;
        }
        if let Some(target) = target_of(&attr.value()) {
            rules.push(Rule {
                attr: attr.name.clone(),
                axis,
                hi_side,
                target,
                target_hi,
                outer: false,
            });
        }
    }
    rules
}

pub fn bias_attr(axis: usize) -> &'static str {
    if axis == 0 {
        "app:layout_constraintHorizontal_bias"
    } else {
        "app:layout_constraintVertical_bias"
    }
}

struct Engine<'a> {
    doc: &'a Document,
    res: &'a Resources,
    text: &'a dyn TextMeasurer,
    density: f32,
    params: HashMap<NodeId, Params>,
    measured: HashMap<NodeId, [f32; 2]>,
    /// Child position relative to its parent, from the parent's last measure.
    local: HashMap<NodeId, [f32; 2]>,
    local_anchors: HashMap<NodeId, Anchors>,
    result: LayoutResult,
}

impl Engine<'_> {
    fn params(&mut self, id: NodeId) -> Params {
        self.params
            .entry(id)
            .or_insert_with(|| params(self.doc, self.res, id, self.density))
            .clone()
    }

    fn visible_children(&mut self, id: NodeId) -> Vec<NodeId> {
        let kids = self.doc.element_children(id);
        kids.into_iter()
            .filter(|&c| self.params(c).visibility != Visibility::Gone)
            .collect()
    }

    fn measure(&mut self, id: NodeId, spec: [Spec; 2]) -> [f32; 2] {
        let p = self.params(id);
        let size = if p.visibility == Visibility::Gone {
            [0.0, 0.0]
        } else {
            match classify(self.doc, id) {
                ViewKind::Container(kind) => self.measure_container(id, kind, &p, spec),
                kind => self.measure_leaf(id, kind, &p, spec),
            }
        };
        self.measured.insert(id, size);
        size
    }

    fn measure_leaf(
        &mut self,
        id: NodeId,
        kind: ViewKind,
        p: &Params,
        spec: [Spec; 2],
    ) -> [f32; 2] {
        let pad = [p.padding(0), p.padding(1)];
        let content = match kind {
            k if k.shows_text() => {
                let style = text_style(self.doc, self.res, id, self.density);
                let indicator = if kind == ViewKind::Check { 32.0 } else { 0.0 };
                let max_w = match spec[0] {
                    Spec::Exactly(s) | Spec::AtMost(s) => Some((s - pad[0] - indicator).max(1.0)),
                    Spec::Unspecified => None,
                };
                let [w, h] = self
                    .text
                    .measure(&style.text, style.size, style.bold, max_w);
                [w + indicator, h.max(style.size * 1.17)]
            }
            ViewKind::Image => {
                let s = if short_name(self.doc.name(id)) == "FloatingActionButton" {
                    56.0
                } else {
                    48.0
                };
                [s, s]
            }
            ViewKind::Progress | ViewKind::Other => [48.0, 48.0],
            ViewKind::Include => [160.0, 48.0],
            _ => [0.0, 0.0],
        };
        [0, 1].map(|a| resolve((content[a] + pad[a]).max(p.min[a]), spec[a]))
    }

    fn measure_container(
        &mut self,
        id: NodeId,
        kind: ContainerKind,
        p: &Params,
        spec: [Spec; 2],
    ) -> [f32; 2] {
        match kind {
            ContainerKind::Frame => self.measure_frame(id, p, spec, None),
            ContainerKind::Scroll { vertical } => {
                self.measure_frame(id, p, spec, Some(vertical as usize))
            }
            ContainerKind::Linear { vertical } => {
                self.measure_linear(id, p, spec, vertical as usize)
            }
            ContainerKind::Relative => self.measure_rules(id, p, spec, false),
            ContainerKind::Constraint => self.measure_rules(id, p, spec, true),
        }
    }

    /// FrameLayout, and ScrollViews when `scroll` names the scrolling axis.
    fn measure_frame(
        &mut self,
        id: NodeId,
        p: &Params,
        spec: [Spec; 2],
        scroll: Option<usize>,
    ) -> [f32; 2] {
        let pad = [p.padding(0), p.padding(1)];
        let kids = self.visible_children(id);
        let spec_for = |a: usize, cp: &Params, parent: [Spec; 2]| {
            let used = pad[a] + cp.margins(a);
            if Some(a) == scroll {
                let dim = if cp.size[a] == Dim::Match {
                    Dim::Wrap
                } else {
                    cp.size[a]
                };
                child_spec(Spec::Unspecified, used, dim)
            } else {
                child_spec(parent[a], used, cp.size[a])
            }
        };

        let mut content = [0f32; 2];
        for &c in &kids {
            let cp = self.params(c);
            let m = self.measure(c, [spec_for(0, &cp, spec), spec_for(1, &cp, spec)]);
            for a in 0..2 {
                content[a] = content[a].max(m[a] + cp.margins(a));
            }
        }
        let size = [0, 1].map(|a| resolve((content[a] + pad[a]).max(p.min[a]), spec[a]));

        // Children that match the parent get its final size.
        let exact = [Spec::Exactly(size[0]), Spec::Exactly(size[1])];
        for &c in &kids {
            let cp = self.params(c);
            let m = self.measured[&c];
            let stale = (0..2).any(|a| {
                cp.size[a] == Dim::Match
                    && Some(a) != scroll
                    && m[a] != (size[a] - pad[a] - cp.margins(a)).max(0.0)
            });
            if stale {
                self.measure(c, [spec_for(0, &cp, exact), spec_for(1, &cp, exact)]);
            }
        }

        for &c in &kids {
            let cp = self.params(c);
            let m = self.measured[&c];
            let pos = [0, 1].map(|a| {
                align_pos(
                    cp.layout_gravity.axis(a).unwrap_or(Align::Start),
                    p.padding_lo[a],
                    size[a] - p.padding_hi[a],
                    m[a],
                    cp.margin_lo[a],
                    cp.margin_hi[a],
                )
            });
            self.local.insert(c, pos);
        }
        size
    }

    fn measure_linear(&mut self, id: NodeId, p: &Params, spec: [Spec; 2], main: usize) -> [f32; 2] {
        let cross = 1 - main;
        let pad = [p.padding(0), p.padding(1)];
        let kids = self.visible_children(id);
        let weight_sum = Attrs {
            doc: self.doc,
            res: self.res,
            id,
        }
        .get("android:weightSum")
        .and_then(|v| v.trim().parse::<f32>().ok())
        .filter(|&w| w > 0.0);
        let distribute = matches!(spec[main], Spec::Exactly(_));

        let mut used = 0f32;
        let mut cross_max = 0f32;
        // (child, measured only once the free space is known)
        let mut weighted: Vec<(NodeId, bool)> = Vec::new();
        for &c in &kids {
            let cp = self.params(c);
            let weight_only = cp.weight > 0.0 && cp.size[main].is_zero();
            if weight_only && distribute {
                used += cp.margins(main);
                weighted.push((c, true));
                continue;
            }
            let mut s = [Spec::Unspecified; 2];
            let main_dim = if weight_only {
                Dim::Wrap
            } else {
                cp.size[main]
            };
            s[main] = child_spec(spec[main], pad[main] + used + cp.margins(main), main_dim);
            s[cross] = child_spec(spec[cross], pad[cross] + cp.margins(cross), cp.size[cross]);
            let m = self.measure(c, s);
            used += m[main] + cp.margins(main);
            cross_max = cross_max.max(m[cross] + cp.margins(cross));
            if cp.weight > 0.0 {
                weighted.push((c, false));
            }
        }

        if let (true, Spec::Exactly(total)) = (!weighted.is_empty(), spec[main]) {
            let total_weight = match weight_sum {
                Some(w) => w,
                None => weighted.iter().map(|&(c, _)| self.params(c).weight).sum(),
            };
            let free = (total - pad[main] - used).max(0.0);
            for &(c, deferred) in &weighted {
                let cp = self.params(c);
                let base = if deferred {
                    0.0
                } else {
                    self.measured[&c][main]
                };
                let len = (base + free * cp.weight / total_weight).max(0.0);
                let mut s = [Spec::Unspecified; 2];
                s[main] = Spec::Exactly(len);
                s[cross] = child_spec(spec[cross], pad[cross] + cp.margins(cross), cp.size[cross]);
                let m = self.measure(c, s);
                used += len - base;
                cross_max = cross_max.max(m[cross] + cp.margins(cross));
            }
        }

        let mut size = [0f32; 2];
        size[main] = resolve((used + pad[main]).max(p.min[main]), spec[main]);
        size[cross] = resolve((cross_max + pad[cross]).max(p.min[cross]), spec[cross]);

        // Children that match the parent across the main axis get its final size.
        for &c in &kids {
            let cp = self.params(c);
            let target = (size[cross] - pad[cross] - cp.margins(cross)).max(0.0);
            if cp.size[cross] == Dim::Match && self.measured[&c][cross] != target {
                let mut s = [Spec::Unspecified; 2];
                s[main] = Spec::Exactly(self.measured[&c][main]);
                s[cross] = Spec::Exactly(target);
                self.measure(c, s);
            }
        }

        let mut content_main = 0f32;
        for &c in &kids {
            content_main += self.measured[&c][main] + self.params(c).margins(main);
        }
        let (lo, hi) = (p.padding_lo[main], size[main] - p.padding_hi[main]);
        let mut cursor = match p.gravity.axis(main) {
            Some(Align::Center) => lo + (hi - lo - content_main) / 2.0,
            Some(Align::End) => hi - content_main,
            _ => lo,
        };
        for &c in &kids {
            let cp = self.params(c);
            let m = self.measured[&c];
            let mut pos = [0f32; 2];
            cursor += cp.margin_lo[main];
            pos[main] = cursor;
            cursor += m[main] + cp.margin_hi[main];
            let align = cp
                .layout_gravity
                .axis(cross)
                .or(p.gravity.axis(cross))
                .unwrap_or(Align::Start);
            pos[cross] = align_pos(
                align,
                p.padding_lo[cross],
                size[cross] - p.padding_hi[cross],
                m[cross],
                cp.margin_lo[cross],
                cp.margin_hi[cross],
            );
            self.local.insert(c, pos);
        }
        size
    }

    /// RelativeLayout and ConstraintLayout: children are positioned by rules,
    /// so the container's own size depends on where they end up.
    fn measure_rules(
        &mut self,
        id: NodeId,
        p: &Params,
        spec: [Spec; 2],
        constraint: bool,
    ) -> [f32; 2] {
        let unbounded = [0, 1].map(|a| spec[a] == Spec::Unspecified);
        let provisional = [0, 1].map(|a| match spec[a] {
            Spec::Exactly(s) | Spec::AtMost(s) => s,
            Spec::Unspecified => 0.0,
        });
        // Like the framework, size a wrap_content container from its content
        // first (children centered or pinned to the far edge sit at the start
        // while measuring), then place everything for the final size.
        let measuring = [0, 1].map(|a| !matches!(spec[a], Spec::Exactly(_)));
        let extent = self.arrange_by_rules(id, p, provisional, unbounded, measuring, constraint);
        let size = [0, 1].map(|a| match spec[a] {
            Spec::Exactly(s) => s,
            s => resolve(extent[a].max(p.min[a]), s),
        });
        if size != provisional || measuring.contains(&true) {
            self.arrange_by_rules(id, p, size, [false; 2], [false; 2], constraint);
        }
        size
    }

    /// Places the children of a rule-based container of `size` and returns the
    /// size their content needs.
    fn arrange_by_rules(
        &mut self,
        id: NodeId,
        p: &Params,
        size: [f32; 2],
        unbounded: [bool; 2],
        measuring: [bool; 2],
        constraint: bool,
    ) -> [f32; 2] {
        let kids = self.visible_children(id);
        let names: HashMap<String, NodeId> = kids
            .iter()
            .filter_map(|&c| {
                let v = self.doc.attr(c, "android:id")?;
                Some((id_name(&v)?.to_string(), c))
            })
            .collect();

        // Rules may point at siblings declared later, so place children in
        // dependency order. A cycle (e.g. a ConstraintLayout chain) is broken
        // by placing its first member while ignoring unplaced targets.
        let mut placed: HashMap<NodeId, Rect> = HashMap::new();
        let mut pending = kids.clone();
        let mut relax_first = false;
        while !pending.is_empty() {
            let mut progressed = false;
            let mut i = 0;
            while i < pending.len() {
                let c = pending[i];
                let relax = relax_first && i == 0;
                match self.place_by_rules(
                    c, p, size, unbounded, measuring, constraint, &names, &placed, relax,
                ) {
                    Some(r) => {
                        placed.insert(c, r);
                        pending.remove(i);
                        progressed = true;
                        relax_first = false;
                    }
                    None => i += 1,
                }
            }
            if !progressed {
                relax_first = true;
            }
        }

        let mut extent = [p.padding(0), p.padding(1)];
        for &c in &kids {
            let r = placed[&c];
            let cp = self.params(c);
            extent[0] = extent[0].max(r.right() + cp.margin_hi[0] + p.padding_hi[0]);
            extent[1] = extent[1].max(r.bottom() + cp.margin_hi[1] + p.padding_hi[1]);
        }
        extent
    }

    #[allow(clippy::too_many_arguments)]
    fn place_by_rules(
        &mut self,
        c: NodeId,
        parent: &Params,
        size: [f32; 2],
        unbounded: [bool; 2],
        measuring: [bool; 2],
        constraint: bool,
        names: &HashMap<String, NodeId>,
        placed: &HashMap<NodeId, Rect>,
        relax: bool,
    ) -> Option<Rect> {
        let cp = self.params(c);
        let edges = [0, 1].map(|a| (parent.padding_lo[a], size[a] - parent.padding_hi[a]));
        let rules = if constraint {
            constraint_rules(self.doc, c)
        } else {
            relative_rules(self.doc, c)
        };

        let mut anchors = Anchors::default();
        let mut centered = [false; 2];
        for rule in rules {
            let a = rule.axis;
            let line = match &rule.target {
                Target::Center if measuring[a] => continue,
                Target::Parent if measuring[a] && rule.target_hi => continue,
                Target::Center => {
                    centered[a] = true;
                    continue;
                }
                Target::Parent => {
                    if rule.target_hi {
                        edges[a].1
                    } else {
                        edges[a].0
                    }
                }
                Target::Sibling(name) => {
                    // Unknown ids are ignored, like the framework does.
                    let Some(&t) = names.get(name) else { continue };
                    match placed.get(&t) {
                        Some(r) => {
                            let tp = self.params(t);
                            if rule.target_hi {
                                r.pos(a) + r.len(a) + if rule.outer { tp.margin_hi[a] } else { 0.0 }
                            } else {
                                r.pos(a) - if rule.outer { tp.margin_lo[a] } else { 0.0 }
                            }
                        }
                        None if relax => continue,
                        None => return None,
                    }
                }
            };
            if rule.hi_side {
                anchors.hi[a] = Some(line);
            } else {
                anchors.lo[a] = Some(line);
            }
        }

        let spec = [0, 1].map(|a| {
            let (lo_edge, hi_edge) = edges[a];
            let (lo, hi) = (anchors.lo[a], anchors.hi[a]);
            let m = cp.margins(a);
            let free = |l: f32, h: f32| (h - l - m).max(0.0);
            let loose = |l: f32, h: f32| {
                if unbounded[a] {
                    Spec::Unspecified
                } else {
                    Spec::AtMost(free(l, h))
                }
            };
            if constraint {
                match cp.size[a] {
                    Dim::Dp(d) if d > 0.0 => Spec::Exactly(d),
                    Dim::Dp(_) => match (lo, hi) {
                        (Some(l), Some(h)) => Spec::Exactly(free(l, h)),
                        _ => loose(lo_edge, hi_edge),
                    },
                    Dim::Match if !unbounded[a] => Spec::Exactly(free(lo_edge, hi_edge)),
                    _ => loose(lo_edge, hi_edge),
                }
            } else {
                let (l, h) = (lo.unwrap_or(lo_edge), hi.unwrap_or(hi_edge));
                match cp.size[a] {
                    Dim::Dp(d) => Spec::Exactly(d),
                    Dim::Match if !unbounded[a] => Spec::Exactly(free(l, h)),
                    _ => loose(l, h),
                }
            }
        });
        let m = self.measure(c, spec);

        let mut pos = [0f32; 2];
        for a in 0..2 {
            let (lo_edge, _) = edges[a];
            let (lo, hi) = (anchors.lo[a], anchors.hi[a]);
            pos[a] = if constraint {
                match (lo, hi) {
                    _ if cp.size[a] == Dim::Match => lo_edge + cp.margin_lo[a],
                    (Some(l), Some(h)) => {
                        let bias = self
                            .doc
                            .attr(c, bias_attr(a))
                            .and_then(|b| b.trim().parse().ok())
                            .unwrap_or(0.5);
                        let start = l + cp.margin_lo[a];
                        let end = h - cp.margin_hi[a];
                        start + (end - start - m[a]) * bias
                    }
                    (Some(l), None) => l + cp.margin_lo[a],
                    (None, Some(h)) => h - cp.margin_hi[a] - m[a],
                    // Unconstrained: where the design editor last put it.
                    (None, None) => {
                        let attr = if a == 0 {
                            "tools:layout_editor_absoluteX"
                        } else {
                            "tools:layout_editor_absoluteY"
                        };
                        let abs = self
                            .doc
                            .attr(c, attr)
                            .and_then(|v| self.res.resolve_dimension(&v, self.density))
                            .unwrap_or(0.0);
                        lo_edge + abs
                    }
                }
            } else {
                match (lo, hi) {
                    (Some(l), _) => l + cp.margin_lo[a],
                    (None, Some(_)) if cp.size[a] == Dim::Match => lo_edge + cp.margin_lo[a],
                    (None, Some(h)) => h - cp.margin_hi[a] - m[a],
                    // centerHorizontal/Vertical ignore padding and margins.
                    (None, None) if centered[a] => (size[a] - m[a]) / 2.0,
                    (None, None) => lo_edge + cp.margin_lo[a],
                }
            };
        }
        self.local.insert(c, pos);
        self.local_anchors.insert(c, anchors);
        Some(Rect::new(pos[0], pos[1], m[0], m[1]))
    }

    fn place(&mut self, id: NodeId, x: f32, y: f32) {
        let m = self.measured.get(&id).copied().unwrap_or_default();
        self.result.rects.insert(id, Rect::new(x, y, m[0], m[1]));
        self.result.order.push(id);
        if classify(self.doc, id).container().is_none() {
            return;
        }
        for c in self.visible_children(id) {
            let l = self.local.get(&c).copied().unwrap_or_default();
            if let Some(a) = self.local_anchors.get(&c).copied() {
                self.result.anchors.insert(c, a.offset([x, y]));
            }
            self.place(c, x + l[0], y + l[1]);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(xml: &str) -> (Document, LayoutResult) {
        let doc = Document::parse(xml).unwrap();
        let res = Resources::default();
        let result = layout(&doc, &res, &ApproxTextMeasurer, [360.0, 640.0], 2.0);
        (doc, result)
    }

    fn rect(doc: &Document, result: &LayoutResult, id: &str) -> Rect {
        result.rect(doc.find_by_android_id(id).unwrap()).unwrap()
    }

    #[test]
    fn linear_layout_with_weights_and_margins() {
        let (doc, r) = run(r#"
<LinearLayout android:layout_width="match_parent" android:layout_height="match_parent"
    android:orientation="vertical" android:padding="10dp">
    <View android:id="@+id/a" android:layout_width="match_parent" android:layout_height="40dp"
        android:layout_marginBottom="5dp"/>
    <View android:id="@+id/b" android:layout_width="100dp" android:layout_height="0dp"
        android:layout_weight="1" android:layout_gravity="end"/>
    <View android:id="@+id/c" android:layout_width="50dp" android:layout_height="0dp"
        android:layout_weight="3" android:layout_gravity="center_horizontal"/>
</LinearLayout>"#);
        assert_eq!(rect(&doc, &r, "a"), Rect::new(10.0, 10.0, 340.0, 40.0));
        // 640 - 20 padding - 45 used = 575 free, split 1:3.
        let b = rect(&doc, &r, "b");
        assert_eq!((b.x, b.y, b.w), (250.0, 55.0, 100.0));
        assert!((b.h - 143.75).abs() < 0.01);
        let c = rect(&doc, &r, "c");
        assert_eq!(c.x, 155.0);
        assert!((c.bottom() - 630.0).abs() < 0.01);
    }

    #[test]
    fn frame_layout_gravity() {
        let (doc, r) = run(r#"
<FrameLayout android:layout_width="match_parent" android:layout_height="200dp">
    <View android:id="@+id/a" android:layout_width="20dp" android:layout_height="20dp"
        android:layout_gravity="bottom|end" android:layout_margin="4dp"/>
    <View android:id="@+id/b" android:layout_width="20dp" android:layout_height="20dp"
        android:layout_gravity="center"/>
</FrameLayout>"#);
        assert_eq!(rect(&doc, &r, "a"), Rect::new(336.0, 176.0, 20.0, 20.0));
        assert_eq!(rect(&doc, &r, "b"), Rect::new(170.0, 90.0, 20.0, 20.0));
    }

    #[test]
    fn relative_layout_rules() {
        let (doc, r) = run(r#"
<RelativeLayout android:layout_width="match_parent" android:layout_height="match_parent">
    <View android:id="@+id/below" android:layout_width="30dp" android:layout_height="10dp"
        android:layout_below="@id/top" android:layout_alignParentEnd="true"/>
    <View android:id="@+id/top" android:layout_width="50dp" android:layout_height="20dp"
        android:layout_centerHorizontal="true" android:layout_marginBottom="6dp"/>
    <View android:id="@+id/fill" android:layout_width="match_parent" android:layout_height="10dp"
        android:layout_toEndOf="@id/top" android:layout_alignParentBottom="true"/>
</RelativeLayout>"#);
        assert_eq!(rect(&doc, &r, "top"), Rect::new(155.0, 0.0, 50.0, 20.0));
        assert_eq!(rect(&doc, &r, "below"), Rect::new(330.0, 26.0, 30.0, 10.0));
        assert_eq!(rect(&doc, &r, "fill"), Rect::new(205.0, 630.0, 155.0, 10.0));
    }

    #[test]
    fn constraint_layout_basics() {
        let (doc, r) = run(r#"
<androidx.constraintlayout.widget.ConstraintLayout android:layout_width="match_parent" android:layout_height="match_parent">
    <View android:id="@+id/center" android:layout_width="100dp" android:layout_height="40dp"
        app:layout_constraintStart_toStartOf="parent" app:layout_constraintEnd_toEndOf="parent"
        app:layout_constraintTop_toTopOf="parent" app:layout_constraintBottom_toBottomOf="parent"/>
    <View android:id="@+id/biased" android:layout_width="60dp" android:layout_height="10dp"
        app:layout_constraintStart_toStartOf="parent" app:layout_constraintEnd_toEndOf="parent"
        app:layout_constraintHorizontal_bias="0"
        app:layout_constraintTop_toBottomOf="@id/center" android:layout_marginTop="8dp"/>
    <View android:id="@+id/stretch" android:layout_width="0dp" android:layout_height="10dp"
        android:layout_marginStart="16dp"
        app:layout_constraintStart_toEndOf="@id/biased" app:layout_constraintEnd_toEndOf="parent"
        app:layout_constraintBottom_toBottomOf="parent"/>
    <View android:id="@+id/free" android:layout_width="10dp" android:layout_height="10dp"
        tools:layout_editor_absoluteX="30dp" tools:layout_editor_absoluteY="40dp"/>
</androidx.constraintlayout.widget.ConstraintLayout>"#);
        assert_eq!(
            rect(&doc, &r, "center"),
            Rect::new(130.0, 300.0, 100.0, 40.0)
        );
        assert_eq!(rect(&doc, &r, "biased"), Rect::new(0.0, 348.0, 60.0, 10.0));
        assert_eq!(
            rect(&doc, &r, "stretch"),
            Rect::new(76.0, 630.0, 284.0, 10.0)
        );
        assert_eq!(rect(&doc, &r, "free"), Rect::new(30.0, 40.0, 10.0, 10.0));
        let anchors = r.anchors[&doc.find_by_android_id("biased").unwrap()];
        assert_eq!(anchors.lo[0], Some(0.0));
        assert_eq!(anchors.hi[0], Some(360.0));
        assert_eq!(anchors.lo[1], Some(340.0));
    }

    #[test]
    fn constraint_chain_does_not_hang() {
        let (doc, r) = run(r#"
<androidx.constraintlayout.widget.ConstraintLayout android:layout_width="match_parent" android:layout_height="match_parent">
    <View android:id="@+id/a" android:layout_width="10dp" android:layout_height="10dp"
        app:layout_constraintStart_toStartOf="parent" app:layout_constraintEnd_toStartOf="@id/b"/>
    <View android:id="@+id/b" android:layout_width="10dp" android:layout_height="10dp"
        app:layout_constraintStart_toEndOf="@id/a" app:layout_constraintEnd_toEndOf="parent"/>
</androidx.constraintlayout.widget.ConstraintLayout>"#);
        assert_eq!(rect(&doc, &r, "a").x, 0.0);
        assert_eq!(rect(&doc, &r, "b").x, 180.0);
    }

    #[test]
    fn wrap_content_parents_and_scroll_views() {
        let (doc, r) = run(r#"
<ScrollView android:layout_width="match_parent" android:layout_height="match_parent">
    <LinearLayout android:id="@+id/list" android:layout_width="match_parent"
        android:layout_height="wrap_content" android:orientation="vertical">
        <View android:id="@+id/tall" android:layout_width="match_parent" android:layout_height="1000dp"/>
        <TextView android:id="@+id/text" android:layout_width="wrap_content" android:layout_height="wrap_content"
            android:text="Hello" android:textSize="20sp"/>
    </LinearLayout>
</ScrollView>"#);
        let list = rect(&doc, &r, "list");
        let text = rect(&doc, &r, "text");
        assert_eq!(list.w, 360.0);
        assert_eq!(text.y, 1000.0);
        assert!(text.w > 40.0 && text.h > 20.0);
        assert_eq!(list.h, 1000.0 + text.h);
    }

    #[test]
    fn gone_views_take_no_space() {
        let (doc, r) = run(r#"
<LinearLayout android:layout_width="match_parent" android:layout_height="wrap_content">
    <View android:id="@+id/a" android:layout_width="50dp" android:layout_height="10dp" android:visibility="gone"/>
    <View android:id="@+id/b" android:layout_width="50dp" android:layout_height="10dp"/>
</LinearLayout>"#);
        assert!(r.rect(doc.find_by_android_id("a").unwrap()).is_none());
        assert_eq!(rect(&doc, &r, "b").x, 0.0);
    }
}
