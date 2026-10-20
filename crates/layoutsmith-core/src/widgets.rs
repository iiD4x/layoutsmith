//! What kind of view an element is, as far as preview and editing care.

use crate::xml::{Document, NodeId};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContainerKind {
    Linear { vertical: bool },
    Frame,
    Relative,
    Constraint,
    Scroll { vertical: bool },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewKind {
    Container(ContainerKind),
    Text,
    Button,
    EditText,
    /// CheckBox, RadioButton, Switch: text with an indicator in front.
    Check,
    Image,
    Progress,
    Space,
    /// A plain `<View>`, usually a divider or a colored box.
    Plain,
    Include,
    /// A view we know nothing about (custom views, fragments, ...).
    Other,
}

impl ViewKind {
    pub fn container(self) -> Option<ContainerKind> {
        match self {
            ViewKind::Container(c) => Some(c),
            _ => None,
        }
    }

    pub fn shows_text(self) -> bool {
        matches!(
            self,
            ViewKind::Text | ViewKind::Button | ViewKind::EditText | ViewKind::Check
        )
    }
}

/// `androidx.constraintlayout.widget.ConstraintLayout` -> `ConstraintLayout`.
pub fn short_name(tag: &str) -> &str {
    tag.rsplit('.').next().unwrap_or(tag)
}

pub fn classify(doc: &Document, id: NodeId) -> ViewKind {
    let name = short_name(doc.name(id));
    let orientation = |default_vertical: bool| {
        let vertical = match doc.attr(id, "android:orientation").as_deref() {
            Some("vertical") => true,
            Some("horizontal") => false,
            _ => default_vertical,
        };
        ViewKind::Container(ContainerKind::Linear { vertical })
    };
    match name {
        "LinearLayout" | "TableRow" | "MaterialButtonToggleGroup" | "ChipGroup" => {
            orientation(false)
        }
        "RadioGroup" | "TableLayout" | "TextInputLayout" | "AppBarLayout" | "GridLayout" => {
            orientation(true)
        }
        "RelativeLayout" | "PercentRelativeLayout" => ViewKind::Container(ContainerKind::Relative),
        "ConstraintLayout" | "MotionLayout" => ViewKind::Container(ContainerKind::Constraint),
        "ScrollView" | "NestedScrollView" => {
            ViewKind::Container(ContainerKind::Scroll { vertical: true })
        }
        "HorizontalScrollView" => ViewKind::Container(ContainerKind::Scroll { vertical: false }),
        "FrameLayout"
        | "CoordinatorLayout"
        | "CardView"
        | "MaterialCardView"
        | "Toolbar"
        | "MaterialToolbar"
        | "merge"
        | "DrawerLayout"
        | "SwipeRefreshLayout"
        | "CollapsingToolbarLayout" => ViewKind::Container(ContainerKind::Frame),
        "Space" => ViewKind::Space,
        "View" => ViewKind::Plain,
        "include" => ViewKind::Include,
        "CheckBox"
        | "RadioButton"
        | "Switch"
        | "SwitchCompat"
        | "SwitchMaterial"
        | "MaterialSwitch"
        | "MaterialCheckBox"
        | "MaterialRadioButton" => ViewKind::Check,
        "ProgressBar"
        | "SeekBar"
        | "RatingBar"
        | "Slider"
        | "CircularProgressIndicator"
        | "LinearProgressIndicator" => ViewKind::Progress,
        _ if name.contains("EditText") || name.ends_with("AutoCompleteTextView") => {
            ViewKind::EditText
        }
        _ if name.contains("ImageView")
            || name.contains("ImageButton")
            || name == "FloatingActionButton" =>
        {
            ViewKind::Image
        }
        _ if name.contains("Button") || name == "Chip" => ViewKind::Button,
        _ if name.contains("TextView") => ViewKind::Text,
        _ if name.ends_with("Layout") || !doc.element_children(id).is_empty() => {
            ViewKind::Container(ContainerKind::Frame)
        }
        _ => ViewKind::Other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kind_of(xml: &str) -> ViewKind {
        let doc = Document::parse(xml).unwrap();
        classify(&doc, doc.root().unwrap())
    }

    #[test]
    fn classifies_common_views() {
        assert_eq!(
            kind_of("<LinearLayout/>"),
            ViewKind::Container(ContainerKind::Linear { vertical: false })
        );
        assert_eq!(
            kind_of(r#"<LinearLayout android:orientation="vertical"/>"#),
            ViewKind::Container(ContainerKind::Linear { vertical: true })
        );
        assert_eq!(
            kind_of("<androidx.constraintlayout.widget.ConstraintLayout/>"),
            ViewKind::Container(ContainerKind::Constraint)
        );
        assert_eq!(
            kind_of("<com.google.android.material.button.MaterialButton/>"),
            ViewKind::Button
        );
        assert_eq!(
            kind_of("<com.google.android.material.floatingactionbutton.FloatingActionButton/>"),
            ViewKind::Image
        );
        assert_eq!(kind_of("<ImageButton/>"), ViewKind::Image);
        assert_eq!(
            kind_of("<com.google.android.material.textfield.TextInputEditText/>"),
            ViewKind::EditText
        );
        assert_eq!(kind_of("<TextView/>"), ViewKind::Text);
        assert_eq!(
            kind_of("<com.example.Fancy><View/></com.example.Fancy>"),
            ViewKind::Container(ContainerKind::Frame)
        );
        assert_eq!(kind_of("<com.example.Fancy/>"), ViewKind::Other);
    }
}
