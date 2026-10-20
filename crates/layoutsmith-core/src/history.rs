//! Undo and redo by document snapshots. Layout files are small, so keeping
//! whole copies is simpler and more robust than recording inverse edits.

use crate::xml::Document;

const LIMIT: usize = 200;

#[derive(Debug, Clone)]
struct Snapshot {
    doc: Document,
    label: String,
}

#[derive(Debug, Clone, Default)]
pub struct History {
    undo: Vec<Snapshot>,
    redo: Vec<Snapshot>,
    /// Consecutive edits with the same key (typing in one field, nudging with
    /// the arrow keys) become a single undo step.
    last_key: Option<String>,
}

impl History {
    /// Call before changing `doc`.
    pub fn record(&mut self, doc: &Document, label: &str) {
        self.last_key = None;
        self.push(doc, label);
    }

    /// Like [`History::record`], but merges with the previous step when it had
    /// the same `key`.
    pub fn record_merged(&mut self, doc: &Document, label: &str, key: &str) {
        if self.last_key.as_deref() == Some(key) {
            return;
        }
        self.push(doc, label);
        self.last_key = Some(key.to_string());
    }

    /// Ends merging, so the next edit starts a new undo step.
    pub fn seal(&mut self) {
        self.last_key = None;
    }

    fn push(&mut self, doc: &Document, label: &str) {
        self.undo.push(Snapshot {
            doc: doc.clone(),
            label: label.to_string(),
        });
        if self.undo.len() > LIMIT {
            self.undo.remove(0);
        }
        self.redo.clear();
    }

    pub fn undo(&mut self, current: &Document) -> Option<Document> {
        let snap = self.undo.pop()?;
        self.last_key = None;
        self.redo.push(Snapshot {
            doc: current.clone(),
            label: snap.label.clone(),
        });
        Some(snap.doc)
    }

    pub fn redo(&mut self, current: &Document) -> Option<Document> {
        let snap = self.redo.pop()?;
        self.last_key = None;
        self.undo.push(Snapshot {
            doc: current.clone(),
            label: snap.label.clone(),
        });
        Some(snap.doc)
    }

    pub fn undo_label(&self) -> Option<&str> {
        self.undo.last().map(|s| s.label.as_str())
    }

    pub fn redo_label(&self) -> Option<&str> {
        self.redo.last().map(|s| s.label.as_str())
    }

    pub fn clear(&mut self) {
        *self = Self::default();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn undo_redo_and_merging() {
        let mut doc = Document::parse("<A x=\"0\"/>").unwrap();
        let root = doc.root().unwrap();
        let mut h = History::default();

        h.record_merged(&doc, "Edit x", "x");
        doc.set_attr(root, "x", "1");
        h.record_merged(&doc, "Edit x", "x");
        doc.set_attr(root, "x", "12");
        h.record(&doc, "Add y");
        doc.set_attr(root, "y", "2");

        doc = h.undo(&doc).unwrap();
        assert_eq!(doc.to_xml(), "<A x=\"12\"/>");
        doc = h.undo(&doc).unwrap();
        assert_eq!(doc.to_xml(), "<A x=\"0\"/>");
        assert!(h.undo(&doc).is_none());
        assert_eq!(h.redo_label(), Some("Edit x"));
        doc = h.redo(&doc).unwrap();
        doc = h.redo(&doc).unwrap();
        assert_eq!(doc.to_xml(), "<A x=\"12\" y=\"2\"/>");
    }
}
