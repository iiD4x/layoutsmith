//! A small lossless XML document model.
//!
//! Android layout files are written by hand and live in version control, so the
//! editor must never reformat them. The parser keeps every byte of the original
//! text (whitespace between attributes, quote style, comments, entity escapes)
//! and [`Document::to_xml`] reproduces the input exactly when nothing changed.
//! Edits only touch the attributes and nodes they are about.

use std::fmt;

/// Index of a node inside a [`Document`]. Ids stay valid for the lifetime of the
/// document (and its clones): removed nodes are detached, never reused.
pub type NodeId = usize;

/// The id of the invisible node that holds the prolog, the root element and
/// anything after it.
pub const DOCUMENT: NodeId = 0;

pub const ANDROID_NS: &str = "http://schemas.android.com/apk/res/android";
pub const APP_NS: &str = "http://schemas.android.com/apk/res-auto";
pub const TOOLS_NS: &str = "http://schemas.android.com/tools";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError {
    pub line: usize,
    pub column: usize,
    pub message: String,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}: {}", self.line, self.column, self.message)
    }
}

impl std::error::Error for ParseError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Attr {
    /// Whitespace between the previous token and the attribute name.
    pub leading: String,
    pub name: String,
    /// Everything between the name and the opening quote, usually `=`.
    pub eq: String,
    pub quote: char,
    /// The value exactly as written, entities still escaped.
    pub raw_value: String,
}

