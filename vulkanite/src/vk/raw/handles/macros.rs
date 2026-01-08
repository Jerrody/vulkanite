macro_rules! vk_handle {
    ($name:ident, $obj_type:ident, $doc_tag:meta, $vk_name:literal, $ty:ident) => {
        #[repr(transparent)]
        #[$doc_tag]
        #[doc(alias = $vk_name)]
        #[derive(Eq, PartialEq, PartialOrd, Ord, Hash, Clone, Copy)]
        pub struct $name($ty);

        impl private::Sealed for $name {}
        impl Handle for $name {
            type InnerType = $ty;

            const TYPE: ObjectType = ObjectType::$obj_type;

            #[inline(always)]
            fn as_raw(&self) -> $ty {
                self.0
            }

            #[inline(always)]
            unsafe fn from_raw(x: $ty) -> Self {
                Self(x)
            }

            #[inline(always)]
            unsafe fn clone(&self) -> Self {
                Self(self.0)
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "0x{:X}", self.0)
            }
        }

        impl fmt::Debug for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.debug_tuple(stringify!($name))
                    .field(&format_args!("0x{:X}", self.0))
                    .finish()
            }
        }
    };
}

macro_rules! handle_dispatchable {
    ($name:ident, $obj_type:ident, $doc_tag:meta, $vk_name:literal) => {
        vk_handle! {$name, $obj_type, $doc_tag, $vk_name, NonZeroUsize}
    };
}

macro_rules! handle_nondispatchable {
    ($name:ident, $obj_type:ident, $doc_tag:meta, $vk_name:literal) => {
        vk_handle! {$name, $obj_type, $doc_tag, $vk_name, NonZeroU64}
    };
}

#[cfg(all(feature = "ext_mesh_shader", feature = "ext_shader_object"))]
macro_rules! handle_nondispatchable_u64 {
    ($name:ident, $obj_type:expr, $doc:expr, $vk_name:expr) => {
        #[repr(transparent)]
        #[derive(Eq, PartialEq, Ord, PartialOrd, Clone, Copy, Hash)]
        $doc
        pub struct $name(u64);

        impl Handle for $name {
            const TYPE: ObjectType = $obj_type;

            #[inline]
            fn as_raw(self) -> u64 {
                self.0
            }

            #[inline]
            fn from_raw(x: u64) -> Self {
                Self(x)
            }
        }

        impl Default for $name {
            fn default() -> Self {
                Self(0)
            }
        }

        impl fmt::Debug for $name {
            fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
                write!(f, "0x{:x}", self.0)
            }
        }
    };
}
