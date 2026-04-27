# Parser — System Design

**Component:** `monomix-kernel::parser`
**Status:** Design phase
**Date:** 2026-04-26
**References:** SCOPE.md §0.6, §1.2; ADR-0001; ADR-0002; `designs/expression-dag.md`

---

## 1. Requirements

### 1.1 Functional requirements

The parser converts a source string in the Phase 1 REDUCE-syntax subset into an `ExprId` tree
inside an `ExprPool`. It is the single entry point from text into the kernel; nothing else in
the kernel reads raw strings.

It must support:

- **Expressions** with full operator precedence: arithmetic (`+`, `-`, `*`, `/`, `^`/`**`),
  unary minus, equality (`=`), grouping via parentheses.
- **Function calls:** `f(arg1, arg2, ...)` for built-in and user-declared names.
- **Assignment:** `symbol := expr` binds a name in the Python `Session` (the parser emits an
  `Assign` node; binding resolution happens outside the kernel).
- **Statement terminators:** `;` (display result) and `$` (suppress result). Both end a
  statement. A source string may contain multiple statements; the parser returns them all.
- **Line comments:** `%` through end-of-line.
- **Block comments:** `comment` *text* `;` — the literal keyword `comment`, arbitrary text,
  terminated by `;`.
- **Built-in function recognition:** `df`, `int` (Phase 2 stub), `solve`, `factor` (Phase 2
  stub), `expand`, `simplify`, `sub`. The parser emits typed AST nodes for these; unrecognized
  names become generic `Fn(Custom(...), ...)` nodes.
- **Numeric literals:** integers (decimal), rationals written as `p/q` (treated as one token
  sequence, not two), IEEE-754 floating-point (`1.5`, `2.0e-3`).
- **Byte-accurate source spans** on every AST node, so diagnostics can point to the exact
  character(s) in the original input.
- **Error recovery:** a syntax error in one statement must not abort parsing of subsequent
  statements. The parser emits a `Diagnostic` for the bad statement and continues from the
  next `;` or `$`.

### 1.2 Non-functional requirements

| Requirement | Target | Rationale |
|-------------|--------|-----------|
| Throughput | ≥5 MB/s of source text | Script files can be large; this is conservative headroom |
| Latency | <1 ms for a single interactive statement (≤200 chars) | REPL feel |
| No panics on any input | Verified by ≥1 h `cargo-fuzz` | SCOPE.md §1.12 success criterion |
| `Send + Sync` | Required | Kernel rule (ADR-0002) |
| No `unsafe` | Required | Kernel rule (ADR-0002) |
| Error quality | Diagnostic names the unexpected token and suggests the expected one | Developer experience |

### 1.3 Constraints

- Parser lives entirely in `crates/monomix-kernel/src/parser.rs` (and sibling modules
  `lexer.rs`, `ast.rs`). No Python code in this layer.
- The parser writes its output directly into an `ExprPool` via `&mut ExprPool`; it does not
  build an intermediate heap-allocated tree that is later converted.
- No parser-generator or PEG-crate runtime in the final binary (see §4.1 for the trade-off).
  The lexer and parser are hand-written.
- The parser is stateless between calls; all mutable state is in `ExprPool` and a `ParseResult`
  return value.

---

## 2. High-Level Design

### 2.1 Phase 1 grammar (formal specification)

```
program        ::= statement* EOF

statement      ::= assign_stmt terminator
                 | expr_stmt   terminator
                 | comment_stmt
                 | error_recovery

assign_stmt    ::= IDENT ":=" expr
expr_stmt      ::= expr

terminator     ::= ";" | "$"
comment_stmt   ::= "%" LINE_TAIL           -- consumed by lexer, emits no AST node
                 | "comment" comment_body ";"
comment_body   ::= (any token except ";")*

expr           ::= equality_expr
equality_expr  ::= add_expr ("=" add_expr)?
add_expr       ::= mul_expr (("+" | "-") mul_expr)*
mul_expr       ::= unary_expr (("*" | "/") unary_expr)*
unary_expr     ::= "-" unary_expr | pow_expr
pow_expr       ::= call_expr ("^" unary_expr | "**" unary_expr)?   -- right-associative
call_expr      ::= primary ("(" arg_list ")")?
arg_list       ::= expr ("," expr)*
primary        ::= INTEGER | RATIONAL | FLOAT | IDENT | "(" expr ")"

-- Built-in call forms (sugar over call_expr; same AST node):
-- df(expr, ident [, ident]*)
-- int(expr, ident)                -- emits UnsupportedError stub
-- solve(expr, ident)
-- factor(expr)                    -- emits UnsupportedError stub
-- expand(expr)
-- simplify(expr)
-- sub(ident "=" expr, expr)       -- REDUCE-style: sub(x=5, expr)
```

Operator precedence summary (lowest to highest):

| Level | Operator | Associativity |
|-------|----------|---------------|
| 1 | `=` | non-associative |
| 2 | `+`, `-` | left |
| 3 | `*`, `/` | left |
| 4 | unary `-` | prefix |
| 5 | `^`, `**` | right |
| 6 | function call `f(...)` | postfix |
| 7 | atoms, `(expr)` | — |

### 2.2 Component diagram

```
                    source: &str
                         │
                         ▼
                  ┌────────────┐
                  │   Lexer    │  byte-by-byte scan → Token + Span stream
                  └─────┬──────┘
                        │  Iterator<Item = (Token, Span)>
                        ▼
               ┌──────────────────┐
               │  Parser (Pratt)  │  recursive descent for statements,
               │                  │  Pratt for expressions
               └──────┬───────────┘
                      │ writes ExprIds into ExprPool
                      │ collects Vec<Diagnostic>
                      ▼
            ┌──────────────────────┐
            │  ParseResult         │
            │  statements: Vec<Stmt>│  Stmt = (ExprId, Span, Terminator)
            │  diagnostics: Vec<Diagnostic>│
            └──────────────────────┘
```

