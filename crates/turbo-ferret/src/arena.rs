//! Generational arena, typed keys and dense side tables.
//!
//! Replaces TB's intrusive lists and raw pointers (H-01, H-03): a key is a weak
//! reference, and a stale key fails lookup safely.

use alloc::vec::Vec;
use core::fmt;
use core::marker::PhantomData;
use core::num::NonZeroU32;

/// A key type for an [`Arena`].
pub trait ArenaKey: Copy + Eq {
    fn from_parts(index: u32, generation: NonZeroU32) -> Self;
    fn index(self) -> u32;
    fn generation(self) -> NonZeroU32;
}

macro_rules! arena_key {
    ($(#[$m:meta])* $name:ident) => {
        $(#[$m])*
        #[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash)]
        pub struct $name {
            index: u32,
            generation: NonZeroU32,
        }
        impl ArenaKey for $name {
            fn from_parts(index: u32, generation: NonZeroU32) -> Self {
                $name { index, generation }
            }
            fn index(self) -> u32 {
                self.index
            }
            fn generation(self) -> NonZeroU32 {
                self.generation
            }
        }
        impl fmt::Debug for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "{}({}v{})", stringify!($name), self.index, self.generation)
            }
        }
    };
}
#[allow(unused_imports)]
pub(crate) use arena_key;

arena_key!(
    /// Storage handle of a widget node; the weak reference used everywhere
    /// (replaces `TBWidget*` and `TBWidgetSafePointer`). `Option<WidgetId>` is 8 bytes.
    WidgetId
);

enum Slot<T> {
    Occupied {
        generation: NonZeroU32,
        value: T,
    },
    Vacant {
        generation: NonZeroU32,
        next_free: u32,
    },
    /// Generation would wrap: never reused, so an old key can never match again.
    Retired,
}

const NONE: u32 = u32::MAX;

pub struct Arena<T, K: ArenaKey = WidgetId> {
    slots: Vec<Slot<T>>,
    free_head: u32,
    len: u32,
    _k: PhantomData<K>,
}

impl<T, K: ArenaKey> Default for Arena<T, K> {
    fn default() -> Self {
        Arena { slots: Vec::new(), free_head: NONE, len: 0, _k: PhantomData }
    }
}

impl<T, K: ArenaKey> Arena<T, K> {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn len(&self) -> usize {
        self.len as usize
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Number of slots ever allocated (side tables size to this).
    pub fn capacity_slots(&self) -> usize {
        self.slots.len()
    }

    pub fn insert(&mut self, value: T) -> K {
        self.len += 1;
        if self.free_head != NONE {
            let index = self.free_head;
            let slot = &mut self.slots[index as usize];
            let Slot::Vacant { generation, next_free } = *slot else {
                unreachable!("free list points at a non-vacant slot")
            };
            self.free_head = next_free;
            *slot = Slot::Occupied { generation, value };
            K::from_parts(index, generation)
        } else {
            let index = self.slots.len() as u32;
            assert!(index != NONE, "arena full");
            let generation = NonZeroU32::MIN;
            self.slots.push(Slot::Occupied { generation, value });
            K::from_parts(index, generation)
        }
    }

    /// Remove and return the value; bumps the slot's generation.
    pub fn remove(&mut self, key: K) -> Option<T> {
        let i = key.index() as usize;
        match self.slots.get(i) {
            Some(Slot::Occupied { generation, .. }) if *generation == key.generation() => {}
            _ => return None,
        }
        let next = key.generation().get().checked_add(1).and_then(NonZeroU32::new);
        let new_slot = match next {
            Some(generation) => Slot::Vacant { generation, next_free: self.free_head },
            None => Slot::Retired,
        };
        let old = core::mem::replace(&mut self.slots[i], new_slot);
        if next.is_some() {
            self.free_head = i as u32;
        }
        self.len -= 1;
        match old {
            Slot::Occupied { value, .. } => Some(value),
            _ => unreachable!(),
        }
    }

    pub fn contains(&self, key: K) -> bool {
        self.get(key).is_some()
    }

    pub fn get(&self, key: K) -> Option<&T> {
        match self.slots.get(key.index() as usize) {
            Some(Slot::Occupied { generation, value }) if *generation == key.generation() => Some(value),
            _ => None,
        }
    }

    pub fn get_mut(&mut self, key: K) -> Option<&mut T> {
        match self.slots.get_mut(key.index() as usize) {
            Some(Slot::Occupied { generation, value }) if *generation == key.generation() => Some(value),
            _ => None,
        }
    }

    pub fn iter(&self) -> impl Iterator<Item = (K, &T)> {
        self.slots.iter().enumerate().filter_map(|(i, s)| match s {
            Slot::Occupied { generation, value } => Some((K::from_parts(i as u32, *generation), value)),
            _ => None,
        })
    }

    pub fn iter_mut(&mut self) -> impl Iterator<Item = (K, &mut T)> {
        self.slots.iter_mut().enumerate().filter_map(|(i, s)| match s {
            Slot::Occupied { generation, value } => Some((K::from_parts(i as u32, *generation), value)),
            _ => None,
        })
    }

    pub fn keys(&self) -> impl Iterator<Item = K> + '_ {
        self.iter().map(|(k, _)| k)
    }

    /// Test hook: force a slot's generation (to exercise retirement).
    #[cfg(test)]
    fn set_generation(&mut self, index: u32, g: NonZeroU32) {
        match &mut self.slots[index as usize] {
            Slot::Occupied { generation, .. } | Slot::Vacant { generation, .. } => *generation = g,
            Slot::Retired => {}
        }
    }
}

