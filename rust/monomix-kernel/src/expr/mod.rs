// Task 4: ExprNode, ExprId, FnTag — core types only.
// Task 5: ExprPool — Arena + atom constructors.

use num_bigint::BigInt;
use ordered_float::OrderedFloat;
use rustc_hash::FxHashMap;
use indexmap::IndexSet;
use smallvec::SmallVec;
use num_traits::ToPrimitive;
use num_integer::Integer;

// ---- Handles ---------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, PartialOrd, Ord)]
pub struct LocalExprId(pub u32);

pub type ExprId = LocalExprId;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct InternedStr(pub u32);

// ---- Function tag ----------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum FnTag {
    Sin,
    Cos,
    Tan,
    Exp,
    Log,
    Sqrt,
    Abs,
    Asin,
    Acos,
    Atan,
    Custom(InternedStr),
}

// ---- Expression node -------------------------------------------------------

#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub enum ExprNode {
    // Atoms
    SmallInt(i64),
    BigInt(Box<BigInt>),
    Rational(Box<(BigInt, BigInt)>),
    Float(OrderedFloat<f64>),
    Symbol(InternedStr),
    String(InternedStr),

    // Composite (children are immutable Box<[ExprId]>)
    Add(Box<[ExprId]>),
    Mul(Box<[ExprId]>),
    Pow(ExprId, ExprId),
    Neg(ExprId),
    Div(ExprId, ExprId),
    Eq(ExprId, ExprId),
    Fn(FnTag, Box<[ExprId]>),
    List(Box<[ExprId]>),
}

// Compile-time size guard — fails compilation if ExprNode exceeds the
// 32-byte budget. Matches the runtime test (below) and the spec's
// documented intent (<=32 bytes, not pinned to an exact size).
const _EXPR_NODE_SIZE_GUARD: () = assert!(
    std::mem::size_of::<ExprNode>() <= 32,
    "ExprNode must be <=32 bytes",
);

// ---- Arena -----------------------------------------------------------------

struct ArenaEntry {
    /// Cached content hash. Currently unused at runtime — the dedup map
    /// already keys by hash — but kept for incremental rehash passes
    /// (e.g., pool serialization / merge).
    #[allow(dead_code)]
    hash: u64,
    node: ExprNode,
    subtree_size: u32,
}

// ---- Pool ------------------------------------------------------------------

pub struct ExprPool {
    nodes: Vec<ArenaEntry>,
    /// dedup: content_hash → list of ExprId with that hash
    dedup: FxHashMap<u64, SmallVec<[ExprId; 1]>>,
    strings: IndexSet<String>,

    pub zero: ExprId,
    pub one: ExprId,
    pub minus_one: ExprId,
}

impl ExprPool {
    pub fn new() -> Self {
        let mut pool = ExprPool {
            nodes: Vec::new(),
            dedup: FxHashMap::default(),
            strings: IndexSet::new(),
            zero: LocalExprId(0),
            one: LocalExprId(0),
            minus_one: LocalExprId(0),
        };
        // Pre-intern common constants
        pool.zero = pool.intern(ExprNode::SmallInt(0));
        pool.one  = pool.intern(ExprNode::SmallInt(1));
        pool.minus_one = pool.intern(ExprNode::SmallInt(-1));
        // Pre-intern common symbols: x, y, z, t, e, pi, i
        for sym in &["x", "y", "z", "t", "e", "pi", "i"] {
            pool.intern_str(sym);
        }
        pool
    }

    // --- String interning ---------------------------------------------------

    fn intern_str(&mut self, s: &str) -> InternedStr {
        let lower: String = s.to_lowercase();
        let (idx, _) = self.strings.insert_full(lower);
        InternedStr(idx as u32)
    }

    pub fn str_of(&self, s: InternedStr) -> &str {
        self.strings.get_index(s.0 as usize).map(|s| s.as_str()).unwrap_or("<?>")
    }

    // --- Low-level intern ---------------------------------------------------