The parser writes output directly into the `ExprPool` — each parsed sub-expression becomes an
interned `ExprId` immediately. There is no intermediate heap-allocated "parse tree" that is
later converted to `ExprId`s.

### 2.3 Public API

```rust
/// Parse `source` and intern all produced expressions into `pool`.
/// Returns all successfully parsed statements and any diagnostics.
/// Never panics — all errors are returned as Diagnostics.
pub fn parse(source: &str, pool: &mut ExprPool) -> ParseResult;

pub struct ParseResult {
    /// One entry per successfully parsed statement (in source order).
    /// Errors cause an entry to be omitted; a Diagnostic is emitted instead.
    pub statements: Vec<Stmt>,
    /// All parse errors encountered. Non-empty does not imply statements is empty
    /// (error recovery means some statements may succeed even with errors present).
    pub diagnostics: Vec<Diagnostic>,
}

pub struct Stmt {
    /// The parsed expression or assignment target; ExprId into the pool.
    pub expr: ExprId,
    /// Whether this statement was terminated with `;` (display) or `$` (suppress).
    pub output: OutputMode,
    /// Span covering the entire statement including its terminator.
    pub span: Span,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum OutputMode { Display, Suppress }

pub struct Diagnostic {
    pub severity: Severity,
    pub span: Span,
    pub message: String,       // human-readable
    pub code: DiagnosticCode,  // machine-readable variant
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Severity { Error, Warning }

/// Byte offset range into the original source string.
/// start is inclusive, end is exclusive — matches Rust slice semantics.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Span { pub start: u32, pub end: u32 }

impl Span {
    pub fn to_str<'s>(&self, source: &'s str) -> &'s str {
        &source[self.start as usize..self.end as usize]
    }
    pub fn merge(self, other: Span) -> Span {
        Span { start: self.start.min(other.start), end: self.end.max(other.end) }
    }
    pub const SYNTHETIC: Span = Span { start: u32::MAX, end: u32::MAX };
}
```

Span offsets are `u32` (4 bytes each → 8 bytes per Span). A source string longer than 4 GB is
not a realistic use case for a CAS; `u32` offsets save space vs. `usize` on 64-bit platforms.

### 2.4 Data flow

```
  source string
       │
       ├─── Lexer ──────────────────────────────────────────────────────┐
       │    scan bytes → Token variants with Span                        │
       │    buffer 1 token of lookahead (peeked)                         │
       │    skip whitespace; handle % comments inline                    │
       └────────────────────────────────────────────────────────────────┘
                 │  stream of (Token, Span)
                 ▼
  Parser
   │
   ├── parse_program(): loop until EOF
   │     ├── handle `comment` keyword → skip to `;`
   │     ├── parse_assign_or_expr()
   │     │     ├── peek(IDENT), peek(`:=`) → parse_assign()
   │     │     └── otherwise → parse_expr() via Pratt dispatcher
   │     ├── expect terminator `;` | `$`
   │     └── on error: emit Diagnostic, advance to next `;`/`$`/EOF (recovery)
   │
   └── parse_expr(min_bp):  Pratt (binding-power) loop
         ├── parse prefix: atom | `(` expr `)` | unary `-`
         ├── call detect_builtin(): emit typed ExprNode if name is built-in
         └── loop: peek infix/postfix operator, check binding power, recurse right
```

---

## 3. Deep Dive

### 3.1 Lexer

The lexer is a byte-scanner that produces tokens on demand. It maintains a single `position:
usize` cursor into the source `&str`. Whitespace (space, tab, `\r`, `\n`) is skipped silently
between tokens. The `%`-to-end-of-line comment is consumed inline and produces no token.

**Token set:**

```rust
#[derive(Clone, PartialEq, Debug)]
pub enum Token {
    // Literals — small/big integer split mirrors ExprPool's SmallInt fast path
    // (see designs/expression-dag.md §3.4). Avoids BigInt heap allocations for
    // the overwhelmingly common case of i64-fitting literals.
    SmallInt(i64),
    BigInt(Box<num_bigint::BigInt>),
    Float(f64),
    /// Identifier carries the source span only — resolved to InternedStr by the
    /// parser at use-site. This decouples the lexer from ExprPool, so the lexer
    /// is reusable for tooling (LSP, formatter, fuzz minimization) that has no
    /// pool. Lowercase normalization is applied at intern time, not at lex time.
    Ident(Span),

    // Operators (canonicalized at lex time)
    Plus, Minus, Star, Slash,
    Pow,                        // both `^` and `**` lex to this token
    Assign,                     // :=
    Equals,                     // =
    Comma, LParen, RParen,

    // Terminators
    Semi, Dollar,

    // Keywords (identified during lexing, not in the parser)
    KwComment,                  // "comment" — case-insensitive

    // Sentinel
    Eof,
}

/// Payload-free `Copy` discriminant for cheap peeking. Pratt's binding-power
/// dispatch and statement-prefix detection only need the kind; cloning a full
/// `Token` (which contains a `Box<BigInt>`) on every peek would allocate.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TokenKind {
    SmallInt, BigInt, Float, Ident,
    Plus, Minus, Star, Slash, Pow,
    Assign, Equals, Comma, LParen, RParen,
    Semi, Dollar, KwComment, Eof,
}

impl Token {
    pub fn kind(&self) -> TokenKind { /* trivial match */ }
}
```

