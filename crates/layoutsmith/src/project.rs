//! An open layout file: its document, resources, current layout and history.

use std::path::{Path, PathBuf};
use std::rc::Rc;

use eframe::egui;
use layoutsmith_core::layout::DEFAULT_DENSITY;
use layoutsmith_core::{Document, History, LayoutResult, Resources, TextMeasurer, layout};

/// Measures text with egui's fonts so the preview's boxes match the text the
/// canvas draws.
pub struct EguiMeasurer {
    pub ctx: egui::Context,
}

impl TextMeasurer for EguiMeasurer {
    fn measure(&self, text: &str, size: f32, bold: bool, max_width: Option<f32>) -> [f32; 2] {
        let galley = self.ctx.fonts_mut(|f| {
            f.layout(
                text.to_owned(),
                egui::FontId::proportional(size),
                egui::Color32::WHITE,
                max_width.unwrap_or(f32::INFINITY),
            )
        });
        let size = galley.size();
        // The bundled fonts have no bold face; bold text is drawn wider.
        [(size.x * if bold { 1.06 } else { 1.0 }).ceil(), size.y]
    }
}

pub struct Project {
    pub path: Option<PathBuf>,
    pub doc: Document,
    pub res: Resources,
    pub layout: LayoutResult,
    pub history: History,
    /// Screen size in dp.
    pub screen: [f32; 2],
    /// Bumped on every document change.
    pub revision: u64,
    saved_xml: String,
    dirty: bool,
    measurer: Rc<dyn TextMeasurer>,
}

impl Project {
    pub fn open(
        path: &Path,
        measurer: Rc<dyn TextMeasurer>,
        screen: [f32; 2],
    ) -> Result<Self, String> {
        let bytes =
            std::fs::read(path).map_err(|e| format!("Cannot read {}: {e}", path.display()))?;
        let (text, utf16) = layoutsmith_core::xml::decode(&bytes)
            .map_err(|e| format!("{}: {e}", path.display()))?;
        // Saving writes UTF-8; refuse rather than silently change the encoding.
        if utf16 {
            return Err(format!(
                "{} is saved as UTF-16. Save it as UTF-8 (as Android Studio does) to edit it.",
                path.display()
            ));
        }
        let doc = Document::parse(&text).map_err(|e| format!("{}:{e}", path.display()))?;
        let res = Resources::for_layout(path);
        Ok(Self::new(
            doc,
            res,
            Some(path.to_path_buf()),
            text,
            measurer,
            screen,
        ))
    }

    /// A new, unsaved layout.
    pub fn untitled(source: &str, measurer: Rc<dyn TextMeasurer>, screen: [f32; 2]) -> Self {
        let doc = Document::parse(source).expect("template parses");
        let mut project = Self::new(
            doc,
            Resources::default(),
            None,
            String::new(),
            measurer,
            screen,
        );
        project.dirty = true;
        project
    }

    fn new(
        doc: Document,
        res: Resources,
        path: Option<PathBuf>,
        saved_xml: String,
        measurer: Rc<dyn TextMeasurer>,
        screen: [f32; 2],
    ) -> Self {
        let mut project = Self {
            path,
            doc,
            res,
            layout: LayoutResult::default(),
            history: History::default(),
            screen,
            revision: 0,
            saved_xml,
            dirty: false,
            measurer,
        };
        project.relayout();
        project
    }

    pub fn file_name(&self) -> String {
        self.path.as_ref().and_then(|p| p.file_name()).map_or_else(
            || "untitled.xml".to_string(),
            |n| n.to_string_lossy().into_owned(),
        )
    }

    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    pub fn save_as(&mut self, path: &Path) -> Result<(), String> {
        let xml = self.doc.to_xml();
        std::fs::write(path, &xml).map_err(|e| format!("Cannot write {}: {e}", path.display()))?;
        if self.path.as_deref() != Some(path) {
            self.path = Some(path.to_path_buf());
            self.res = Resources::for_layout(path);
            self.relayout();
        }
        self.saved_xml = xml;
        self.dirty = false;
        Ok(())
    }

    pub fn reload_resources(&mut self) {
        if let Some(path) = &self.path {
            self.res = Resources::for_layout(path);
            self.relayout();
        }
    }

    pub fn set_screen(&mut self, screen: [f32; 2]) {
        if self.screen != screen {
            self.screen = screen;
            self.relayout();
        }
    }

    pub fn relayout(&mut self) {
        self.layout = layout(
            &self.doc,
            &self.res,
            &*self.measurer,
            self.screen,
            DEFAULT_DENSITY,
        );
    }

    fn changed(&mut self) {
        self.revision += 1;
        self.dirty = self.doc.to_xml() != self.saved_xml;
        self.relayout();
    }

    /// Applies an edit as one undo step. Edits sharing a `merge_key` in a row
    /// (dragging a number, typing in a field) collapse into one step.
    pub fn edit(
        &mut self,
        label: &str,
        merge_key: Option<&str>,
        f: impl FnOnce(&mut Document, &Resources, &LayoutResult),
    ) {
        let before = self.doc.clone();
        f(&mut self.doc, &self.res, &self.layout);
        if self.doc == before {
            return;
        }
        match merge_key {
            Some(key) => self.history.record_merged(&before, label, key),
            None => self.history.record(&before, label),
        }
        self.changed();
    }

    /// Shows a document without recording history, used while dragging.
    pub fn preview(&mut self, doc: Document) {
        if doc != self.doc {
            self.doc = doc;
            self.changed();
        }
    }

    /// Ends a drag: records `origin` as the undo step if anything changed.
    pub fn commit_preview(&mut self, origin: &Document, label: &str) {
        if self.doc.to_xml() != origin.to_xml() {
            self.history.record(origin, label);
        } else {
            self.preview(origin.clone());
        }
        self.history.seal();
    }

    /// Replaces the whole document (raw XML editing) as one undo step.
    pub fn replace_source(&mut self, source: &str) -> Result<(), String> {
        let doc = Document::parse(source).map_err(|e| e.to_string())?;
        let before = std::mem::replace(&mut self.doc, doc);
        self.history.record(&before, "Edit XML");
        self.changed();
        Ok(())
    }

    pub fn undo(&mut self) -> bool {
        match self.history.undo(&self.doc) {
            Some(doc) => {
                self.doc = doc;
                self.changed();
                true
            }
            None => false,
        }
    }

    pub fn redo(&mut self) -> bool {
        match self.history.redo(&self.doc) {
            Some(doc) => {
                self.doc = doc;
                self.changed();
                true
            }
            None => false,
        }
    }
}
