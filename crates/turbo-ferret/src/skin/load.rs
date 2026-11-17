//! Loading a skin file and its bitmaps. TB: TBSkin::Load, LoadInternal,
//! ReloadBitmaps, TBSkinElement::Load, TBSkinElementStateList::Load

use alloc::string::String;
use alloc::vec::Vec;

use crate::geom::{Color, Edges, Point, Size};
use crate::id::TbId;
use crate::layout::dim::DimensionConverter;
use crate::res::{ResNode, unescape};
use crate::services::{ImageDecoder, ResourceLoader};
use crate::value::Value;
use crate::widget::{SkinState, WidgetState};

use super::condition::{CondProp, CondTarget, Condition, Test, state_value};
use super::element::{SkinElement, SkinKind, StateElement};
use super::{Skin, SkinError};

fn s(n: &ResNode, path: &str) -> Option<String> {
    n.get(path).map(|c| String::from(&*c.value.as_str()))
}

impl Skin {
    /// Load a skin file (and an optional override skin on top), then its bitmaps.
    /// `screen_dpi` picks the closest `supported-dpi`. TB: TBSkin::Load
    pub fn load(
        &mut self,
        loader: &mut dyn ResourceLoader,
        images: &mut dyn ImageDecoder,
        path: &str,
        override_path: Option<&str>,
        screen_dpi: i32,
    ) -> Result<(), SkinError> {
        self.load_internal(loader, path, screen_dpi)?;
        if let Some(o) = override_path {
            self.load_internal(loader, o, screen_dpi)?;
        }
        self.reload_bitmaps(loader, images)
    }

    /// TB: TBSkin::LoadInternal
    fn load_internal(&mut self, loader: &mut dyn ResourceLoader, path: &str, screen_dpi: i32) -> Result<(), SkinError> {
        let mut node = ResNode::load(loader, path, None)?;
        if node.get("description").is_some() {
            let base = node.int("description>base-dpi", 96);
            let mut supported = base;
            if let Some(sn) = node.get("description>supported-dpi") {
                if let Some(arr) = sn.value.as_array() {
                    let mut best = 0;
                    for v in arr {
                        let c = v.as_int();
                        if best == 0 || (c - screen_dpi).abs() < (best - screen_dpi).abs() {
                            best = c;
                        }
                    }
                    if best != 0 {
                        supported = best;
                    }
                }
            }
            self.dim = DimensionConverter::new(base, supported, screen_dpi);
        }
        if let Some(c) = s(&node, "defaults>text-color") {
            self.defaults.text_color = Color::parse(&c);
        }
        self.defaults.disabled_opacity = node.float("defaults>disabled>opacity", self.defaults.disabled_opacity);
        self.defaults.placeholder_opacity =
            node.float("defaults>placeholder>opacity", self.defaults.placeholder_opacity);
        if let Some(v) = node.get("defaults>spacing") {
            self.defaults.spacing = self.dim.px_from_value(&v.value, self.defaults.spacing);
        }
        let Some(elements) = node.get_mut("elements") else { return Ok(()) };
        // Resolve `clone` (copies the source element's children; own values win since
        // lookups take the first match). TB: LoadInternal clone loop
        let snapshot = elements.clone();
        for n in elements.children.iter_mut() {
            let mut guard = 0;
            while let Some(i) = n.children.iter().position(|c| &*c.name == "clone") {
                let src_name = n.children[i].value.as_str().into_owned();
                n.children.remove(i);
                if let Some(src) = snapshot.children.iter().find(|c| *c.name == *src_name) {
                    n.clone_children_from(src);
                }
                guard += 1;
                if guard > 16 {
                    break;
                }
            }
        }
        let elements = node.get("elements").unwrap();
        for n in elements.children() {
            let id = TbId::new(&n.name);
            let mut e = self.elements.remove(&id).unwrap_or_else(|| SkinElement::new(&n.name));
            self.load_element(&mut e, n, loader, path);
            self.elements.insert(id, e);
        }
        Ok(())
    }

    /// TB: TBSkinElement::Load
    fn load_element(&self, e: &mut SkinElement, n: &ResNode, loader: &mut dyn ResourceLoader, skin_path: &str) {
        let dc = &self.dim;
        if let Some(b) = s(n, "bitmap") {
            e.bitmap_file = loader.resolve(skin_path, &b);
        }
        e.cut = n.int("cut", e.cut);
        e.expand = n.int("expand", e.expand);
        if let Some(p) = n.get("padding") {
            let px = |v: &Value| dc.px_from_value(v, 0);
            e.padding = match p.value.as_array() {
                Some(a) if a.len() == 4 => Edges::new(px(&a[3]), px(&a[0]), px(&a[1]), px(&a[2])),
                Some(a) if a.len() == 2 => Edges::new(px(&a[1]), px(&a[0]), px(&a[1]), px(&a[0])),
                _ => Edges::all(px(&p.value)),
            };
        }
        let opt = |path: &str, cur: Option<i32>| n.get(path).map(|v| dc.px_from_value(&v.value, 0)).or(cur);
        let px = |path: &str, cur: i32| n.get(path).map(|v| dc.px_from_value(&v.value, cur)).unwrap_or(cur);
        e.sizes.width = opt("width", e.sizes.width);
        e.sizes.height = opt("height", e.sizes.height);
        e.sizes.pref_width = opt("pref-width", e.sizes.pref_width);
        e.sizes.pref_height = opt("pref-height", e.sizes.pref_height);
        e.sizes.min_width = opt("min-width", e.sizes.min_width);
        e.sizes.min_height = opt("min-height", e.sizes.min_height);
        e.sizes.max_width = opt("max-width", e.sizes.max_width);
        e.sizes.max_height = opt("max-height", e.sizes.max_height);
        e.spacing = opt("spacing", e.spacing);
        e.content_ofs = Point::new(px("content-ofs-x", e.content_ofs.x), px("content-ofs-y", e.content_ofs.y));
        e.img_position =
            Point::new(n.int("img-position-x", e.img_position.x), n.int("img-position-y", e.img_position.y));
        e.img_ofs = Point::new(px("img-ofs-x", e.img_ofs.x), px("img-ofs-y", e.img_ofs.y));
        e.flip_x = n.int("flip-x", e.flip_x as i32) != 0;
        e.flip_y = n.int("flip-y", e.flip_y as i32) != 0;
        e.opacity = n.float("opacity", e.opacity);
        if let Some(c) = s(n, "text-color") {
            e.text_color = Some(Color::parse(&c));
        }
        if let Some(c) = s(n, "background-color") {
            e.bg_color = Some(Color::parse(&c));
        }
        if let Some(t) = s(n, "type") {
            e.kind = SkinKind::parse(&t);
        }
        if let Some(t) = n.get("tag") {
            e.tag = t.value.clone();
        }
        load_states(&mut e.overrides, n.get("overrides"));
        load_states(&mut e.strong_overrides, n.get("strong-overrides"));
        load_states(&mut e.children, n.get("children"));
        load_states(&mut e.overlays, n.get("overlays"));
    }