**Rational literal handling.** The sequence `INTEGER "/" INTEGER` is not fused into a single
token at the lexer level — that would require unbounded lookahead or backtracking to distinguish
`1/2` (rational) from `a/b` (division). Instead, the parser's `parse_primary()` recognises the
pattern `Integer Slash Integer` and calls `pool.rational()` directly. This is a parser-level
concern, not a lexer-level concern.

**Case insensitivity.** REDUCE identifiers are case-insensitive: `SIN`, `Sin`, `sin` all mean
the same function. Lowercase normalisation happens at *interning time* in the parser, not in
the lexer (the lexer only emits `Token::Ident(Span)`). The parser slices `source[span]`,
lowercases into a small stack buffer (or heap if >64 chars), and interns the lowercase form so
that the `InternedStr` for `x` and `X` is the same index.

**Operator canonicalization.** `^` and `**` both lex to `Token::Pow`. The original spelling
is recoverable from the span if needed for diagnostics. This removes a redundant arm from
every downstream `match` on operator tokens.

**Number scanning:**
- Integer: `[0-9]+` not followed by `.` or `e`/`E`. The lexer first attempts `i64::from_str`;
  on overflow it falls back to `BigInt::parse_bytes`. Emits `Token::SmallInt(i64)` or
  `Token::BigInt(Box<BigInt>)` accordingly. This makes the common case allocation-free.
- Float: `[0-9]+ '.' [0-9]* ([eE] [+-]? [0-9]+)?` or `[0-9]+ [eE] [+-]? [0-9]+`. The literal
  forms `inf`, `nan`, `+inf`, `-nan` are **rejected** at lex time with
  `DiagnosticCode::InvalidNumericLiteral` — REDUCE has no syntax for them, and admitting them
  would let users build symbolic nodes that violate `ExprPool` invariants.
- Float parsing uses `f64::from_str` (via `str::parse`).

**Length caps.** Identifiers and numeric literals are capped at 1024 source bytes. Exceeding
the cap emits `DiagnosticCode::IdentifierTooLong` or `NumericLiteralTooLong` and consumes the
oversized run as a single error token. This is defensive against fuzzer-generated pathological
input (`aaa...aaa` of arbitrary length) and complements the "no panics on any input"
requirement with "no unbounded allocations on any input".

**Lexer struct:**

```rust
struct Lexer<'s> {
    src: &'s str,
    pos: usize,
    /// Two-slot lookahead ring buffer. [0] is the next token; [1] is the
    /// after-next (only filled when peek_at(1) is called). This is the only
    /// place lookahead exceeds one token, used for `IDENT :=` detection at
    /// statement start (§3.4).
    buffer: ArrayVec<(Token, Span), 2>,
}

impl<'s> Lexer<'s> {
    /// Borrow the next token without consuming it. Fills slot 0 if empty.
    fn peek(&mut self) -> &(Token, Span);
    /// Cheap kind-only peek — does not clone the payload. Used in the Pratt
    /// inner loop for binding-power dispatch.
    fn peek_kind(&mut self) -> TokenKind;
    /// Borrow the token at offset 0 or 1 without consuming. `peek_at(1)` fills
    /// both slots. Calling with offset > 1 is a programming error.
    fn peek_at(&mut self, offset: usize) -> &(Token, Span);
    /// Consume and return the next token (drains slot 0; slot 1 shifts down).
    fn next(&mut self) -> (Token, Span);
    fn skip_whitespace_and_line_comments(&mut self);
}
```

The lexer is now self-contained — no `&mut ExprPool` dependency. Identifier tokens carry only
their source span; the parser performs the lowercase + intern step at use-site. This separation
means the lexer is reusable for any tool that needs tokenization without a populated pool
(syntax highlighter, formatter, future LSP).

### 3.2 Parser — Pratt (top-down operator precedence)

Expression parsing uses the Pratt algorithm (top-down operator precedence), which cleanly
handles mixed left/right associativity, prefix operators, and function-call postfix syntax
without ad hoc grammar transformations.

The central data structure is binding power — a `(left_bp, right_bp)` pair per infix operator:

Binding-power tables operate on `TokenKind` (Copy) so the Pratt inner loop never clones a
`Token`. Numeric values are spaced by 10 to leave room for future precedence levels (e.g.,
bitwise operators in a later phase) without renumbering existing constants.

```rust
fn infix_bp(kind: TokenKind) -> Option<(u8, u8)> {
    match kind {
        TokenKind::Equals  => Some((10, 0)),    // non-associative: (10, 0) forbids chaining
        TokenKind::Plus
        | TokenKind::Minus => Some((20, 21)),   // left-associative
        TokenKind::Star
        | TokenKind::Slash => Some((30, 31)),   // left-associative
        TokenKind::Pow     => Some((50, 49)),   // right-associative: right_bp < left_bp
        _                  => None,
    }
}

fn prefix_bp(kind: TokenKind) -> Option<u8> {
    match kind {
        TokenKind::Minus => Some(40), // unary minus, higher than * but lower than ^
        _                => None,
    }
}
```

**Core Pratt loop:**

