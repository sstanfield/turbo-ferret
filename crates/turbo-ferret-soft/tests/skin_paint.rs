//! M3: the default skin loads; skin painting through the paint walk (golden images);
//! context-loss round trip. Bless with `BLESS=1 cargo test -p turbo-ferret-soft`.

mod common;

use common::assert_golden;
use turbo_ferret::geom::{Color, Rect, Size};
use turbo_ferret::layout::size::Visibility;
use turbo_ferret::widgets::{Layout, Plain};
use turbo_ferret::*;
use turbo_ferret_soft::SoftRenderer;

const BG: Color = Color::rgb(40, 44, 52);

fn resources() -> String {
    format!("{}/../../resources", env!("CARGO_MANIFEST_DIR"))
}

fn ui_with_skin(dpi: i32) -> Ui {
    let mut ui = Ui::new(Config::for_dpi(dpi), Services::with_loader(FsLoader::new(resources())));
    ui.services_mut().log = None;
    ui.load_skin("default_skin/skin.tb.txt", None).expect("default skin loads with every bitmap");
    ui
}

#[test]
fn default_skin_loads_every_bitmap() {
    for dpi in [96, 192, 288, 384] {
        let ui = ui_with_skin(dpi);
        let skin = ui.skin_data();
        assert!(skin.len() > 80, "{} elements", skin.len());
        for e in skin.elements() {
            if !e.bitmap_file.is_empty() {
                assert!(e.bitmap.is_some(), "dpi {dpi}: {} has no bitmap", e.name);
            }
        }
        assert_eq!(skin.dimension_converter().dst_dpi, dpi, "nearest supported dpi");
    }
    // Clone and padding: TBButton has padding 6 8 (top/bottom 6, left/right 8).
    let ui = ui_with_skin(96);
    let b = ui.skin_data().element(tbid!("TBButton")).unwrap();
    assert_eq!(b.padding, turbo_ferret::geom::Edges::new(8, 6, 8, 6));
    assert_eq!(b.cut, 17);
    // At 192 dpi the @192 bitmap is used where one exists, and cut/expand scale.
    let ui2 = ui_with_skin(192);
    let c1 = ui.skin_data().element(tbid!("TBCheckBox")).unwrap();
    let c2 = ui2.skin_data().element(tbid!("TBCheckBox")).unwrap();
    assert_eq!((c1.cut, c2.cut), (19, 38));
    assert_eq!(c2.bitmap.unwrap().rect.w, c1.bitmap.unwrap().rect.w * 2);
    // Without a @192 file the 96 dpi bitmap and its cut are kept (TB).
    let b2 = ui2.skin_data().element(tbid!("TBButton")).unwrap();
    assert_eq!(b2.cut, 17);
    assert_eq!(b2.padding, turbo_ferret::geom::Edges::new(16, 12, 16, 12), "dp padding converts");
}

/// A widget that clips its children to its padding rect.
#[derive(Default)]
struct Clipper;
impl Widget for Clipper {
    fn type_name(&self) -> &'static str {
        "Clipper"
    }
    fn children_clip(&self, padding: Rect) -> Option<Rect> {
        Some(padding)
    }
}

fn add_skinned(ui: &mut Ui, parent: WidgetId, skin: &str, r: Rect) -> WidgetId {
    let w = ui.add(parent, Plain);
    ui.set_skin(w, TbId::new(skin));
    ui.set_rect(w, r);
    w
}