    /// Load every element's bitmap into the atlas (choosing the DPI variant).
    /// TB: TBSkin::ReloadBitmaps / ReloadBitmapsInternal
    pub fn reload_bitmaps(
        &mut self,
        loader: &mut dyn ResourceLoader,
        images: &mut dyn ImageDecoder,
    ) -> Result<(), SkinError> {
        self.atlas.clear();
        self.files.clear();
        let mut missing = Vec::new();
        let ids: Vec<TbId> = self.elements.keys().copied().collect();
        for id in ids {
            let file = self.elements[&id].bitmap_file.clone();
            if file.is_empty() {
                self.elements.get_mut(&id).unwrap().bitmap = None;
                continue;
            }
            let mut bitmap_dpi = self.dim.src_dpi;
            let mut frag = None;
            if self.dim.need_conversion() {
                frag = self.fragment_from_file(loader, images, &self.dim.dst_dpi_filename(&file));
                if frag.is_some() {
                    bitmap_dpi = self.dim.dst_dpi;
                }
            }
            if frag.is_none() {
                frag = self.fragment_from_file(loader, images, &file);
            }
            let dim = self.dim.clone();
            let e = self.elements.get_mut(&id).unwrap();
            set_bitmap_dpi(e, &dim, bitmap_dpi);
            e.bitmap = frag;
            if frag.is_none() {
                missing.push(file);
            }
        }
        if missing.is_empty() { Ok(()) } else { Err(SkinError::MissingBitmaps(missing)) }
    }

    /// Load (or reuse) a bitmap file as a premultiplied atlas fragment.
    /// TB: TBBitmapFragmentManager::GetFragmentFromFile
    fn fragment_from_file(
        &mut self,
        loader: &mut dyn ResourceLoader,
        images: &mut dyn ImageDecoder,
        file: &str,
    ) -> Option<crate::render::atlas::FragmentId> {
        if let Some(f) = self.files.get(file) {
            return Some(*f);
        }
        let bytes = loader.load(file).ok()?;
        let mut img = images.decode(&bytes).ok()?;
        premultiply(&mut img.rgba);
        let f = self.atlas.add(Size::new(img.size.w, img.size.h), &img.rgba)?;
        self.files.insert(String::from(file), f);
        Some(f)
    }
}

/// Straight alpha RGBA8 to premultiplied, in place.
pub fn premultiply(rgba: &mut [u8]) {
    for px in rgba.chunks_exact_mut(4) {
        let a = px[3] as u32;
        if a == 255 {
            continue;
        }
        for c in &mut px[..3] {
            *c = ((*c as u32 * a + 127) / 255) as u8;
        }
    }
}

/// TB: TBSkinElement::SetBitmapDPI (cut and expand scale with a high-DPI bitmap).
fn set_bitmap_dpi(e: &mut SkinElement, dc: &DimensionConverter, bitmap_dpi: i32) {
    if e.bitmap_dpi != 0 {
        return;
    }
    if dc.need_conversion() && bitmap_dpi == dc.dst_dpi {
        e.expand = e.expand * dc.dst_dpi / dc.src_dpi;
        e.cut = e.cut * dc.dst_dpi / dc.src_dpi;
    }
    e.bitmap_dpi = bitmap_dpi;
}

/// TB: TBSkinElementStateList::Load (appends).
fn load_states(list: &mut Vec<StateElement>, n: Option<&ResNode>) {
    let Some(n) = n else { return };
    for en in n.children() {
        let mut st =
            StateElement { element: TbId::new(&en.value.as_str()), state: SkinState::ALL, conditions: Vec::new() };
        for c in en.children() {
            match &*c.name {
                "state" => st.state = WidgetState::parse(&c.value.as_str()),
                "condition" => {
                    let target = CondTarget::parse(&c.str("target", ""));
                    let prop_s = c.str("property", "").into_owned();
                    let prop = CondProp::parse(&prop_s);
                    let value = match c.get("value") {
                        Some(v) if prop == CondProp::State => state_value(&v.value.as_str()),
                        Some(v) => match &v.value {
                            Value::Str(s) => TbId::new(&unescape(s)),
                            other => TbId(other.as_int() as u32),
                        },
                        None => TbId::NONE,
                    };
                    let test = if c.str("test", "") == "!=" { Test::NotEqual } else { Test::Equal };
                    st.conditions.push(Condition { target, prop, value, test });
                }
                _ => {}
            }
        }
        list.push(st);
    }
}