    fn content_hash(node: &ExprNode) -> u64 {
        use std::hash::{Hash, Hasher};
        use rustc_hash::FxHasher;
        let mut h = FxHasher::default();
        std::mem::discriminant(node).hash(&mut h);
        match node {
            ExprNode::SmallInt(n) => n.hash(&mut h),
            ExprNode::BigInt(n) => n.hash(&mut h),
            ExprNode::Rational(b) => { b.0.hash(&mut h); b.1.hash(&mut h); }
            ExprNode::Float(f) => f.hash(&mut h),
            ExprNode::Symbol(s) | ExprNode::String(s) => s.hash(&mut h),
            ExprNode::Add(c) | ExprNode::Mul(c) | ExprNode::List(c) => {
                for id in c.iter() { id.hash(&mut h); }
            }
            ExprNode::Pow(a, b) | ExprNode::Div(a, b) | ExprNode::Eq(a, b) => {
                a.hash(&mut h); b.hash(&mut h);
            }
            ExprNode::Neg(x) => x.hash(&mut h),
            ExprNode::Fn(tag, args) => {
                tag.hash(&mut h);
                for id in args.iter() { id.hash(&mut h); }
            }
        }
        h.finish()
    }

    fn subtree_size_of(node: &ExprNode, nodes: &[ArenaEntry]) -> u32 {
        let children: &[ExprId] = match node {
            ExprNode::Add(c) | ExprNode::Mul(c) | ExprNode::List(c) => c,
            ExprNode::Fn(_, c) => c,
            ExprNode::Pow(a, b) | ExprNode::Div(a, b) | ExprNode::Eq(a, b) => {
                return 1 + nodes[a.0 as usize].subtree_size
                         + nodes[b.0 as usize].subtree_size;
            }
            ExprNode::Neg(x) => return 1 + nodes[x.0 as usize].subtree_size,
            _ => return 1,
        };
        1 + children.iter().map(|c| nodes[c.0 as usize].subtree_size).sum::<u32>()
    }

    fn intern(&mut self, node: ExprNode) -> ExprId {
        let hash = Self::content_hash(&node);
        if let Some(candidates) = self.dedup.get(&hash) {
            for &id in candidates.iter() {
                if self.nodes[id.0 as usize].node == node {
                    return id;
                }
            }
        }
        let size = Self::subtree_size_of(&node, &self.nodes);
        let id = LocalExprId(self.nodes.len() as u32);
        self.nodes.push(ArenaEntry { hash, node, subtree_size: size });
        self.dedup.entry(hash).or_default().push(id);
        id
    }

    // --- Atom constructors --------------------------------------------------

    pub fn small_int(&mut self, n: i64) -> ExprId {
        self.intern(ExprNode::SmallInt(n))
    }

    /// Test helper: locates a pre-interned SmallInt without &mut self.
    #[allow(dead_code)]
    pub(crate) fn small_int_check(&self, n: i64) -> ExprId {
        let h = Self::content_hash(&ExprNode::SmallInt(n));
        if let Some(candidates) = self.dedup.get(&h) {
            for &id in candidates.iter() {
                if self.nodes[id.0 as usize].node == ExprNode::SmallInt(n) {
                    return id;
                }
            }
        }
        panic!("small_int_check: {} not pre-interned", n);
    }

    pub fn integer(&mut self, n: BigInt) -> ExprId {
        if let Some(i) = n.to_i64() {
            self.intern(ExprNode::SmallInt(i))
        } else {
            self.intern(ExprNode::BigInt(Box::new(n)))
        }
    }

    pub fn rational(&mut self, p: BigInt, q: BigInt) -> ExprId {
        use num_traits::Zero;
        assert!(!q.is_zero(), "rational: denominator is zero");
        if p.is_zero() {
            return self.zero;
        }
        let g = p.gcd(&q);
        let mut pn = p / &g;
        let mut qn = q / &g;
        if qn.sign() == num_bigint::Sign::Minus {
            pn = -pn;
            qn = -qn;
        }
        if qn == BigInt::from(1) {
            return self.integer(pn);
        }
        self.intern(ExprNode::Rational(Box::new((pn, qn))))
    }