```rust
fn parse_expr(&mut self, min_bp: u8) -> Result<ExprId, ()> {
    // 1. Parse prefix or atom — `next()` *moves* the token out (no clone).
    let (tok, tok_span) = self.lexer.next();
    let mut lhs = match tok {
        Token::SmallInt(n)  => self.pool.small_int(n),
        Token::BigInt(n)    => self.pool.big_int(*n),
        Token::Float(f)     => self.pool.float(f),
        Token::Ident(span)  => self.parse_ident_or_call(span)?,
        Token::LParen       => {
            let inner = self.parse_expr(0)?;
            self.expect(TokenKind::RParen)?;
            inner
        }
        Token::Minus        => {
            let right = self.parse_expr(prefix_bp(TokenKind::Minus).unwrap())?;
            self.pool.neg(right)
        }
        other => return Err(self.emit_unexpected(other, tok_span)),
    };

    // 2. Rational literal shortcut: SmallInt/BigInt '/' SmallInt/BigInt.
    //    peek_kind() avoids cloning the token's payload.
    if matches!(self.pool.get(lhs), ExprNode::SmallInt(_) | ExprNode::BigInt(_))
        && self.lexer.peek_kind() == TokenKind::Slash
    {
        if let Some(rhs_int) = self.try_consume_rational_denominator() {
            lhs = self.pool.rational_from_ints(lhs, rhs_int)?;
        }
    }

    // 3. Pratt infix loop — kind-only peek, no payload clone.
    loop {
        let op_kind = self.lexer.peek_kind();
        let Some((left_bp, right_bp)) = infix_bp(op_kind) else { break };
        if left_bp <= min_bp { break }
        let (op_tok, op_span) = self.lexer.next(); // consume operator

        let rhs = self.parse_expr(right_bp)?;
        lhs = self.build_infix(op_tok, lhs, rhs, op_span)?;
    }

    Ok(lhs)
}
```

Note the absence of `Token::clone()` in the inner loop — the only owned data inside a `Token`
is the `Box<BigInt>` of `Token::BigInt`, which would otherwise allocate on every peek. With
the `peek_kind()` design, the loop is allocation-free regardless of operand size.

The `build_infix` helper maps operator tokens to `pool.add()`, `pool.mul()`, etc. `Equals`
produces `pool.eq(lhs, rhs)`.

### 3.3 Built-in function detection

When the parser encounters `IDENT "("`, it calls `parse_ident_or_call()`. Built-in dispatch
uses a **pre-interned `BuiltinTable`** — built-in names are interned once at `ExprPool`
construction time, and dispatch becomes integer-equality comparison on `InternedStr` indices
rather than O(name length) string comparisons through `pool.str_of()`.

```rust
/// Indices of built-in function names, interned once at pool construction.
/// Equality comparison on InternedStr is a single u32 compare.
pub struct BuiltinTable {
    pub df:       InternedStr,
    pub int_:     InternedStr,
    pub solve:    InternedStr,
    pub factor:   InternedStr,
    pub expand:   InternedStr,
    pub simplify: InternedStr,
    pub sub:      InternedStr,
}

fn parse_ident_or_call(&mut self, ident_span: Span) -> Result<ExprId, ()> {
    // Resolve span → lowercase → InternedStr at use-site.
    let name = self.intern_lowercased(ident_span);

    if self.lexer.peek_kind() != TokenKind::LParen {
        return Ok(self.pool.symbol_by_id(name));   // plain symbol reference
    }
    self.lexer.next(); // consume '('

    let bt = &self.builtins;
    match name {
        n if n == bt.df       => self.parse_df(ident_span),
        n if n == bt.int_     => self.parse_int_stub(ident_span),
        n if n == bt.solve    => self.parse_solve(ident_span),
        n if n == bt.factor   => self.parse_factor_stub(ident_span),
        n if n == bt.expand   => self.parse_unary_builtin(FnTag::Expand, ident_span),
        n if n == bt.simplify => self.parse_unary_builtin(FnTag::Simplify, ident_span),
        n if n == bt.sub      => self.parse_sub(ident_span),
        _                     => self.parse_generic_call(name, ident_span),
    }
}
```

This eliminates the per-identifier string traffic through `pool.str_of()` that the naive
dispatch would incur on every call site, including all *user* identifiers (which always
fall through to the `_` arm).

Each handler parses its specific argument list and returns an `ExprId`. Stubs (`int`, `factor`)
parse their arguments normally but emit an `ExprId` node tagged with a `UnsupportedStub` marker
in the pool — at evaluation time (not parse time), the evaluator checks this tag and raises
`UnsupportedError`. This means scripts containing `int(...)` parse without error; they fail only
when evaluated, which is the right behaviour for the REPL.

**`df` argument parsing** (supports repeated differentiation):
```
df(expr, var)           → Derivative(expr, var, 1)
df(expr, var, var)      → Derivative(expr, var, 2)   -- repeated symbol
df(expr, x, y)          → partial: df(df(expr, x), y)
```

**`sub` argument parsing** (REDUCE style):
```
sub(x = 5, expr)        → Substitution(expr, x → 5)
sub(x = a, y = b, expr) → nested substitutions, rightmost expr
```

### 3.4 Assignment statements

Assignment (`ident := expr`) is detected at the statement level, not the expression level.
The parser peeks ahead: if the next two tokens are `IDENT` then `:=`, it parses an assignment.
Otherwise it parses a plain expression statement.

```rust
fn parse_stmt(&mut self) -> Result<Option<Stmt>, ()> {
    // Detect assignment via two-token lookahead — the only place in the
    // grammar where we look further than one token ahead.
    if self.lexer.peek_kind() == TokenKind::Ident
        && self.lexer.peek_at(1).0.kind() == TokenKind::Assign
    {
        return self.parse_assign_stmt();
    }
    self.parse_expr_stmt()
}
```

The lexer's `peek_at(offset)` API (§3.1) materialises the second slot on demand. After this
check, `parse_assign_stmt` consumes both tokens via two `lexer.next()` calls. The `ArrayVec<2>`
buffer caps lookahead at exactly two tokens — no general multi-token buffer, no risk of
unbounded lookahead creeping in as the grammar evolves.

The returned `Stmt` for an assignment wraps an `ExprId` that is a `pool.eq(symbol, value)` node
with a special `Assign` wrapper:

