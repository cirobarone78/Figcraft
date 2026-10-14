use figcraft_doc::{Document, NodeKind};

use crate::export_panel::slice_preview_pixmap;

#[test]
fn slice_preview_renders_page_content_in_its_bounds() {
    let mut doc = Document::new("Preview");
    let Some(page) = doc.first_page() else { return };
    let rectangle = doc.create(page, NodeKind::Rectangle, "Artwork", 10.0, 10.0, 20.0, 20.0);
    assert!(rectangle.is_ok());
    let Ok(_rectangle) = rectangle else { return };
    let slice = doc.create(page, NodeKind::Slice, "Slice", 0.0, 0.0, 40.0, 40.0);
    assert!(slice.is_ok());
    let Ok(slice) = slice else { return };

    let pixmap = slice_preview_pixmap(&doc, slice, 40.0);
    assert!(pixmap.is_some(), "a slice should produce a preview image");
    let Some(pixmap) = pixmap else { return };
    assert_eq!((pixmap.width(), pixmap.height()), (40, 40));
    assert!(pixmap.pixel(20, 20).is_some_and(|pixel| pixel.alpha() > 0), "artwork under the slice should be visible in its preview");
}
