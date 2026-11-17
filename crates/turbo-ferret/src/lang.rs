//! Language string tables.
//!
//! TB: tb_language.h, tb_language.cpp (TBLanguage). A language file is a flat resource
//! file of `key text` lines. Resources refer to a string with `@key` (for example
//! `TBButton: text: @close`); [`Lookup::str`](crate::res::Lookup::str) and
//! [`Language::resolve`] translate those.

use alloc::borrow::Cow;
use alloc::collections::BTreeMap;
use alloc::rc::Rc;

use crate::id::TbId;
use crate::res::{ParseError, ResNode};
use crate::services::ResourceLoader;

/// What a missing string becomes (without the `debug` feature).
pub const MISSING: &str = "<TRANSLATE!>";

/// A table of strings by `TbId`. TB: TBLanguage
#[derive(Clone, Debug, Default)]
pub struct Language {
    strings: BTreeMap<TbId, Rc<str>>,
}

impl Language {
    pub fn new() -> Self {
        Self::default()
    }

    /// Add the strings of a language file read through `loader`. Existing strings stay;
    /// a key read again is replaced. TB: tb_language.cpp TBLanguage::Load
    pub fn load(&mut self, loader: &mut dyn ResourceLoader, path: &str) -> Result<(), ParseError> {
        self.load_with_refs(loader, path, None)
    }

    /// [`load`](Self::load), with the ref trees that the file's `@if` conditions and
    /// `@include @tree>...` lines may use.
    pub fn load_with_refs(
        &mut self,
        loader: &mut dyn ResourceLoader,
        path: &str,
        refs: Option<&crate::res::RefTrees>,
    ) -> Result<(), ParseError> {
        let node = ResNode::load(loader, path, refs)?;
        self.add_node(&node);
        Ok(())
    }

    /// Add the strings of language file text.
    pub fn load_str(&mut self, src: &str) -> Result<(), ParseError> {
        let node = ResNode::parse_str(src)?;
        self.add_node(&node);
        Ok(())
    }

    /// Add each child of `node` (name = key, value = text).
    // TB: tb_language.cpp TBLanguage::Load
    pub fn add_node(&mut self, node: &ResNode) {
        for n in node {
            self.strings.insert(TbId::new(&n.name), Rc::from(&*n.value.as_str()));
        }
    }

    /// Add or replace one string.
    pub fn set(&mut self, key: &str, text: &str) {
        self.strings.insert(TbId::new(key), Rc::from(text));
    }

    /// TB: tb_language.cpp TBLanguage::Clear
    pub fn clear(&mut self) {
        self.strings.clear();
    }

    pub fn len(&self) -> usize {
        self.strings.len()
    }

    pub fn is_empty(&self) -> bool {
        self.strings.is_empty()
    }

    /// The string for `id`, if there is one.
    pub fn try_get(&self, id: TbId) -> Option<&str> {
        self.strings.get(&id).map(|s| &**s)
    }

    /// The string for `id`, or [`MISSING`] (with the `debug` feature, `<TRANSLATE:id>`
    /// with the id in hex, built for this call). TB: tb_language.cpp TBLanguage::GetString
    pub fn get(&self, id: TbId) -> Cow<'_, str> {
        match self.try_get(id) {
            Some(s) => Cow::Borrowed(s),
            #[cfg(feature = "debug")]
            None => Cow::Owned(alloc::format!("<TRANSLATE:{:#010x}>", id.0)),
            #[cfg(not(feature = "debug"))]
            None => Cow::Borrowed(MISSING),
        }
    }

    /// The string for `key`, or [`MISSING`] (with the `debug` feature, `<TRANSLATE:key>`,
    /// built for this call rather than kept in a static as TB did, H-04).
    pub fn get_key(&self, key: &str) -> Cow<'_, str> {
        match self.try_get(TbId::new(key)) {
            Some(s) => Cow::Borrowed(s),
            #[cfg(feature = "debug")]
            None => Cow::Owned(alloc::format!("<TRANSLATE:{key}>")),
            #[cfg(not(feature = "debug"))]
            None => Cow::Borrowed(MISSING),
        }
    }

    /// Translate a resource string: `"@key"` (with no `>`, which would make it a node
    /// reference) becomes the string for `key`; anything else is returned as it is.
    /// TB: tb_node_tree.cpp TBNode::GetValueString (the language part)
    pub fn resolve<'a>(&'a self, s: &'a str) -> Cow<'a, str> {
        match s.strip_prefix('@') {
            Some(key) if !key.contains('>') => self.get_key(key),
            _ => Cow::Borrowed(s),
        }
    }
}
