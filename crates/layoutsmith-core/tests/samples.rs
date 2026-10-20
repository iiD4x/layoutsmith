//! End-to-end checks against the sample project in `samples/`.

use std::path::{Path, PathBuf};

use layoutsmith_core::edit;
use layoutsmith_core::layout::{ApproxTextMeasurer, DEFAULT_DENSITY, layout};
use layoutsmith_core::{Align, Document, LayoutResult, Resources};

fn sample(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../samples/res/layout")
        .join(name)
}

fn load(name: &str) -> (String, Document, Resources) {
    let path = sample(name);
    let text = std::fs::read_to_string(&path).unwrap();
    let doc = Document::parse(&text).unwrap();
    (text, doc, Resources::for_layout(&path))
}

fn lay(doc: &Document, res: &Resources) -> LayoutResult {
    layout(
        doc,
        res,
        &ApproxTextMeasurer,
        [360.0, 800.0],
        DEFAULT_DENSITY,
    )
}

#[test]
fn samples_round_trip_unchanged() {
    for name in ["activity_profile.xml", "item_contact.xml"] {
        let (text, doc, _) = load(name);
        assert_eq!(doc.to_xml(), text, "{name}");
    }
}

#[test]
fn profile_lays_out_with_project_resources() {
    let (_, doc, res) = load("activity_profile.xml");
    assert!(res.colors.contains_key("primary"));
    let l = lay(&doc, &res);
    let rect = |id: &str| l.rect(doc.find_by_android_id(id).unwrap()).unwrap();

    let avatar = rect("avatar");
    assert_eq!((avatar.x, avatar.y, avatar.w), (132.0, 132.0, 96.0));
    // @dimen/gap below the avatar.
    assert_eq!(rect("name").y, avatar.bottom() + 12.0);
    // Two weighted buttons share the row inside @dimen/screen_padding margins.
    let (follow, message) = (rect("follow"), rect("message"));
    assert_eq!(follow.x, 16.0);
    assert_eq!(message.right(), 344.0);
    assert!((follow.w - message.w).abs() < 0.01);
    assert!((follow.right() + 8.0 - message.x).abs() < 0.01);
    // The three stats columns are equally wide.
    let posts = rect("posts");
    assert!((posts.w - rect("following").w).abs() < 0.01);
}

#[test]
fn contact_item_relative_rules() {
    let (_, doc, res) = load("item_contact.xml");
    let l = lay(&doc, &res);
    let rect = |id: &str| l.rect(doc.find_by_android_id(id).unwrap()).unwrap();
    let (photo, name, role, call) = (
        rect("photo"),
        rect("contact_name"),
        rect("contact_role"),
        rect("call"),
    );
    assert_eq!(photo.x, 16.0);
    assert_eq!(name.x, photo.right() + 12.0);
    assert_eq!(role.x, name.x);
    assert_eq!(role.y, name.bottom());
    assert_eq!(call.right(), 344.0);
    // The wrap_content row is as tall as its tallest child plus padding.
    let root = l.rect(doc.root().unwrap()).unwrap();
    assert_eq!(root.h, call.h.max(name.h + role.h).max(photo.h) + 32.0);
}

#[test]
fn edits_touch_only_what_they_change() {
    let (text, mut doc, res) = load("activity_profile.xml");
    let l = lay(&doc, &res);
    let notify = doc.find_by_android_id("notify").unwrap();
    edit::move_by(&mut doc, &res, &l, notify, [8.0, 0.0]);
    let after = doc.to_xml();
    assert_eq!(
        after,
        text.replace(
            "        android:layout_marginStart=\"@dimen/screen_padding\"\n        android:text=\"@string/notify\"",
            "        android:layout_marginStart=\"24dp\"\n        android:text=\"@string/notify\""
        )
    );

    // Center the name horizontally (it already is) and then pin it to the end.
    let name = doc.find_by_android_id("name").unwrap();
    let l = lay(&doc, &res);
    edit::align(&mut doc, &res, &l, name, 0, Align::End);
    assert!(!doc.has_attr(name, "app:layout_constraintStart_toStartOf"));
    let l = lay(&doc, &res);
    assert_eq!(l.rect(name).unwrap().right(), 360.0);

    // Everything still parses and round-trips.
    let xml = doc.to_xml();
    assert_eq!(Document::parse(&xml).unwrap().to_xml(), xml);
}