```rust
pub struct Stmt {
    pub kind: StmtKind,
    pub expr: ExprId,
    pub output: OutputMode,
    pub span: Span,
}

pub enum StmtKind {
    /// Plain expression — pass to evaluator.
    Expr,
    /// Assignment: lhs symbol := rhs. The Python Session handles the binding.
    Assign { lhs: InternedStr },
}
```

Assignment semantics (actually updating the binding table) live in the Python `Session`, not in
the kernel. The kernel just reports what was assigned to what.

### 3.5 Span tracking

Every `ExprId` produced by the parser carries a span, but spans are *not* stored inside the
`ExprPool` arena (that would inflate every node, including those produced by the simplifier and
differentiator that have no source origin). Instead, spans live in a side-table:

```rust
/// Returned alongside ParseResult. Maps ExprId → source Span for nodes
/// that originated from parsing. Nodes created by simplify/diff have no entry.
pub type SpanMap = FxHashMap<ExprId, Span>;
```

`ParseResult` includes a `SpanMap`. The Python `ParseError` exception copies the relevant
span(s) out of the map before the map is dropped. This keeps the hot-path data structures
(arena, dedup map) span-free while still providing accurate diagnostics.

**Phase 2 optimisation (deferred):** `FxHashMap<ExprId, Span>` is convenient but heavyweight
— ~48 B/entry vs 8 B for the span itself. Since lookup happens only on diagnostic emission
(off the hot path), a sorted `Vec<(ExprId, Span)>` (binary-searched) or parallel arrays
`(Vec<ExprId>, Vec<Span>)` would cut memory ~5×. Defer until profiling shows `SpanMap`
visible in memory traces.

**Span construction rules:**
- Atom: span of the token itself.
- Unary `-expr`: span from `-` token to end of `expr`.
- Binary `lhs op rhs`: span from start of `lhs` to end of `rhs`.
- Function call `f(args)`: span from start of `f` to closing `)`.
- Statement: span from first token to terminator (inclusive).

### 3.6 Error recovery

The parser uses **synchronisation-point recovery**: on any parse error, it emits a `Diagnostic`,
then advances the token stream until it finds a `;` or `$` *at paren depth 0*, or `EOF`.
Parsing resumes at the next statement. Tracking paren depth is essential because Phase 2
constructs (`for ... do ... ;`, `procedure ... ; ... end ;`) will admit nested terminators —
without depth-tracking, recovery would consume them and resume in the wrong place.

```rust
fn synchronise(&mut self) {
    let mut depth: u32 = 0;
    loop {
        match self.lexer.peek_kind() {
            TokenKind::LParen => { depth += 1; self.lexer.next(); }
            TokenKind::RParen if depth > 0 => { depth -= 1; self.lexer.next(); }
            TokenKind::Semi | TokenKind::Dollar if depth == 0 => break,
            TokenKind::Eof => return,
            _ => { self.lexer.next(); }
        }
    }
    // Consume the terminator itself so the outer loop sees a clean state.
    self.lexer.next();
}
```

This is the same strategy used by most production compilers (e.g., `rustc` for block-level
recovery), augmented with depth tracking. It guarantees: (a) every statement either produces
an `ExprId` or produces exactly one `Diagnostic`, (b) parsing never loops forever, (c) a
single bad statement does not cascade into spurious errors in subsequent statements,
(d) nested grammar constructs (Phase 2+) cannot trick recovery into resuming inside a
half-parsed structure.

**Error quality.** Diagnostics include the span, a description of what was found, and what was
expected:

```
ParseError at 1:14–1:16: expected expression, found ":="
  | df(x^2 + := 1, x)
  |            ^^
```

The `DiagnosticCode` enum enables the Python exception hierarchy to be specific:

```rust
pub enum DiagnosticCode {
    UnexpectedToken { found: TokenKind, expected: &'static str },
    UnterminatedStatement,
    UnbalancedParen,
    InvalidNumericLiteral,
    NumericLiteralTooLong,
    IdentifierTooLong,
    MissingArgument { function: &'static str },
    TooManyArguments { function: &'static str, max: usize },
}
```

`UnexpectedToken.found` is `TokenKind` (not `String`) — the user-facing rendering happens at
diagnostic-print time using a static `kind → display` table, avoiding per-error allocation.

**Phase 2 cleanup (deferred):** `Diagnostic.message: String` is largely redundant with `code`
(which already encodes the dynamic payload structurally). Once the rendering table is in
place, consider replacing `message` with `Option<Cow<'static, str>>` for cases where the code
variant doesn't capture everything, or removing it entirely. This eliminates per-error
allocations in fuzz workloads (where errors are the common case).

### 3.7 Integration with ExprPool

The parser writes directly into the `ExprPool`. The caller passes a `&mut ExprPool`. Because
parsing is single-threaded (one input stream, one mutable pool), no locking is needed during
parsing itself. At the PyO3 boundary:

```rust
// In monomix-py/src/lib.rs
#[pyfunction]
fn parse(source: &str, py: Python<'_>, session: &PySession) -> PyResult<Vec<PyExpr>> {
    let pool_guard = session.pool.write();  // exclusive write lock
    py.allow_threads(|| {                  // GIL released
        let result = monomix_kernel::parse(source, &mut *pool_guard);
        // Map diagnostics → ParseError; map Stmts → PyExpr handles
        ...
    })
}
```

The GIL is released while the parser runs. The write lock on the pool is held for the duration
of `parse()`. This is acceptable because parsing is fast (<1 ms for interactive input) — the
lock contention window is narrow.