fn scene() -> Ui {
    let mut ui = ui_with_skin(96);
    ui.set_viewport(Size::new(400, 220));
    let root = ui.root();
    // Nine-slice in its normal and pressed states.
    add_skinned(&mut ui, root, "TBButton", Rect::new(10, 10, 100, 32));
    let pressed = add_skinned(&mut ui, root, "TBButton", Rect::new(120, 10, 100, 32));
    ui.set_state(pressed, WidgetState::PRESSED, true);
    // Override by condition: a button in a "button_group" parent with axis x.
    let group = ui.add(root, Layout::new(Axis::X));
    ui.set_skin(group, tbid!("button_group"));
    ui.set_rect(group, Rect::new(230, 10, 160, 32));
    for i in 0..2 {
        add_skinned(&mut ui, group, "TBButton", Rect::new(i * 80, 0, 80, 32));
    }
    // Children elements: the flat button paints its hovered child when hovered.
    let flat = add_skinned(&mut ui, root, "TBButton.flat", Rect::new(10, 52, 100, 32));
    ui.set_state(flat, WidgetState::HOVERED, true);
    // Tile + children + overlays.
    add_skinned(&mut ui, root, "TBSectionHeader", Rect::new(120, 52, 160, 28));
    // Stretch border and an image element.
    add_skinned(&mut ui, root, "TBContainer", Rect::new(290, 52, 100, 60));
    add_skinned(&mut ui, root, "TBWindow.close", Rect::new(10, 94, 24, 24));
    // Window mover: override on "selected".
    let mover = add_skinned(&mut ui, root, "TBWindow.mover", Rect::new(40, 94, 120, 24));
    ui.set_state(mover, WidgetState::SELECTED, true);
    // Opacity and the disabled factor.
    let half = add_skinned(&mut ui, root, "TBButton", Rect::new(170, 90, 100, 32));
    ui.set_opacity(half, 0.5);
    let dis = add_skinned(&mut ui, root, "TBButton", Rect::new(10, 128, 100, 32));
    ui.set_disabled(dis, true);
    // Clip: a child larger than its clipping parent's padding rect.
    let clip = ui.add(root, Clipper);
    ui.set_skin(clip, tbid!("TBContainer"));
    ui.set_rect(clip, Rect::new(120, 128, 120, 60));
    add_skinned(&mut ui, clip, "TBButton", Rect::new(-20, 10, 200, 40));
    // Invisible widgets are not painted.
    let gone = add_skinned(&mut ui, root, "TBButton", Rect::new(250, 128, 100, 32));
    ui.set_visibility(gone, Visibility::Invisible);
    ui.update();
    ui
}

fn render(ui: &mut Ui, r: &mut SoftRenderer) {
    r.clear(BG);
    ui.paint(r);
}

#[test]
fn skin_scene_golden() {
    let mut ui = scene();
    let mut r = SoftRenderer::new(Size::new(400, 220));
    render(&mut ui, &mut r);
    assert_golden("skin_scene", &r);
}

#[test]
fn context_loss_round_trip() {
    let mut ui = scene();
    let mut a = SoftRenderer::new(Size::new(400, 220));
    render(&mut ui, &mut a);
    let created = a.stats().bitmaps_created;
    assert!(created > 0);
    // Repainting does not recreate bitmaps.
    render(&mut ui, &mut a);
    assert_eq!(a.stats().bitmaps_created, created);
    // Lose the context: a new renderer with no bitmaps; the next paint recreates them.
    ui.renderer_lost();
    let mut b = SoftRenderer::new(Size::new(400, 220));
    render(&mut ui, &mut b);
    assert!(b.stats().bitmaps_created > 0);
    assert_eq!(a.pixels(), b.pixels(), "same image after context loss");
}

#[test]
fn skin_padding_and_sizes_drive_measure() {
    let mut ui = ui_with_skin(96);
    let root = ui.root();
    let w = ui.add(root, Plain);
    ui.set_skin(w, tbid!("TBButton"));
    // No content: preferred = intrinsic bitmap size minus expand (TB).
    let ps = ui.preferred_size_free(w);
    let e = ui.skin_data().element(tbid!("TBButton")).unwrap();
    let iw = e.intrinsic_width().unwrap();
    assert_eq!(ps.pref.w, iw);
    assert_eq!(ui.padding(w), e.padding);
    assert_eq!(ui.padding_rect(w), Rect::new(0, 0, 0, 0).shrink_edges(e.padding));
}
