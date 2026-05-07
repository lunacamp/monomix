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
    pub fn small_int_check(&self, n: i64) -> ExprId {
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
}

impl Default for ExprPool {
    fn default() -> Self { Self::new() }
}

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
}