    pub fn float(&mut self, f: f64) -> ExprId {
        self.intern(ExprNode::Float(OrderedFloat(f)))
    }

    pub fn symbol(&mut self, name: &str) -> ExprId {
        let s = self.intern_str(name);
        self.intern(ExprNode::Symbol(s))
    }

    pub fn symbol_by_id(&mut self, s: InternedStr) -> ExprId {
        self.intern(ExprNode::Symbol(s))
    }

    pub fn string_lit(&mut self, s: &str) -> ExprId {
        let id = self.intern_str(s);
        self.intern(ExprNode::String(id))
    }

    // --- Composite constructors (normalizing) -------------------------------

    pub fn add(&mut self, children: Vec<ExprId>) -> ExprId {
        // Flatten nested Add nodes
        let mut flat: Vec<ExprId> = Vec::with_capacity(children.len());
        for c in children {
            if let ExprNode::Add(inner) = self.get(c).clone() {
                flat.extend_from_slice(&inner);
            } else {
                flat.push(c);
            }
        }
        // Remove zeros
        flat.retain(|&c| !self.is_zero(c));
        if flat.is_empty() {
            return self.zero;
        }
        if flat.len() == 1 {
            return flat[0];
        }
        // Sort for canonical form (commutativity)
        flat.sort_unstable();
        self.intern(ExprNode::Add(flat.into_boxed_slice()))
    }

    pub fn mul(&mut self, children: Vec<ExprId>) -> ExprId {
        // Flatten nested Mul nodes
        let mut flat: Vec<ExprId> = Vec::with_capacity(children.len());
        for c in children {
            if let ExprNode::Mul(inner) = self.get(c).clone() {
                flat.extend_from_slice(&inner);
            } else {
                flat.push(c);
            }
        }
        // Short-circuit on zero
        if flat.iter().any(|&c| self.is_zero(c)) {
            return self.zero;
        }
        // Remove ones
        flat.retain(|&c| !self.is_one(c));
        if flat.is_empty() {
            return self.one;
        }
        if flat.len() == 1 {
            return flat[0];
        }
        flat.sort_unstable();
        self.intern(ExprNode::Mul(flat.into_boxed_slice()))
    }

    pub fn pow(&mut self, base: ExprId, exp: ExprId) -> ExprId {
        if self.is_zero(exp) { return self.one; }
        if self.is_one(exp)  { return base; }
        self.intern(ExprNode::Pow(base, exp))
    }

    pub fn neg(&mut self, x: ExprId) -> ExprId {
        if let ExprNode::Neg(inner) = *self.get(x) {
            return inner; // neg(neg(x)) → x
        }
        if self.is_zero(x) {
            return self.zero;
        }
        self.intern(ExprNode::Neg(x))
    }

    pub fn div(&mut self, num: ExprId, den: ExprId) -> ExprId {
        if self.is_one(den) { return num; }
        self.intern(ExprNode::Div(num, den))
    }

    pub fn eq_node(&mut self, lhs: ExprId, rhs: ExprId) -> ExprId {
        self.intern(ExprNode::Eq(lhs, rhs))
    }

    pub fn func(&mut self, tag: FnTag, args: Vec<ExprId>) -> ExprId {
        self.intern(ExprNode::Fn(tag, args.into_boxed_slice()))
    }

    /// Build a `Fn(Custom(...), ...)` node from a string name.
    ///
    /// Caller MUST ensure `name` is not a built-in function name. The parser
    /// dispatches built-ins (`sin`, `cos`, `tan`, `exp`, `log`, `sqrt`, `abs`,
    /// `asin`, `acos`, `atan`) to the proper `FnTag` variants via its
    /// `BuiltinIds` lookup; passing a built-in name through this method would
    /// create a `Custom(intern(name))` node that does NOT compare equal to
    /// the corresponding `FnTag::Sin` / `FnTag::Cos` / etc.
    ///
    /// For built-ins, use `func(FnTag::Sin, args)` directly.
    pub fn func_named(&mut self, name: &str, args: Vec<ExprId>) -> ExprId {
        let s = self.intern_str(name);
        self.func(FnTag::Custom(s), args)
    }