For the MCP server (Phase 1.5), each request arrives on its own thread. If two requests parse
simultaneously, the second waits for the pool write lock. For Phase 1 this is fine; Phase 2
can introduce per-request pools (see §5.2).

### 3.8 Error mapping at the PyO3 boundary

```rust
// KernelError::Parse carries a Vec<Diagnostic>
KernelError::Parse(diags) => {
    let py_diags: Vec<_> = diags.iter().map(|d| PyDiagnostic {
        message: d.message.clone(),
        start_byte: d.span.start,
        end_byte: d.span.end,
        code: format!("{:?}", d.code),
    }).collect();
    ParseError::new_err(py_diags)
}
```

Python `monomix.ParseError` exposes `.diagnostics` as a list of objects with `.message`,
`.start_byte`, `.end_byte`, and `.code`. The REPL uses start/end bytes to underline the
offending source range in the `rich`-formatted output.

---

## 4. Trade-off Analysis

### 4.1 Hand-written parser vs. parser generator

**Chosen: hand-written recursive descent + Pratt.**

| Dimension | Hand-written | `pest` (PEG) | `lalrpop` (LALR) | `nom` (combinator) |
|-----------|-------------|--------------|------------------|-------------------|
| Compile-time overhead | None | Moderate (proc-macro) | High (LALR table gen) | Minimal |
| Binary size | Smallest | Medium | Medium | Medium |
| Error recovery | Full control | Limited (PEG backtracks) | Grammar-level | Ad hoc |
| Span tracking | Trivial to add | Built-in | Built-in | Manual |
| Operator precedence | Pratt: clean | PEG: verbose | Explicit precedence levels | Recursive |
| Grammar evolution | Refactor code | Modify grammar file | Modify grammar file | Refactor code |
| Debugging | Step through code | Grammar traces | Hard | Combinator traces |
| Dependency | None | `pest` crate | `lalrpop` + build step | `nom` crate |
| Fuzz-friendly | Excellent | Good | Good | Good |

**Why hand-written wins here:** the Phase 1 grammar is small and stable. A PEG or LALR grammar
adds a build-time code-generation step, a runtime dependency, and an abstraction layer between
the grammar and the error recovery logic. For a CAS with good error messages as an explicit
requirement, control over the parser structure is more valuable than grammar terseness.

The Pratt algorithm handles REDUCE's operator precedence and right-associative exponentiation
cleanly in ~60 lines of code. A PEG grammar for the same would be longer and harder to read.

**Revisit trigger:** if the grammar grows significantly beyond the Phase 1 subset (Phase 2 adds
`procedure`, `for`, `while`, `let`-rules) and the hand-written parser becomes hard to extend, a
migration to `lalrpop` is the natural next step. The parser module's interface (`parse() →
ParseResult`) will not change, so the migration is internal.

### 4.2 Span storage: side-table vs. inline in ExprNode

**Chosen: side-table (`SpanMap`).**

Storing spans inline in `ExprNode` would add 8 bytes to every node (two `u32` offsets) — a 20%
increase in arena density for the common case. Worse, nodes produced by the simplifier,
differentiator, and polynomial engine have no source span. Inline spans would force all kernel
code to invent synthetic spans, or to leave span fields uninitialised.

The side-table approach means spans only exist for nodes that were parsed from source. Evaluation
errors that arise in non-parsed nodes (e.g., division by zero during simplification) use
`Span::SYNTHETIC` and emit a different error message that doesn't try to point to source.

Trade-off: looking up a span requires a `HashMap` lookup instead of a struct field read. This
cost is paid only at error-reporting time — never on the hot path (parsing, simplification,
differentiation). The cost is negligible where it matters.

### 4.3 Direct ExprPool emission vs. two-stage AST

**Chosen: direct emission into `ExprPool`.**

The alternative is to build an intermediate AST (a separate heap-allocated tree) and then walk
it to intern everything into the pool. This two-stage approach is common in industrial compilers
(parse → AST → HIR → MIR → …) because later stages need to annotate nodes freely.

For Monomix Phase 1, a two-stage approach would:
- Double the allocations during parsing (once for the AST, once for interning).
- Require a separate AST node type distinct from `ExprNode`, maintained in parallel.
- Add conversion code that is purely mechanical.

The only benefit is that the intermediate AST could be annotated during parsing (e.g., adding
type information). Since the Phase 1 parser has no semantic passes — it's purely syntactic — the
benefit doesn't apply.

**Revisit trigger:** if Phase 2 adds a semantic analysis pass (e.g., type-checking user
procedures, resolving `let`-rule scope), a proper two-stage pipeline with a typed HIR becomes
worthwhile.

### 4.4 Case normalisation: lexer vs. parser vs. ExprPool

**Chosen: lexer normalises identifiers to lowercase before interning.**

Alternative: intern the original case and apply normalisation in ExprPool's `symbol()` constructor.
This would make the interning invariant ("two symbols that should be equal produce the same
`ExprId`") harder to reason about — ExprPool's `intern()` is case-sensitive by design, and
adding case folding there would affect all uses of `InternedStr` (including function tags).

Normalising in the lexer localises the case policy to a single place and means the rest of the
kernel is purely case-sensitive on lowercase strings.

---

## 5. Scale, Limits, and Future Work

### 5.1 Grammar evolution for Phase 2

Phase 2 adds user procedures, `for`/`while`/`do` loops, `let`-rules, and script loading. The
Phase 1 parser is designed to make these additions straightforward:

- **Procedures** (`procedure f(x, y); ...; end;`): add a `parse_procedure()` branch in
  `parse_stmt()`. The `StmtKind` enum gains a `Procedure` variant. No Pratt changes needed.
- **`for`/`while`/`do`**: add keywords to the lexer; add `parse_for()` / `parse_while()` in
  `parse_stmt()`. Expressions inside loop bounds already parse with the existing Pratt code.
- **`let`-rules** (`let sin(~x)^2 = 1 - cos(~x)^2`): these require a pattern-matching
  sub-syntax (`~x` for wildcards). Add a `Token::Tilde` to the lexer and a `parse_pattern()`
  function that mirrors `parse_expr()` but allows `~` prefixed symbols. No changes to
  existing Pratt tables.
- **Script loading** (`load "file.red"`): pure Python concern (filesystem access lives in Tier
  1). The parser sees the contents of the loaded file as another `source` string; the kernel
  API is unchanged.

### 5.2 Per-request parser isolation (Phase 1.5 / Phase 2)

In Phase 1, all requests share one `ExprPool` behind a write lock. The lock contention window
is narrow for parsing (< 1 ms) but could grow if parsing large script files concurrently on the
MCP server. Phase 2 can introduce **per-request pools**:

```
request arrives → allocate ParsePool (small, temporary)
                 → parse into ParsePool
                 → transfer result ExprIds into shared SessionPool
                 → drop ParsePool
