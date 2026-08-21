//! The HIR's identity vocabulary — index-based, session-local newtypes.
//!
//! Analysis never compares AST strings or source offsets to decide "is this
//! the same thing?": identity is an ID, and an ID resolves back to a source
//! position only through the [`super::HirSourceMap`]. IDs are indexes into
//! per-file arenas, so they are stable for the lifetime of the [`super::
//! HirFile`] they were allocated in and meaningless outside it — the
//! cross-snapshot story is the query layer's (Phase 6), not the ID's.

/// An index-shaped ID: allocated by an [`Arena`], resolved by it.
pub trait Idx: Copy + Eq + std::hash::Hash + std::fmt::Debug {
    /// Wraps a raw arena index.
    fn new(index: usize) -> Self;
    /// The raw arena index.
    fn index(self) -> usize;
}

macro_rules! define_ids {
    ($($(#[doc = $doc:expr])* $name:ident),+ $(,)?) => {
        $(
            $(#[doc = $doc])*
            #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
            pub struct $name(pub u32);

            impl Idx for $name {
                fn new(index: usize) -> Self {
                    $name(u32::try_from(index).expect("arena index fits u32"))
                }
                fn index(self) -> usize {
                    self.0 as usize
                }
            }
        )+
    };
}

define_ids! {
    /// One source file of the analyzed set. The lowering is handed one; the
    /// multi-file allocator is the resolver's (Phase 2).
    FileId,
    /// One HIR node — anything the source map can place: a statement, an
    /// expression head, a pattern piece, a declaration name.
    NodeId,
    /// An item that owns nested definitions (an enum owning its variants).
    OwnerId,
    /// A definition the resolver produced (Phase 2): an enum, a variant as
    /// a value, an import target. Reserved here so the source map's shape
    /// is final.
    DefId,
    /// A local binding (a pattern binding, a `val` binding) — resolver-owned
    /// (Phase 2).
    LocalId,
    /// One statement stream: the file's top level, an arm's block, an
    /// `else` block, a template interpolation's inner program.
    BodyId,
    /// One expression the analysis can point at — most are opaque
    /// TypeScript spans ([`super::Expr::OpaqueTs`]).
    ExprId,
    /// One pattern node, or-alternatives and nested payloads included.
    PatternId,
    /// One pattern site — the construct-neutral unit every pattern-carrying
    /// syntax lowers to ([`super::PatternSite`]).
    PatternSiteId,
    /// One enum variant, across every enum of the file.
    VariantId,
    /// One payload field, across every variant of the file.
    FieldId,
    /// One lexical scope — resolver-owned (Phase 2).
    ScopeId,
}

/// A typed vector: IDs in, values out. The allocation order is the identity
/// order, which for the lowering means source order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Arena<I: Idx, T> {
    raw: Vec<T>,
    _marker: std::marker::PhantomData<I>,
}

impl<I: Idx, T> Default for Arena<I, T> {
    fn default() -> Self {
        Arena {
            raw: Vec::new(),
            _marker: std::marker::PhantomData,
        }
    }
}

impl<I: Idx, T> Arena<I, T> {
    /// Stores `value` and returns its ID.
    pub fn alloc(&mut self, value: T) -> I {
        let id = I::new(self.raw.len());
        self.raw.push(value);
        id
    }

    /// How many values have been allocated.
    pub fn len(&self) -> usize {
        self.raw.len()
    }

    /// Whether nothing has been allocated.
    pub fn is_empty(&self) -> bool {
        self.raw.is_empty()
    }

    /// Iterates `(id, value)` in allocation (= source) order.
    pub fn iter(&self) -> impl Iterator<Item = (I, &T)> {
        self.raw.iter().enumerate().map(|(i, v)| (I::new(i), v))
    }
}

impl<I: Idx, T> std::ops::Index<I> for Arena<I, T> {
    type Output = T;
    fn index(&self, id: I) -> &T {
        &self.raw[id.index()]
    }
}

impl<I: Idx, T> std::ops::IndexMut<I> for Arena<I, T> {
    fn index_mut(&mut self, id: I) -> &mut T {
        &mut self.raw[id.index()]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_arena_hands_back_what_it_allocated() {
        let mut arena: Arena<NodeId, &str> = Arena::default();
        let a = arena.alloc("a");
        let b = arena.alloc("b");
        assert_ne!(a, b);
        assert_eq!(arena[a], "a");
        assert_eq!(arena[b], "b");
        assert_eq!(
            arena.iter().map(|(_, v)| *v).collect::<Vec<_>>(),
            ["a", "b"]
        );
    }
}