    pub fn list(&mut self, items: Vec<ExprId>) -> ExprId {
        self.intern(ExprNode::List(items.into_boxed_slice()))
    }

    // --- Access -------------------------------------------------------------

    pub fn get(&self, id: ExprId) -> &ExprNode {
        &self.nodes[id.0 as usize].node
    }

    pub fn subtree_size(&self, id: ExprId) -> u32 {
        self.nodes[id.0 as usize].subtree_size
    }

    /// Returns owned Vec of children for any node.
    /// Atoms return empty. For binary operators, returns [a, b]. For n-ary,
    /// the slice is cloned because nodes don't store children in a contiguous
    /// representation that supports a unified `&[ExprId]` view (binary ops
    /// hold two separate fields, not a slice).
    pub fn children(&self, id: ExprId) -> Vec<ExprId> {
        match &self.nodes[id.0 as usize].node {
            ExprNode::Add(c) | ExprNode::Mul(c) | ExprNode::List(c) => c.to_vec(),
            ExprNode::Fn(_, c) => c.to_vec(),
            ExprNode::Pow(a, b) | ExprNode::Div(a, b) | ExprNode::Eq(a, b) => vec![*a, *b],
            ExprNode::Neg(x) => vec![*x],
            _ => Vec::new(),
        }
    }

    pub fn is_zero(&self, id: ExprId) -> bool { id == self.zero }
    pub fn is_one(&self, id: ExprId) -> bool  { id == self.one  }

    pub fn is_atom(&self, id: ExprId) -> bool {
        matches!(self.get(id),
            ExprNode::SmallInt(_) | ExprNode::BigInt(_) | ExprNode::Rational(_)
            | ExprNode::Float(_) | ExprNode::Symbol(_) | ExprNode::String(_))
    }

    pub fn is_numeric(&self, id: ExprId) -> bool {
        matches!(self.get(id),
            ExprNode::SmallInt(_) | ExprNode::BigInt(_) | ExprNode::Rational(_)
            | ExprNode::Float(_))
    }

    pub fn len(&self) -> usize { self.nodes.len() }

    // --- Traversal helpers --------------------------------------------------

    pub fn contains_symbol(&self, expr: ExprId, sym: ExprId) -> bool {
        self.fold(expr, false, &mut |found, id, _node| found || id == sym)
    }

    pub fn fold<A>(
        &self,
        root: ExprId,
        init: A,
        f: &mut dyn FnMut(A, ExprId, &ExprNode) -> A,
    ) -> A {
        let mut visited = rustc_hash::FxHashSet::default();
        self.fold_impl(root, init, f, &mut visited)
    }

    fn fold_impl<A>(
        &self,
        id: ExprId,
        acc: A,
        f: &mut dyn FnMut(A, ExprId, &ExprNode) -> A,
        visited: &mut rustc_hash::FxHashSet<ExprId>,
    ) -> A {
        if !visited.insert(id) {
            return acc;
        }
        let node = &self.nodes[id.0 as usize].node;
        // Visit children first (bottom-up)
        let acc = match node {
            ExprNode::Add(c) | ExprNode::Mul(c) | ExprNode::List(c) => {
                let ids: Vec<ExprId> = c.to_vec();
                ids.iter().fold(acc, |a, &child| self.fold_impl(child, a, f, visited))
            }
            ExprNode::Fn(_, c) => {
                let ids: Vec<ExprId> = c.to_vec();
                ids.iter().fold(acc, |a, &child| self.fold_impl(child, a, f, visited))
            }
            ExprNode::Pow(a, b) | ExprNode::Div(a, b) | ExprNode::Eq(a, b) => {
                let (a, b) = (*a, *b);
                let acc = self.fold_impl(a, acc, f, visited);
                self.fold_impl(b, acc, f, visited)
            }
            ExprNode::Neg(x) => { let x = *x; self.fold_impl(x, acc, f, visited) }
            _ => acc,
        };
        f(acc, id, &self.nodes[id.0 as usize].node)
    }