impl Attr {
    pub fn value(&self) -> String {
        unescape(&self.raw_value)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Element {
    pub name: String,
    pub attrs: Vec<Attr>,
    /// Whitespace between the last attribute and `>` or `/>`.
    pub tag_tail: String,
    pub self_closing: bool,
    /// The closing tag as written. Empty means "generate `</name>`".
    pub end_tag: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NodeKind {
    Document,
    Element(Element),
    /// Character data exactly as written (entities still escaped).
    Text(String),
    /// Comments, the XML declaration, processing instructions, CDATA and
    /// doctypes, kept verbatim.
    Raw(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Node {
    pub kind: NodeKind,
    pub parent: Option<NodeId>,
    pub children: Vec<NodeId>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Document {
    nodes: Vec<Node>,
}

impl Default for Document {
    fn default() -> Self {
        Self {
            nodes: vec![Node {
                kind: NodeKind::Document,
                parent: None,
                children: Vec::new(),
            }],
        }
    }
}

impl Document {
    pub fn parse(src: &str) -> Result<Self, ParseError> {
        Parser { src, pos: 0 }.parse()
    }

    pub fn to_xml(&self) -> String {
        let mut out = String::new();
        for &c in &self.nodes[DOCUMENT].children {
            self.write_node(c, &mut out);
        }
        out
    }

    /// Serializes a single node and its subtree.
    pub fn node_to_xml(&self, id: NodeId) -> String {
        let mut out = String::new();
        self.write_node(id, &mut out);
        out
    }

    fn write_node(&self, id: NodeId, out: &mut String) {
        let node = &self.nodes[id];
        match &node.kind {
            NodeKind::Document => {}
            NodeKind::Text(t) | NodeKind::Raw(t) => out.push_str(t),
            NodeKind::Element(e) => {
                out.push('<');
                out.push_str(&e.name);
                for a in &e.attrs {
                    out.push_str(&a.leading);
                    out.push_str(&a.name);
                    out.push_str(&a.eq);
                    out.push(a.quote);
                    out.push_str(&a.raw_value);
                    out.push(a.quote);
                }
                out.push_str(&e.tag_tail);
                if e.self_closing && node.children.is_empty() {
                    out.push_str("/>");
                    return;
                }
                out.push('>');
                for &c in &node.children {
                    self.write_node(c, out);
                }
                if e.end_tag.is_empty() {
                    out.push_str("</");
                    out.push_str(&e.name);
                    out.push('>');
                } else {
                    out.push_str(&e.end_tag);
                }
            }
        }
    }

    // ---- queries -----------------------------------------------------------

    pub fn node(&self, id: NodeId) -> &Node {
        &self.nodes[id]
    }

    /// The root element, if the document has one.
    pub fn root(&self) -> Option<NodeId> {
        self.nodes[DOCUMENT]
            .children
            .iter()
            .copied()
            .find(|&c| self.is_element(c))
    }

    pub fn is_element(&self, id: NodeId) -> bool {
        matches!(
            self.nodes.get(id).map(|n| &n.kind),
            Some(NodeKind::Element(_))
        )
    }

    pub fn element(&self, id: NodeId) -> Option<&Element> {
        match &self.nodes.get(id)?.kind {
            NodeKind::Element(e) => Some(e),
            _ => None,
        }
    }

    fn element_mut(&mut self, id: NodeId) -> Option<&mut Element> {
        match &mut self.nodes.get_mut(id)?.kind {
            NodeKind::Element(e) => Some(e),
            _ => None,
        }
    }

    /// The tag name of an element, or `""` for other nodes.
    pub fn name(&self, id: NodeId) -> &str {
        self.element(id).map_or("", |e| e.name.as_str())
    }

    /// The unescaped value of an attribute.
    pub fn attr(&self, id: NodeId, name: &str) -> Option<String> {
        self.element(id)?
            .attrs
            .iter()
            .find(|a| a.name == name)
            .map(Attr::value)
    }

    pub fn has_attr(&self, id: NodeId, name: &str) -> bool {
        self.element(id)
            .is_some_and(|e| e.attrs.iter().any(|a| a.name == name))
    }

    /// The parent element, or `None` for the root element and detached nodes.
    pub fn parent(&self, id: NodeId) -> Option<NodeId> {
        self.nodes.get(id)?.parent.filter(|&p| p != DOCUMENT)
    }

    pub fn element_children(&self, id: NodeId) -> Vec<NodeId> {
        self.nodes[id]
            .children
            .iter()
            .copied()
            .filter(|&c| self.is_element(c))
            .collect()
    }

    /// Whether `id` is still part of the document tree.
    pub fn is_attached(&self, id: NodeId) -> bool {
        let mut cur = id;
        loop {
            if cur == DOCUMENT {
                return true;
            }
            match self.nodes.get(cur).and_then(|n| n.parent) {
                Some(p) => cur = p,
                None => return false,
            }
        }
    }

    pub fn is_ancestor_or_self(&self, ancestor: NodeId, id: NodeId) -> bool {
        let mut cur = Some(id);
        while let Some(c) = cur {
            if c == ancestor {
                return true;
            }
            cur = self.nodes[c].parent;
        }
        false
    }

    /// The element and all elements below it, in document order.
    pub fn descendants(&self, id: NodeId) -> Vec<NodeId> {
        let mut out = Vec::new();
        self.collect_elements(id, &mut out);
        out
    }

    fn collect_elements(&self, id: NodeId, out: &mut Vec<NodeId>) {
        if self.is_element(id) {
            out.push(id);
        }
        for &c in &self.nodes[id].children {
            self.collect_elements(c, out);
        }
    }

    /// Concatenated, unescaped text inside an element.
    pub fn text_content(&self, id: NodeId) -> String {
        let mut out = String::new();
        for &c in &self.nodes[id].children {
            match &self.nodes[c].kind {
                NodeKind::Text(t) => out.push_str(&unescape(t)),
                NodeKind::Raw(r) if r.starts_with("<![CDATA[") => {
                    out.push_str(&r[9..r.len() - 3]);
                }
                NodeKind::Element(_) => out.push_str(&self.text_content(c)),
                _ => {}
            }
        }
        out
    }

    /// Finds the element whose `android:id` is `@+id/<name>` or `@id/<name>`.
    pub fn find_by_android_id(&self, name: &str) -> Option<NodeId> {
        let root = self.root()?;
        self.descendants(root).into_iter().find(|&n| {
            self.attr(n, "android:id")
                .is_some_and(|v| id_name(&v) == Some(name))
        })
    }

    // ---- attribute edits ---------------------------------------------------

    /// Sets an attribute, keeping its position and quoting if it already
    /// exists. New attributes follow the formatting of their neighbours.
    pub fn set_attr(&mut self, id: NodeId, name: &str, value: &str) {
        let leading = self.new_attr_leading(id);
        let Some(e) = self.element_mut(id) else {
            return;
        };
        if let Some(a) = e.attrs.iter_mut().find(|a| a.name == name) {
            a.raw_value = escape_attr(value, a.quote);
            return;
        }
        let index = insertion_index(&e.attrs, name);
        e.attrs.insert(
            index,
            Attr {
                leading,
                name: name.to_string(),
                eq: "=".to_string(),
                quote: '"',
                raw_value: escape_attr(value, '"'),
            },
        );
    }

    /// Renames an element (changes its view type), keeping its attributes.
    pub fn set_name(&mut self, id: NodeId, name: &str) {
        if let Some(e) = self.element_mut(id) {
            e.name = name.to_string();
            e.end_tag.clear();
        }
    }

    /// Removes an attribute. Returns whether it existed.
    pub fn remove_attr(&mut self, id: NodeId, name: &str) -> bool {
        let Some(e) = self.element_mut(id) else {
            return false;
        };
        let Some(i) = e.attrs.iter().position(|a| a.name == name) else {
            return false;
        };
        let removed = e.attrs.remove(i);
        // Keep the first attribute on the tag's line if it was there before.
        if i == 0
            && let Some(next) = e.attrs.first_mut()
            && !removed.leading.contains('\n')
        {
            next.leading = removed.leading;
        }
        true
    }

    /// Declares `xmlns:<prefix>` on the root element if it is missing.
    pub fn ensure_namespace(&mut self, prefix: &str, uri: &str) {
        let Some(root) = self.root() else { return };
        let name = format!("xmlns:{prefix}");
        if !self.has_attr(root, &name) {
            self.set_attr(root, &name, uri);
        }
    }

    fn new_attr_leading(&self, id: NodeId) -> String {
        let Some(e) = self.element(id) else {
            return " ".into();
        };
        // Prefer the style of an attribute that sits on its own line.
        if let Some(a) = e
            .attrs
            .iter()
            .skip(1)
            .rev()
            .find(|a| a.leading.contains('\n'))
        {
            return a.leading.clone();
        }
        if let Some(a) = e.attrs.last() {
            return a.leading.clone();
        }
        if self.uses_attr_per_line() {
            format!("\n{}{}", self.indent_of(id), self.indent_unit())
        } else {
            " ".into()
        }
    }

    /// Whether the file puts each attribute on its own line, as Android
    /// Studio's formatter does.
    pub fn uses_attr_per_line(&self) -> bool {
        let Some(root) = self.root() else { return true };
        let mut seen_multi = false;
        for n in self.descendants(root) {
            let attrs = &self.element(n).unwrap().attrs;
            if attrs.len() > 1 {
                if attrs[1..].iter().any(|a| a.leading.contains('\n')) {
                    return true;
                }
                seen_multi = true;
            }
        }
        !seen_multi
    }

    // ---- indentation -------------------------------------------------------

    /// The indentation of the line an element starts on.
    pub fn indent_of(&self, id: NodeId) -> String {
        let Some(parent) = self.nodes[id].parent else {
            return String::new();
        };
        if parent == DOCUMENT {
            return String::new();
        }
        let siblings = &self.nodes[parent].children;
        if let Some(i) = siblings.iter().position(|&c| c == id)
            && i > 0
            && let NodeKind::Text(t) = &self.nodes[siblings[i - 1]].kind
            && let Some(nl) = t.rfind('\n')
        {
            let tail = &t[nl + 1..];
            if tail.chars().all(|c| c == ' ' || c == '\t') {
                return tail.to_string();
            }
        }
        format!("{}{}", self.indent_of(parent), self.indent_unit())
    }

    /// One level of indentation as used by this file (four spaces by default).
    pub fn indent_unit(&self) -> String {
        if let Some(root) = self.root()
            && let Some(&first) = self.element_children(root).first()
        {
            let siblings = &self.nodes[root].children;
            let i = siblings.iter().position(|&c| c == first).unwrap();
            if i > 0
                && let NodeKind::Text(t) = &self.nodes[siblings[i - 1]].kind
                && let Some(nl) = t.rfind('\n')
            {
                let tail = &t[nl + 1..];
                if !tail.is_empty() && tail.chars().all(|c| c == ' ' || c == '\t') {
                    return tail.to_string();
                }
            }
        }
        "    ".into()
    }

    // ---- structural edits --------------------------------------------------

    /// Creates a detached element. Attach it with [`Document::insert_element`].
    pub fn create_element(&mut self, name: &str, attrs: &[(&str, &str)]) -> NodeId {
        let per_line = self.uses_attr_per_line();
        let unit = self.indent_unit();
        let element = Element {
            name: name.to_string(),
            attrs: attrs
                .iter()
                .map(|(n, v)| Attr {
                    leading: if per_line {
                        format!("\n{unit}")
                    } else {
                        " ".into()
                    },
                    name: n.to_string(),
                    eq: "=".into(),
                    quote: '"',
                    raw_value: escape_attr(v, '"'),
                })
                .collect(),
            tag_tail: " ".into(),
            self_closing: true,
            end_tag: String::new(),
        };
        self.push_node(NodeKind::Element(element), None)
    }

    /// Copies an element and its subtree. The copy starts detached.
    pub fn clone_subtree(&mut self, id: NodeId) -> NodeId {
        let kind = self.nodes[id].kind.clone();
        let copy = self.push_node(kind, None);
        for c in self.nodes[id].children.clone() {
            let cc = self.clone_subtree(c);
            self.nodes[cc].parent = Some(copy);
            self.nodes[copy].children.push(cc);
        }
        copy
    }

    /// Removes an element (and the indentation in front of it) from the tree.
    pub fn detach(&mut self, id: NodeId) {
        self.detach_with_whitespace(id);
    }

    /// Detaches an element together with the whitespace separating it from a
    /// sibling, and returns that whitespace. The siblings left behind keep
    /// their layout: the first child gives up the separator after it, any
    /// other child the one before it.
    fn detach_with_whitespace(&mut self, id: NodeId) -> Option<String> {
        let parent = self.nodes[id].parent?;
        let i = self.nodes[parent].children.iter().position(|&c| c == id)?;
        self.nodes[parent].children.remove(i);
        self.nodes[id].parent = None;

        let kids = &self.nodes[parent].children;
        let is_ws = |n: NodeId| matches!(&self.nodes[n].kind, NodeKind::Text(t) if t.trim().is_empty() && t.contains('\n'));
        let element_before = kids[..i].iter().any(|&c| self.is_element(c));
        let element_after = kids[i..].iter().any(|&c| self.is_element(c));
        let victim = if i > 0 && is_ws(kids[i - 1]) && (element_before || !element_after) {
            Some(i - 1)
        } else if i < kids.len() && is_ws(kids[i]) && element_after {
            Some(i)
        } else {
            None
        }?;
        let node = self.nodes[parent].children.remove(victim);
        self.nodes[node].parent = None;
        match &self.nodes[node].kind {
            NodeKind::Text(t) => Some(t.clone()),
            _ => None,
        }
    }

    /// Moves an attached element to `index` among the element children of
    /// `new_parent`, re-indenting it for its new depth.
    pub fn move_element(&mut self, id: NodeId, new_parent: NodeId, index: usize) {
        let old_indent = self.indent_of(id);
        let ws = self.detach_with_whitespace(id);
        self.attach(id, new_parent, index, &old_indent, ws);
    }

    /// Attaches a detached element created by [`Document::create_element`] or
    /// [`Document::clone_subtree`]. `source_indent` is the indentation the
    /// subtree was written for (`""` for freshly created elements).
    pub fn insert_element(
        &mut self,
        id: NodeId,
        parent: NodeId,
        index: usize,
        source_indent: &str,
    ) {
        self.attach(id, parent, index, source_indent, None);
    }

    /// `own_ws` is the whitespace that preceded a moved element; reusing it
    /// keeps blank lines between siblings where they were.
    fn attach(
        &mut self,
        id: NodeId,
        parent: NodeId,
        index: usize,
        old_indent: &str,
        own_ws: Option<String>,
    ) {
        let parent_indent = self.indent_of(parent);
        let new_indent = format!("{parent_indent}{}", self.indent_unit());
        if let Some(e) = self.element_mut(parent)
            && e.self_closing
        {
            e.self_closing = false;
            e.end_tag.clear();
        }
        let separator = match own_ws {
            Some(ws) => reindent_ws(&ws, old_indent, &new_indent),
            None => self.sibling_separator(parent, &new_indent),
        };
        self.reindent(id, old_indent, &new_indent);
        self.nodes[id].parent = Some(parent);

        let kids = &self.nodes[parent].children;
        let element_positions: Vec<usize> = kids
            .iter()
            .enumerate()
            .filter(|&(_, &c)| self.is_element(c))
            .map(|(i, _)| i)
            .collect();
        let had_children = !kids.is_empty();
        if let Some(&p) = element_positions.get(index) {
            // [.., ws, X, separator, next]
            let sep = self.push_node(NodeKind::Text(separator), Some(parent));
            let kids = &mut self.nodes[parent].children;
            kids.insert(p, sep);
            kids.insert(p, id);
        } else if let Some(&last) = element_positions.last() {
            // [.., last, separator, X, closing ws]
            let sep = self.push_node(NodeKind::Text(separator), Some(parent));
            let kids = &mut self.nodes[parent].children;
            kids.insert(last + 1, id);
            kids.insert(last + 1, sep);
        } else {
            let ws = self.push_node(NodeKind::Text(format!("\n{new_indent}")), Some(parent));
            let kids = &mut self.nodes[parent].children;
            kids.insert(0, id);
            kids.insert(0, ws);
            if !had_children {
                let close =
                    self.push_node(NodeKind::Text(format!("\n{parent_indent}")), Some(parent));
                self.nodes[parent].children.push(close);
            }
        }
    }

    /// Whitespace to put between two children of `parent`: a blank line if
    /// its children are separated by blank lines, a plain newline otherwise.
    fn sibling_separator(&self, parent: NodeId, indent: &str) -> String {
        let kids = &self.nodes[parent].children;
        let blank_lines = kids
            .iter()
            .enumerate()
            .filter(|&(i, &c)| i > 0 && self.is_element(c))
            .filter_map(|(i, _)| match &self.nodes[kids[i - 1]].kind {
                NodeKind::Text(t) if t.trim().is_empty() => Some(t.matches('\n').count() > 1),
                _ => None,
            })
            .next_back()
            .unwrap_or(false);
        format!("{}{indent}", if blank_lines { "\n\n" } else { "\n" })
    }

    fn reindent(&mut self, id: NodeId, old: &str, new: &str) {
        if old == new {
            return;
        }
        match &mut self.nodes[id].kind {
            NodeKind::Element(e) => {
                for a in &mut e.attrs {
                    a.leading = reindent_ws(&a.leading, old, new);
                }
                e.tag_tail = reindent_ws(&e.tag_tail, old, new);
            }
            NodeKind::Text(t) if t.trim().is_empty() => *t = reindent_ws(t, old, new),
            _ => {}
        }
        for c in self.nodes[id].children.clone() {
            self.reindent(c, old, new);
        }
    }

    fn push_node(&mut self, kind: NodeKind, parent: Option<NodeId>) -> NodeId {
        self.nodes.push(Node {
            kind,
            parent,
            children: Vec::new(),
        });
        self.nodes.len() - 1
    }
}

/// Where a new attribute goes: namespaces after namespaces, layout params
/// after the other layout params, everything else at the end.
fn insertion_index(attrs: &[Attr], name: &str) -> usize {
    let after_last =
        |pred: &dyn Fn(&str) -> bool| attrs.iter().rposition(|a| pred(&a.name)).map(|i| i + 1);
    if name.starts_with("xmlns:") {
        return after_last(&|n| n.starts_with("xmlns")).unwrap_or(0);
    }
    if let Some((prefix, rest)) = name.split_once(':')
        && rest.starts_with("layout_")
    {
        let group = format!("{prefix}:layout_");
        if let Some(i) = after_last(&|n| n.starts_with(&group)) {
            return i;
        }
        if let Some(i) = after_last(&|n| n.contains(":layout_")) {
            return i;
        }
    }
    attrs.len()
}

fn reindent_ws(ws: &str, old: &str, new: &str) -> String {
    if !ws.contains('\n') {
        return ws.to_string();
    }
    let mut out = String::with_capacity(ws.len() + new.len());
    for (i, line) in ws.split('\n').enumerate() {
        if i > 0 {
            out.push('\n');
            match line.strip_prefix(old) {
                Some(rest) => {
                    out.push_str(new);
                    out.push_str(rest);
                }
                None => out.push_str(line),
            }
        } else {
            out.push_str(line);
        }
    }
    out
}

/// The name part of an id reference: `@+id/title` and `@id/title` give `title`.
pub fn id_name(value: &str) -> Option<&str> {
    value
        .strip_prefix("@+id/")
        .or_else(|| value.strip_prefix("@id/"))
        .or_else(|| value.strip_prefix("@android:id/"))
}

pub fn unescape(raw: &str) -> String {
    if !raw.contains('&') {
        return raw.to_string();
    }
    let mut out = String::with_capacity(raw.len());
    let mut rest = raw;
    while let Some(amp) = rest.find('&') {
        out.push_str(&rest[..amp]);
        rest = &rest[amp..];
        let Some(semi) = rest.find(';') else { break };
        let entity = &rest[1..semi];
        let decoded = match entity {
            "lt" => Some('<'),
            "gt" => Some('>'),
            "amp" => Some('&'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            _ => entity
                .strip_prefix("#x")
                .or_else(|| entity.strip_prefix("#X"))
                .and_then(|h| u32::from_str_radix(h, 16).ok())
                .or_else(|| entity.strip_prefix('#').and_then(|d| d.parse().ok()))
                .and_then(char::from_u32),
        };
        match decoded {
            Some(c) => {
                out.push(c);
                rest = &rest[semi + 1..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

pub fn escape_attr(value: &str, quote: char) -> String {
    let mut out = String::with_capacity(value.len());
    for c in value.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '"' if quote == '"' => out.push_str("&quot;"),
            '\'' if quote == '\'' => out.push_str("&apos;"),
            '\n' => out.push_str("&#10;"),
            _ => out.push(c),
        }
    }
    out
}

struct Parser<'a> {
    src: &'a str,
    pos: usize,
}

impl Parser<'_> {
    fn parse(mut self) -> Result<Document, ParseError> {
        let mut doc = Document::default();
        let mut stack: Vec<(NodeId, usize)> = vec![(DOCUMENT, 0)];
        let src = self.src;
        while self.pos < src.len() {
            let parent = stack.last().unwrap().0;
            let rest = &src[self.pos..];
            if !rest.starts_with('<') {
                let end = rest.find('<').map_or(src.len(), |i| self.pos + i);
                let text = &src[self.pos..end];
                if parent == DOCUMENT
                    && !text
                        .trim_matches(|c: char| c.is_whitespace() || c == '\u{feff}')
                        .is_empty()
                {
                    return Err(self.error(self.pos, "text outside the root element"));
                }
                doc.push_child(parent, NodeKind::Text(text.to_string()));
                self.pos = end;
                continue;
            }
            let delimited = [("<!--", "-->"), ("<![CDATA[", "]]>"), ("<?", "?>")]
                .into_iter()
                .find(|(open, _)| rest.starts_with(open));
            if let Some((open, close)) = delimited {
                let end = rest[open.len()..]
                    .find(close)
                    .map(|i| self.pos + open.len() + i + close.len())
                    .ok_or_else(|| self.error(self.pos, &format!("missing `{close}`")))?;
                doc.push_child(parent, NodeKind::Raw(src[self.pos..end].to_string()));
                self.pos = end;
            } else if rest.starts_with("<!") {
                let end = self.doctype_end()?;
                doc.push_child(parent, NodeKind::Raw(src[self.pos..end].to_string()));
                self.pos = end;
            } else if rest.starts_with("</") {
                let close = rest
                    .find('>')
                    .ok_or_else(|| self.error(self.pos, "unterminated closing tag"))?;
                let name = rest[2..close].trim();
                if stack.len() < 2 {
                    return Err(self.error(self.pos, &format!("unexpected `</{name}>`")));
                }
                let (open_id, _) = stack.pop().unwrap();
                let element = doc.element_mut(open_id).unwrap();
                if element.name != name {
                    let expected = element.name.clone();
                    return Err(self.error(
                        self.pos,
                        &format!("expected `</{expected}>` but found `</{name}>`"),
                    ));
                }
                element.end_tag = rest[..=close].to_string();
                self.pos += close + 1;
            } else {
                let start = self.pos;
                let element = self.start_tag()?;
                if parent == DOCUMENT && doc.root().is_some() {
                    return Err(self.error(start, "more than one root element"));
                }
                let self_closing = element.self_closing;
                let id = doc.push_child(parent, NodeKind::Element(element));
                if !self_closing {
                    stack.push((id, start));
                }
            }
        }
        if let Some(&(open, start)) = stack.get(1) {
            return Err(self.error(start, &format!("`<{}>` is never closed", doc.name(open))));
        }
        if doc.root().is_none() {
            return Err(self.error(0, "no root element"));
        }
        Ok(doc)
    }

    fn doctype_end(&self) -> Result<usize, ParseError> {
        let mut depth = 0usize;
        for (i, b) in self.src.as_bytes()[self.pos..].iter().enumerate() {
            match b {
                b'[' => depth += 1,
                b']' => depth = depth.saturating_sub(1),
                b'>' if depth == 0 => return Ok(self.pos + i + 1),
                _ => {}
            }
        }
        Err(self.error(self.pos, "unterminated declaration"))
    }

    fn start_tag(&mut self) -> Result<Element, ParseError> {
        let src = self.src;
        let b = src.as_bytes();
        let tag_start = self.pos;
        let mut i = self.pos + 1;
        let name_start = i;
        while i < b.len() && !is_ws(b[i]) && b[i] != b'>' && b[i] != b'/' {
            i += 1;
        }
        if i == name_start {
            return Err(self.error(tag_start, "expected an element name after `<`"));
        }
        let name = src[name_start..i].to_string();
        let mut attrs = Vec::new();
        loop {
            let ws_start = i;
            while i < b.len() && is_ws(b[i]) {
                i += 1;
            }
            let ws = &src[ws_start..i];
            let Some(&c) = b.get(i) else {
                return Err(self.error(tag_start, &format!("unterminated `<{name}` tag")));
            };
            match c {
                b'>' | b'/' => {
                    let self_closing = c == b'/';
                    if self_closing && b.get(i + 1) != Some(&b'>') {
                        return Err(self.error(i, "expected `/>`"));
                    }
                    self.pos = i + if self_closing { 2 } else { 1 };
                    return Ok(Element {
                        name,
                        attrs,
                        tag_tail: ws.to_string(),
                        self_closing,
                        end_tag: String::new(),
                    });
                }
                _ => {
                    let attr_start = i;
                    while i < b.len() && !is_ws(b[i]) && !matches!(b[i], b'=' | b'>' | b'/') {
                        i += 1;
                    }
                    let attr_name = &src[attr_start..i];
                    let eq_start = i;
                    while i < b.len() && is_ws(b[i]) {
                        i += 1;
                    }
                    if b.get(i) != Some(&b'=') {
                        return Err(self.error(i, &format!("expected `=` after `{attr_name}`")));
                    }
                    i += 1;
                    while i < b.len() && is_ws(b[i]) {
                        i += 1;
                    }
                    let eq = &src[eq_start..i];
                    let quote = match b.get(i) {
                        Some(&q @ (b'"' | b'\'')) => q,
                        _ => {
                            return Err(self
                                .error(i, &format!("expected a quoted value for `{attr_name}`")));
                        }
                    };
                    let value_start = i + 1;
                    let value_end = src[value_start..]
                        .find(quote as char)
                        .map(|j| value_start + j)
                        .ok_or_else(|| self.error(i, "unterminated attribute value"))?;
                    attrs.push(Attr {
                        leading: ws.to_string(),
                        name: attr_name.to_string(),
                        eq: eq.to_string(),
                        quote: quote as char,
                        raw_value: src[value_start..value_end].to_string(),
                    });
                    i = value_end + 1;
                }
            }
        }
    }

    fn error(&self, pos: usize, message: &str) -> ParseError {
        let before = &self.src[..pos.min(self.src.len())];
        let line = before.matches('\n').count() + 1;
        let column = before.len() - before.rfind('\n').map_or(0, |i| i + 1) + 1;
        ParseError {
            line,
            column,
            message: message.to_string(),
        }
    }
}

impl Document {
    fn push_child(&mut self, parent: NodeId, kind: NodeKind) -> NodeId {
        let id = self.push_node(kind, Some(parent));
        self.nodes[parent].children.push(id);
        id
    }
}

fn is_ws(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\r' | b'\n')
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<!-- A comment -->
<LinearLayout xmlns:android="http://schemas.android.com/apk/res/android"
    android:layout_width="match_parent"
    android:layout_height='match_parent'
    android:orientation = "vertical" >

    <TextView
        android:id="@+id/title"
        android:layout_width="wrap_content"
        android:layout_height="wrap_content"
        android:text="Tom &amp; Jerry" />

    <Button android:id="@+id/ok" android:layout_width="wrap_content"
        android:layout_height="wrap_content"/>
</LinearLayout>
"#;

    #[test]
    fn round_trips_byte_for_byte() {
        let doc = Document::parse(SAMPLE).unwrap();
        assert_eq!(doc.to_xml(), SAMPLE);
    }

    #[test]
    fn reads_attributes_unescaped() {
        let doc = Document::parse(SAMPLE).unwrap();
        let title = doc.find_by_android_id("title").unwrap();
        assert_eq!(doc.attr(title, "android:text").unwrap(), "Tom & Jerry");
        assert_eq!(doc.name(title), "TextView");
    }

    #[test]
    fn set_attr_changes_only_the_value() {
        let mut doc = Document::parse(SAMPLE).unwrap();
        let title = doc.find_by_android_id("title").unwrap();
        doc.set_attr(title, "android:text", "Hi <you>");
        let out = doc.to_xml();
        assert_eq!(out, SAMPLE.replace("Tom &amp; Jerry", "Hi &lt;you>"));
    }

    #[test]
    fn new_attributes_follow_neighbour_formatting() {
        let mut doc = Document::parse(SAMPLE).unwrap();
        let title = doc.find_by_android_id("title").unwrap();
        doc.set_attr(title, "android:layout_marginTop", "8dp");
        doc.set_attr(title, "android:textColor", "#FF0000");
        let out = doc.to_xml();
        assert!(out.contains(
            "        android:layout_height=\"wrap_content\"\n        android:layout_marginTop=\"8dp\"\n        android:text=\"Tom &amp; Jerry\"\n        android:textColor=\"#FF0000\" />"
        ), "{out}");
    }

    #[test]
    fn remove_attr_keeps_the_rest() {
        let mut doc = Document::parse(SAMPLE).unwrap();
        let title = doc.find_by_android_id("title").unwrap();
        assert!(doc.remove_attr(title, "android:text"));
        assert!(!doc.remove_attr(title, "android:text"));
        assert!(
            doc.to_xml()
                .contains("android:layout_height=\"wrap_content\" />")
        );
    }

    #[test]
    fn move_element_reindents() {
        let mut doc = Document::parse(SAMPLE).unwrap();
        let root = doc.root().unwrap();
        let ok = doc.find_by_android_id("ok").unwrap();
        doc.move_element(ok, root, 0);
        let out = doc.to_xml();
        let ok_pos = out.find("<Button").unwrap();
        let title_pos = out.find("<TextView").unwrap();
        assert!(ok_pos < title_pos);
        assert!(out.contains("vertical\" >\n\n    <Button"), "{out}");
        assert!(out.contains("wrap_content\"/>\n\n    <TextView"), "{out}");
        assert!(out.ends_with("Jerry\" />\n</LinearLayout>\n"), "{out}");
    }

    #[test]
    fn move_into_self_closing_element() {
        let src = "<FrameLayout>\n    <View a=\"1\" />\n    <LinearLayout\n        b=\"2\" />\n</FrameLayout>\n";
        let mut doc = Document::parse(src).unwrap();
        let root = doc.root().unwrap();
        let kids = doc.element_children(root);
        doc.move_element(kids[0], kids[1], 0);
        assert_eq!(
            doc.to_xml(),
            "<FrameLayout>\n    <LinearLayout\n        b=\"2\" >\n        <View a=\"1\" />\n    </LinearLayout>\n</FrameLayout>\n"
        );
    }

    #[test]
    fn create_and_insert_element() {
        let mut doc = Document::parse(SAMPLE).unwrap();
        let root = doc.root().unwrap();
        let v = doc.create_element(
            "View",
            &[
                ("android:layout_width", "10dp"),
                ("android:layout_height", "1dp"),
            ],
        );
        doc.insert_element(v, root, 2, "");
        let out = doc.to_xml();
        assert!(out.contains(
            "android:layout_height=\"wrap_content\"/>\n\n    <View\n        android:layout_width=\"10dp\"\n        android:layout_height=\"1dp\" />\n</LinearLayout>"
        ), "{out}");
        assert_eq!(Document::parse(&out).unwrap().to_xml(), out);
    }

    #[test]
    fn reorder_keeps_blank_lines_where_they_were() {
        let src = "<L>\n    <A/>\n\n    <B/>\n    <C/>\n</L>";
        let mut doc = Document::parse(src).unwrap();
        let root = doc.root().unwrap();
        let kids = doc.element_children(root);
        doc.move_element(kids[1], root, 0);
        assert_eq!(doc.to_xml(), "<L>\n    <B/>\n\n    <A/>\n    <C/>\n</L>");
        doc.move_element(kids[1], root, 2);
        assert_eq!(doc.to_xml(), "<L>\n    <A/>\n    <C/>\n\n    <B/>\n</L>");
    }

    #[test]
    fn layout_params_group_by_namespace() {
        let mut doc = Document::parse("<V\n    android:layout_width=\"1dp\"\n    app:layout_constraintTop_toTopOf=\"parent\"\n    android:text=\"x\" />").unwrap();
        let root = doc.root().unwrap();
        doc.set_attr(root, "android:layout_marginTop", "2dp");
        doc.set_attr(root, "app:layout_constraintStart_toStartOf", "parent");
        assert_eq!(
            doc.to_xml(),
            "<V\n    android:layout_width=\"1dp\"\n    android:layout_marginTop=\"2dp\"\n    app:layout_constraintTop_toTopOf=\"parent\"\n    app:layout_constraintStart_toStartOf=\"parent\"\n    android:text=\"x\" />"
        );
    }

    #[test]
    fn detach_removes_indentation() {
        let mut doc = Document::parse(SAMPLE).unwrap();
        let ok = doc.find_by_android_id("ok").unwrap();
        doc.detach(ok);
        assert!(doc.to_xml().ends_with("Jerry\" />\n</LinearLayout>\n"));
        assert!(!doc.is_attached(ok));
    }

    #[test]
    fn reports_errors_with_positions() {
        let err = Document::parse("<A>\n  <B>\n</A>").unwrap_err();
        assert_eq!((err.line, err.column), (3, 1));
        assert!(Document::parse("<A x=1/>").is_err());
        assert!(Document::parse("<A/><B/>").is_err());
    }

    #[test]
    fn ensure_namespace_adds_once() {
        let mut doc = Document::parse(SAMPLE).unwrap();
        doc.ensure_namespace("app", APP_NS);
        doc.ensure_namespace("app", APP_NS);
        let out = doc.to_xml();
        assert_eq!(out.matches("xmlns:app").count(), 1);
        assert!(out.contains(
            "<LinearLayout xmlns:android=\"http://schemas.android.com/apk/res/android\"\n    xmlns:app=\"http://schemas.android.com/apk/res-auto\"\n    android:layout_width"
        ), "{out}");
    }
}
