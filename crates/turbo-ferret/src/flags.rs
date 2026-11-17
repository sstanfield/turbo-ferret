//! Hand-rolled bit flag types (H-14: replaces TB's packed bitfields).

macro_rules! flags {
    ($(#[$m:meta])* $vis:vis struct $name:ident: $t:ty { $($(#[$fm:meta])* const $f:ident = $v:expr;)* }) => {
        $(#[$m])*
        #[derive(Copy, Clone, Eq, PartialEq, Hash, Default)]
        $vis struct $name(pub $t);
        #[allow(dead_code)]
        impl $name {
            $($(#[$fm])* pub const $f: $name = $name($v);)*
            pub const NONE: $name = $name(0);
            pub const fn bits(self) -> $t { self.0 }
            pub const fn contains(self, o: $name) -> bool { self.0 & o.0 == o.0 }
            pub const fn intersects(self, o: $name) -> bool { self.0 & o.0 != 0 }
            pub const fn is_empty(self) -> bool { self.0 == 0 }
            pub const fn union(self, o: $name) -> $name { $name(self.0 | o.0) }
            pub const fn without(self, o: $name) -> $name { $name(self.0 & !o.0) }
            pub fn insert(&mut self, o: $name) { self.0 |= o.0; }
            pub fn remove(&mut self, o: $name) { self.0 &= !o.0; }
            pub fn set(&mut self, o: $name, on: bool) { if on { self.insert(o) } else { self.remove(o) } }
        }
        impl core::ops::BitOr for $name {
            type Output = $name;
            fn bitor(self, o: $name) -> $name { $name(self.0 | o.0) }
        }
        impl core::ops::BitOrAssign for $name {
            fn bitor_assign(&mut self, o: $name) { self.0 |= o.0; }
        }
        impl core::ops::BitAnd for $name {
            type Output = $name;
            fn bitand(self, o: $name) -> $name { $name(self.0 & o.0) }
        }
        impl core::ops::Not for $name {
            type Output = $name;
            fn not(self) -> $name { $name(!self.0) }
        }
        impl core::fmt::Debug for $name {
            fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
                write!(f, "{}(", stringify!($name))?;
                let mut first = true;
                $(if self.contains($name::$f) && $name::$f.0 != 0 {
                    if !first { write!(f, " | ")?; }
                    write!(f, "{}", stringify!($f))?;
                    first = false;
                })*
                let _ = first;
                write!(f, ")")
            }
        }
    };
}
pub(crate) use flags;