    /// Rebuild `root` bottom-up by recursively mapping each node through
    /// the normalizing constructors, calling `f` on each rebuilt node.
    ///
    /// The `cache` is **`f`-specific**: it maps `original_root → post-f result`.
    /// Reusing the same cache across two different `f` callbacks will return
    /// stale (wrong) results. Use `map_bottom_up_fresh` if you don't need to
    /// reuse the allocation across calls of the same `f`.
    pub fn map_bottom_up(
        &mut self,
        root: ExprId,
        cache: &mut FxHashMap<ExprId, ExprId>,
        f: &mut dyn FnMut(&mut ExprPool, ExprId) -> ExprId,
    ) -> ExprId {
        if let Some(&cached) = cache.get(&root) {
            return cached;
        }
        let node = self.nodes[root.0 as usize].node.clone();
        let mapped_node = match node {
            ExprNode::Add(c) => {
                let ids: Vec<ExprId> = c.iter().map(|&child| self.map_bottom_up(child, cache, f)).collect();
                self.add(ids)
            }
            ExprNode::Mul(c) => {
                let ids: Vec<ExprId> = c.iter().map(|&child| self.map_bottom_up(child, cache, f)).collect();
                self.mul(ids)
            }
            ExprNode::Pow(a, b) => {
                let a2 = self.map_bottom_up(a, cache, f);
                let b2 = self.map_bottom_up(b, cache, f);
                self.pow(a2, b2)
            }
            ExprNode::Neg(x) => {
                let x2 = self.map_bottom_up(x, cache, f);
                self.neg(x2)
            }
            ExprNode::Div(a, b) => {
                let a2 = self.map_bottom_up(a, cache, f);
                let b2 = self.map_bottom_up(b, cache, f);
                self.div(a2, b2)
            }
            ExprNode::Eq(a, b) => {
                let a2 = self.map_bottom_up(a, cache, f);
                let b2 = self.map_bottom_up(b, cache, f);
                self.eq_node(a2, b2)
            }
            ExprNode::Fn(tag, c) => {
                let ids: Vec<ExprId> = c.iter().map(|&child| self.map_bottom_up(child, cache, f)).collect();
                self.func(tag, ids)
            }
            ExprNode::List(c) => {
                let ids: Vec<ExprId> = c.iter().map(|&child| self.map_bottom_up(child, cache, f)).collect();
                self.list(ids)
            }
            _ => root, // atoms map to themselves
        };
        let result = f(self, mapped_node);
        cache.insert(root, result);
        result
    }

    pub fn map_bottom_up_fresh(
        &mut self,
        root: ExprId,
        f: &mut dyn FnMut(&mut ExprPool, ExprId) -> ExprId,
    ) -> ExprId {
        let mut cache = FxHashMap::default();
        self.map_bottom_up(root, &mut cache, f)
    }
}

impl Default for ExprPool {
    fn default() -> Self { Self::new() }
}