```

`ExprPool::merge(src: ExprPool) -> IdMap` would re-intern all nodes from `src` into `self`,
returning a mapping from old `ExprId`s to new ones. This is a single linear scan of the source
arena. The write lock on the session pool is held only during the merge step, not during
parsing.

This design requires the `ExprId` alias migration from `LocalExprId(u32)` to
`ContentExprId(u64)` (see `designs/expression-dag.md §5.4`) to make the merge step O(1) per
node instead of requiring deduplication — with content-addressed IDs, an `ExprId` from the
parse pool is valid in the session pool without relocation.

### 5.3 Incremental / streaming parsing (Phase 2+)

The REPL already handles multi-line input at the Python layer by accumulating text until a `;`
or `$` is seen. This is a sufficient model for Phase 1. For Phase 2, if large script files are
common, an incremental parser (parse as bytes arrive) could reduce latency. This would require
a resumable lexer — feasible with the hand-written design but not an immediate priority.

### 5.4 Unicode identifiers

Phase 1 assumes ASCII-only source (REDUCE's original syntax is ASCII). If Phase 3+ needs
Unicode identifiers (e.g., Greek letters for physics packages), the lexer needs to be updated
to scan Unicode scalar values and normalise to NFC before lowercasing. The rest of the parser
is unaffected because `InternedStr` is already an opaque index. This is a lexer-only change.

---

## 6. Testing Strategy

### 6.1 Unit tests (`cargo test`)

**Lexer tests (exhaustive token coverage):**
- Each token variant is produced by the correct source text.
- Spans are byte-accurate for each token.
- Case normalisation: `SIN`, `Sin`, `sin` all produce the same `Ident`.
- Comment stripping: `% this is a comment\n1+1` produces `Integer(1)`, `Plus`, `Integer(1)`.
- `comment` block: `comment this is a comment; 1+1;` skips to `;` and then produces the expression.

**Parser expression tests:**
- `1 + 2` → `Add([1, 2])`
- `1 + 2 * 3` → `Add([1, Mul([2, 3])])` (precedence)
- `2 ^ 3 ^ 4` → `Pow(2, Pow(3, 4))` (right-associativity)
- `-(-x)` → `x` (double-negation via pool normalisation)
- `x = y` → `Eq(x, y)`
- `(x + 1) * (x - 1)` → `Mul([Add([x, 1]), Add([x, Neg(1)])])`

**Parser statement tests:**
- `x := 2*y;` → `StmtKind::Assign { lhs: "x" }`, `OutputMode::Display`
- `x := 2*y$` → `OutputMode::Suppress`
- Multiple statements: `a := 1; b := 2;` → two `Stmt`s
- Assignment does not consume subsequent statement: `x := 1; y;` → two stmts

**Built-in parsing tests:**
- `df(x^2, x)` → derivative node
- `df(x^2*y, x, y)` → partial derivative sequence
- `sub(x = 5, x^2 + 1)` → substitution node
- `simplify(x + x)` → simplify-tagged call

**Error recovery tests:**
- `1 +; 2;` → one diagnostic, one stmt (`2`)
- `)x;` → one diagnostic, one stmt (`x`)
- `::; 1+1;` → one diagnostic, one stmt (`1+1`)
- Empty input: zero stmts, zero diagnostics

### 6.2 Property-based tests (`proptest`)

- **Round-trip:** for any expression `e` constructed programmatically from the expression DAG,
  `unparse(e)` followed by `parse(unparse(e))` produces a structurally identical `ExprId`
  (requires an `unparse` function; implement it as a prerequisite).
- **No panics:** feed `proptest`-generated arbitrary strings to `parse()`; assert it never
  panics and always returns a `ParseResult`.
- **Span bounds:** for every `ExprId` in `span_map`, `span.end ≤ source.len()` and
  `span.start ≤ span.end`.
- **Diagnostics are non-overlapping:** no two `Diagnostic` spans in the same `ParseResult`
  overlap (each error recovery consumes one segment).

### 6.3 Benchmarks (`criterion`)

- Parse a 100-term polynomial expression: `a1*x^100 + a2*x^99 + ... + a100` (~1000 tokens).
  Target: <500 µs.
- Parse a 1 KB interactive session transcript (20 statements). Target: <200 µs.
- Parse a 100 KB script file (simulated `load` scenario). Target: <20 ms.
- Lexer throughput in isolation (tokens/sec). Target: ≥500K tokens/sec.
- **Pessimal input** — 1000 random tokens that trigger error recovery on most statements.
  Target: <2 ms. Confirms recovery and diagnostic emission stay within budget under fuzz-like
  conditions; if this regresses, `Diagnostic.message` allocation is the likely cause (§3.6).

### 6.4 Fuzz testing (`cargo-fuzz`)

- Fuzz target: `parse(arbitrary_bytes, &mut pool)`. Assert no panics, no `unwrap()` failures,
  `diagnostics.len() + statements.len() > 0` for any non-empty input.
- Run ≥1 hour before each release (SCOPE.md §1.12 success criterion).
- Seed corpus: all `.tst` files from the legacy REDUCE corpus.

### 6.5 Golden corpus tests (`pytest`)

- A curated subset of `legacy/reduce-algebra-code-r7357-trunk/packages/*/*.tst` files that
  fall within the Phase 1 grammar subset.
- For each file, parse successfully (zero diagnostics) and verify the statement count matches
  the expected count (hand-audited once, then frozen).
- Run as part of `pytest tests/test_golden/` in CI.

---

## 7. Action Items

### Phase 1 — Core implementation

1. [ ] Create `crates/monomix-kernel/src/lexer.rs` with `Token`, `TokenKind`, `Span`, and
       `Lexer` using a 2-slot `ArrayVec` lookahead buffer (§3.1)
2. [ ] Implement `Token::SmallInt(i64)` / `Token::BigInt(Box<BigInt>)` split — try `i64::from_str`
       first, fall back to `BigInt` only on overflow (§3.1)
3. [ ] Implement `TokenKind` (Copy, payload-free) and `Lexer::peek_kind()` for cheap inner-loop
       dispatch (§3.1, §3.2)
4. [ ] Decouple lexer from `ExprPool`: emit `Token::Ident(Span)`; intern + lowercase happens at
       parser use-site (§3.1)
5. [ ] Canonicalize `^` and `**` to single `Token::Pow` at lex time (§3.1)
6. [ ] Reject `inf`/`nan` float literals at lex time; enforce 1024-byte caps on identifier and
       numeric-literal length (§3.1)
7. [ ] Create `crates/monomix-kernel/src/parser/ast.rs` with `Stmt`, `StmtKind`, `OutputMode`,
       `Diagnostic`, `DiagnosticCode`, `ParseResult`, `SpanMap`. `DiagnosticCode::UnexpectedToken`
       carries `TokenKind` not `String` (§3.6)
8. [ ] Implement Pratt expression parser in `crates/monomix-kernel/src/parser/expr.rs` with
       `TokenKind`-keyed binding-power tables. No `Token::clone()` in the inner loop (§3.2)
9. [ ] Implement statement parser (`parse_stmt`, `parse_assign_stmt`, `parse_expr_stmt`,
       paren-depth-aware `synchronise` for error recovery) in `crates/monomix-kernel/src/parser/stmt.rs`
       (§3.4, §3.6)
10. [ ] Implement `BuiltinTable` — pre-intern `df`/`int`/`solve`/`factor`/`expand`/`simplify`/`sub`
        into `InternedStr` indices; dispatch via integer equality, not string compare (§3.3)
11. [ ] Implement built-in dispatch handlers: `df`, `sub`, `simplify`, `expand`, `solve` (full);
        `int`, `factor` (UnsupportedStub) (§3.3)
12. [ ] Implement `Span` side-table (`SpanMap` as `FxHashMap` for now); thread it through parse
        calls (§3.5)
13. [ ] Wire up `parse()` public entry point in `crates/monomix-kernel/src/lib.rs`
14. [ ] Implement `KernelError::Parse(Vec<Diagnostic>)` variant and map to `monomix.ParseError`
        in the PyO3 boundary (`crates/monomix-py/src/error.rs`)

### Phase 1 — Verification

15. [ ] Write unit tests for all lexer token/span cases including `SmallInt`/`BigInt` boundary,
        `inf`/`nan` rejection, length caps (§6.1)
16. [ ] Write unit tests for all expression precedence and associativity cases (§6.1)
17. [ ] Write unit tests for statement parsing, assignment, terminator, multi-statement (§6.1)
18. [ ] Write unit tests for all built-in argument forms (`df`, `sub`, stubs) (§6.1)
19. [ ] Write unit tests for error recovery, including paren-depth-aware sync points (§6.1)
20. [ ] Write `proptest` suite (§6.2)
21. [ ] Set up criterion benchmarks including pessimal-input case (§6.3)
22. [ ] Set up `cargo-fuzz` target with legacy `.tst` seed corpus (§6.4)
23. [ ] Curate and commit golden corpus test set (§6.5)
24. [ ] Verify: `cargo-fuzz` ≥1 h with no panics (Phase 1.12 success criterion)

### Phase 2 — Optimisation and grammar extensions (deferred)

25. [ ] If profiling shows pressure: replace `SpanMap = FxHashMap` with sorted
        `Vec<(ExprId, Span)>` or parallel arrays (§3.5)
26. [ ] Reconsider `Diagnostic.message: String` — replace with `Option<Cow<'static, str>>` or
        remove entirely once static rendering table is in place (§3.6)
27. [ ] Add `Token::KwProcedure`, `KwFor`, `KwWhile`, `KwDo`, `KwLet`, `KwEnd` to lexer (§5.1)
28. [ ] Implement `parse_procedure()` and `parse_for()` / `parse_while()` in stmt parser (§5.1)
29. [ ] Add `Token::Tilde` and `parse_pattern()` for `let`-rules (§5.1)
30. [ ] Evaluate per-request pool isolation for MCP server (§5.2) based on Phase 1.5 profiling
31. [ ] Incremental REPL re-parse: track byte offset of last completed statement, skip past it
        on continuation input (§5.3)