/// Optional per-widget data ("components"), dense by slot index and generation-checked.
pub struct SideTable<T, K: ArenaKey = WidgetId> {
    slots: Vec<Option<(NonZeroU32, T)>>,
    _k: PhantomData<K>,
}

impl<T, K: ArenaKey> Default for SideTable<T, K> {
    fn default() -> Self {
        SideTable { slots: Vec::new(), _k: PhantomData }
    }
}

impl<T, K: ArenaKey> SideTable<T, K> {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn get(&self, key: K) -> Option<&T> {
        match self.slots.get(key.index() as usize) {
            Some(Some((g, v))) if *g == key.generation() => Some(v),
            _ => None,
        }
    }

    pub fn get_mut(&mut self, key: K) -> Option<&mut T> {
        match self.slots.get_mut(key.index() as usize) {
            Some(Some((g, v))) if *g == key.generation() => Some(v),
            _ => None,
        }
    }

    pub fn contains(&self, key: K) -> bool {
        self.get(key).is_some()
    }

    /// Insert, returning the previous value for this same key (not for a stale one).
    pub fn insert(&mut self, key: K, v: T) -> Option<T> {
        let i = key.index() as usize;
        if self.slots.len() <= i {
            self.slots.resize_with(i + 1, || None);
        }
        let old = self.slots[i].replace((key.generation(), v));
        match old {
            Some((g, v)) if g == key.generation() => Some(v),
            _ => None,
        }
    }

    pub fn get_or_insert_with(&mut self, key: K, f: impl FnOnce() -> T) -> &mut T {
        if !self.contains(key) {
            self.insert(key, f());
        }
        self.get_mut(key).unwrap()
    }

    pub fn remove(&mut self, key: K) -> Option<T> {
        let slot = self.slots.get_mut(key.index() as usize)?;
        match slot {
            Some((g, _)) if *g == key.generation() => slot.take().map(|(_, v)| v),
            _ => None,
        }
    }

    pub fn iter(&self) -> impl Iterator<Item = (K, &T)> {
        self.slots.iter().enumerate().filter_map(|(i, s)| s.as_ref().map(|(g, v)| (K::from_parts(i as u32, *g), v)))
    }

    pub fn clear(&mut self) {
        self.slots.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn insert_get_remove() {
        let mut a: Arena<&str> = Arena::new();
        let x = a.insert("x");
        let y = a.insert("y");
        assert_eq!(a.len(), 2);
        assert_eq!(a.get(x), Some(&"x"));
        assert_eq!(a.remove(x), Some("x"));
        assert_eq!(a.get(x), None);
        assert_eq!(a.remove(x), None);
        assert_eq!(a.get(y), Some(&"y"));
        assert_eq!(a.len(), 1);
    }

    #[test]
    fn stale_id_after_reuse() {
        let mut a: Arena<u32> = Arena::new();
        let x = a.insert(1);
        a.remove(x);
        let z = a.insert(2);
        assert_eq!(z.index(), x.index(), "slot reused");
        assert_ne!(z, x);
        assert_eq!(a.get(x), None, "old generation fails");
        assert_eq!(a.get(z), Some(&2));
        *a.get_mut(z).unwrap() = 3;
        assert_eq!(a.get(z), Some(&3));
    }

    #[test]
    fn generation_wrap_retires_slot() {
        let mut a: Arena<u32> = Arena::new();
        let x = a.insert(1);
        a.set_generation(x.index(), NonZeroU32::MAX);
        let x = WidgetId::from_parts(x.index(), NonZeroU32::MAX);
        assert_eq!(a.remove(x), Some(1));
        let y = a.insert(2);
        assert_ne!(y.index(), x.index(), "retired slot is never reused");
        assert_eq!(a.get(x), None);
    }

    #[test]
    fn iteration() {
        let mut a: Arena<u32> = Arena::new();
        let k: Vec<_> = (0..5).map(|i| a.insert(i)).collect();
        a.remove(k[1]);
        a.remove(k[3]);
        let vals: Vec<u32> = a.iter().map(|(_, v)| *v).collect();
        assert_eq!(vals, [0, 2, 4]);
    }

    #[test]
    fn side_table() {
        let mut a: Arena<()> = Arena::new();
        let mut t: SideTable<&str> = SideTable::new();
        let x = a.insert(());
        assert_eq!(t.get(x), None);
        assert_eq!(t.insert(x, "a"), None);
        assert_eq!(t.insert(x, "b"), Some("a"));
        assert_eq!(t.get(x), Some(&"b"));
        a.remove(x);
        let y = a.insert(());
        assert_eq!(y.index(), x.index());
        assert_eq!(t.get(y), None, "entry for the old generation is invisible");
        assert_eq!(t.insert(y, "c"), None, "replacing a stale entry returns None");
        assert_eq!(t.get(x), None);
        assert_eq!(t.remove(x), None);
        assert_eq!(t.remove(y), Some("c"));
        *t.get_or_insert_with(y, || "d") = "e";
        assert_eq!(t.get(y), Some(&"e"));
        assert_eq!(t.iter().count(), 1);
    }

    #[test]
    fn option_widget_id_is_8_bytes() {
        assert_eq!(core::mem::size_of::<Option<WidgetId>>(), 8);
    }
}