// Compile-time guarantee that ExprPool stays Send + Sync (kernel constraint).
const _: fn() = || {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<ExprPool>();
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expr_node_size_at_most_32_bytes() {
        // This is also enforced at compile time below, but we test it
        // explicitly so a failure has a clear test name.
        assert!(
            std::mem::size_of::<ExprNode>() <= 32,
            "ExprNode is {} bytes, must be <=32",
            std::mem::size_of::<ExprNode>()
        );
    }

    #[test]
    fn local_expr_id_is_copy() {
        let id = LocalExprId(0);
        let id2 = id; // copy
        assert_eq!(id, id2);
    }

    #[test]
    fn pool_integer_interning_idempotent() {
        let mut pool = ExprPool::new();
        let a = pool.small_int(42);
        let b = pool.small_int(42);
        assert_eq!(a, b);
    }

    #[test]
    fn pool_symbol_interning_case_insensitive() {
        let mut pool = ExprPool::new();
        let x1 = pool.symbol("x");
        let x2 = pool.symbol("X"); // lowercased at intern
        assert_eq!(x1, x2);
    }

    #[test]
    fn pool_zero_one_minus_one_pre_interned() {
        let pool = ExprPool::new();
        let z = pool.small_int_check(0);
        let o = pool.small_int_check(1);
        assert_eq!(z, pool.zero);
        assert_eq!(o, pool.one);
    }

    #[test]
    fn pool_rational_normalized() {
        let mut pool = ExprPool::new();
        use num_bigint::BigInt;
        let r1 = pool.rational(BigInt::from(4), BigInt::from(6));
        let r2 = pool.rational(BigInt::from(2), BigInt::from(3));
        assert_eq!(r1, r2);
    }

    #[test]
    fn add_flattens_and_sorts() {
        let mut pool = ExprPool::new();
        let a = pool.symbol("a");
        let b = pool.symbol("b");
        let c = pool.symbol("c");
        let ab = pool.add(vec![a, b]);
        let ab_c = pool.add(vec![ab, c]);
        let bc = pool.add(vec![b, c]);
        let a_bc = pool.add(vec![a, bc]);
        assert_eq!(ab_c, a_bc, "add should flatten and produce same node");
    }

    #[test]
    fn add_commutativity() {
        let mut pool = ExprPool::new();
        let a = pool.symbol("a");
        let b = pool.symbol("b");
        let ab = pool.add(vec![a, b]);
        let ba = pool.add(vec![b, a]);
        assert_eq!(ab, ba);
    }

    #[test]
    fn neg_double_negation() {
        let mut pool = ExprPool::new();
        let x = pool.symbol("x");
        let neg_x = pool.neg(x);
        assert_eq!(pool.neg(neg_x), x);
    }

    #[test]
    fn pow_identity_rules() {
        let mut pool = ExprPool::new();
        let x = pool.symbol("x");
        let zero = pool.zero;
        let one = pool.one;
        assert_eq!(pool.pow(x, zero), one);
        assert_eq!(pool.pow(x, one), x);
    }

    #[test]
    fn div_by_one() {
        let mut pool = ExprPool::new();
        let x = pool.symbol("x");
        let one = pool.one;
        assert_eq!(pool.div(x, one), x);
    }

    #[test]
    fn contains_symbol_finds_nested() {
        let mut pool = ExprPool::new();
        let x = pool.symbol("x");
        let y = pool.symbol("y");
        let xy = pool.mul(vec![x, y]);
        let one = pool.small_int(1);
        let expr = pool.add(vec![xy, one]);
        assert!(pool.contains_symbol(expr, x));
        assert!(pool.contains_symbol(expr, y));
        let z = pool.symbol("z");
        assert!(!pool.contains_symbol(expr, z));
    }

    #[test]
    fn fold_sums_all_small_ints() {
        let mut pool = ExprPool::new();
        let a = pool.small_int(3);
        let b = pool.small_int(4);
        let expr = pool.add(vec![a, b]);
        let sum = pool.fold(expr, 0i64, &mut |acc, _id, node| match node {
            ExprNode::SmallInt(n) => acc + n,
            _ => acc,
        });
        assert_eq!(sum, 7);
    }

    #[test]
    fn map_bottom_up_identity() {
        let mut pool = ExprPool::new();
        let x = pool.symbol("x");
        let one = pool.small_int(1);
        let expr = pool.add(vec![x, one]);
        let result = pool.map_bottom_up_fresh(expr, &mut |_pool, id| id);
        assert_eq!(result, expr);
    }
}
