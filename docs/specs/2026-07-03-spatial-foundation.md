# SPECIFICATION — Fixed-Point 2.5D Spatial Foundation (FINAL, canon-clean)

Status: ready for user approval. Every REQUIRED change from both plan-mode guardians is resolved at the root below; the two residual items needing a user nod are listed as OPEN QUESTIONS in §7. Nothing is silently deferred.

---

## 1. Summary + locked model

mu-core's world is a single continuous 2D ground plane addressed in **exact signed integer sub-units** (fixed-point, `i64`), never floating point, so the same position, distance test, and "is-it-in-front" answer come out **bit-for-bit identical** on native, wasm, and FFI. All gameplay and combat resolve on the plane. Positions are continuous; facing is continuous (a nonzero direction vector — the 8-way `Direction` enum is retired as a live type). Flight is a two-state traversal mode (`Grounded`/`Flying`) whose only rule is whether the tile walk-grid check applies — no altitude, no air combat, no anti-air. Walkability stays a 256×256 tile bitset under the continuous positions. Classic MU tile coordinates (`u8`×`u8`) survive **only** as an authoring unit, converted to world-space once at the load boundary (integer→integer, never through a float, never on live entities). Every type is `serde` + integer only; every predicate is a pure, total, deterministic value-in/value-out function with no clock, RNG, host concept, or hidden state.

**Fixed-point format is `Q40.24` in `i64`: `UNITS_PER_TILE = 65536 = 2^16` sub-units per classic tile** (canon-review REQUIRED #1 — 16 fractional bits, not the draft's 8, so the movement wave's narrowing ops keep Q32.32-class fractional richness at zero range cost; the cascade to the cone-denominator bound and wire values is carried through §2/§4 below).

---

## 2. Types & API — the port

All live in **`core/src/components/spatial.rs`** unless marked otherwise. Doc comments elided (real code carries them; `missing_docs` is on).

### Constants

```rust
pub const TILE_SHIFT: u32 = 16;
pub const UNITS_PER_TILE: i64 = 1 << TILE_SHIFT;        // 65_536 = 2^16
pub const HALF_TILE: i64 = 1 << (TILE_SHIFT - 1);       // 32_768
pub const WORLD_EXTENT: i64 = 256 * UNITS_PER_TILE;     // 16_777_216 = 2^24
pub const RADIUS_MAX: u64 = 2 * (WORLD_EXTENT as u64);  // 33_554_432 = 2^25  (see note)
pub const CONE_DEN_MAX: u64 = 1 << 27;                  // 134_217_728 = 2^27
```

`RADIUS_MAX as u64` on `WORLD_EXTENT` is written `2 * (WORLD_EXTENT.unsigned_abs())` in real code (no `as`); shown here for range only.

### 2.1 `Fixed` — the fixed-point scalar

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(from = "i64", into = "i64")]        // wire = bare integer; total both ways
pub struct Fixed(i64);
```

- **Wire form:** bare integer, e.g. `196736`. `From<i64>` (total — every `i64` is a valid raw scalar; `Fixed` itself is unbounded, bounds live on `WorldPos`).
- **Constructors / accessors:** `const fn from_raw(i64) -> Fixed`, `const fn raw(self) -> i64`.
- **Authoring convenience** (arch-review REQUIRED #4 — parenthesized shift + no default-overflow `+`):
  ```rust
  pub fn from_tile_parts(whole_tiles: i64, sub_units: i64) -> Fixed {
      Fixed((whole_tiles << TILE_SHIFT).saturating_add(sub_units))
  }
  ```
- **Linear ops (Wave A public surface) — saturating, total, deterministic:**
  ```rust
  pub fn add(self, o: Fixed) -> Fixed { Fixed(self.0.saturating_add(o.0)) }
  pub fn sub(self, o: Fixed) -> Fixed { Fixed(self.0.saturating_sub(o.0)) }
  pub fn scale(self, k: i64)  -> Fixed { Fixed(self.0.saturating_mul(k)) }
  ```
  For every in-world value the saturation branch is provably unreachable (§4 proof); it is the *defined total fallback*, never the live path. `checked_*`→`Option` is banned (branch that can differ per target); default `+`/`-`/`*` is banned (debug-panic/release-wrap = de-facto `panic!` + cross-profile divergence).

- **Narrowing mul/div — contract pinned now, NOT Wave A surface (ships with its first consumer in the movement wave, Iron Law 4):**
  ```rust
  pub struct NonZeroFixed(Fixed);   // invariant: inner != 0; new() rejects zero -> SpatialError

  // round-to-nearest, ties AWAY FROM ZERO — the SINGLE rounding rule, applied to BOTH mul and div
  // (canon-review REQUIRED #2: div no longer truncates while mul rounds; one rule everywhere;
  //  away-from-zero is bias-free about zero, unlike round-half-up)
  fn round_shift(p: i128, shift: u32) -> i128;   // p / 2^shift, nearest, ties away from 0
  fn round_div(n: i128, d: i128)     -> i128;     // n / d,      nearest, ties away from 0
  fn saturate_i64(x: i128) -> i64 {               // total saturating narrow, NO as-cast, NO unwrap
      match i64::try_from(x) {
          Ok(v)  => v,
          Err(_) => if x < 0 { i64::MIN } else { i64::MAX },  // the saturation SEMANTIC, not a guard
      }
  }
  pub fn mul(self, o: Fixed) -> Fixed {
      Fixed(saturate_i64(round_shift(i128::from(self.0) * i128::from(o.0), TILE_SHIFT)))
  }
  pub fn div(self, d: NonZeroFixed) -> Fixed {
      Fixed(saturate_i64(round_div(i128::from(self.0) << TILE_SHIFT, i128::from(d.0.0))))
  }
  ```
  Widen (`i128::from`) → operate → round-nearest → saturate-narrow. Divisor is `NonZeroFixed` so div-by-zero is unrepresentable (no fallible op in core). The `Err(_)` arm of `saturate_i64` is the saturation semantic (reachable by the op's contract), not a "should never happen" branch — no rule violation.

### 2.2 `WorldPos` / `WorldVec` — point and offset are distinct types

```rust
#[derive(Serialize, Deserialize)] struct Vec2Wire { x: i64, y: i64 }   // private wire mirror

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "Vec2Wire", into = "Vec2Wire")]   // bounds-checked on the wire
pub struct WorldPos { x: Fixed, y: Fixed }           // invariant: each component ∈ [0, WORLD_EXTENT]

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(from = "Vec2Wire", into = "Vec2Wire")]       // unbounded, total
pub struct WorldVec { x: Fixed, y: Fixed }
```

- **Wire form (both):** `{"x":<i64>,"y":<i64>}`. `WorldPos::try_from` rejects any component outside `[0, WORLD_EXTENT]` → `SpatialError::ComponentOutOfBounds`.
- **Constructors:**
  ```rust
  WorldPos::new(x: Fixed, y: Fixed) -> Result<WorldPos, SpatialError>   // parse boundary; bounds-checked
  WorldPos::clamped(x: i64, y: i64) -> WorldPos                         // total; clamps each to [0, WORLD_EXTENT]
  WorldVec::new(x: Fixed, y: Fixed) -> WorldVec                         // total
  fn x(self) -> Fixed;  fn y(self) -> Fixed;                            // on both
  ```
- **Typed affine algebra — only the legal combinations exist; illegal ones do not compile** (Iron Law 3):
  ```rust
  impl Sub<WorldPos> for WorldPos { type Output = WorldVec; }   // Pos − Pos → Vec  (offset_from)
  impl Add<WorldVec> for WorldPos { type Output = WorldPos; }   // Pos + Vec → Pos  (translated)
  impl Add<WorldVec> for WorldVec { type Output = WorldVec; }
  impl Sub<WorldVec> for WorldVec { type Output = WorldVec; }
  impl WorldVec { pub fn scale(self, k: i64) -> WorldVec; }
  // NO Add<WorldPos> for WorldPos.
  ```
  **`Pos + Vec` clamps into `[0, WORLD_EXTENT]`, it does NOT saturate at `i64` range** (arch-review REQUIRED #2 — otherwise a `WorldPos` past `WORLD_EXTENT` is reachable through safe arithmetic and the invariant is a lie):
  ```rust
  fn translated(self, v: WorldVec) -> WorldPos {
      WorldPos::clamped(self.x.raw().saturating_add(v.x.raw()),
                        self.y.raw().saturating_add(v.y.raw()))
  }
  ```
- **Distance — no sqrt anywhere:**
  ```rust
  pub fn dot(self, o: WorldVec) -> i128;                 // (WorldVec) widen-multiply-accumulate in i128
  pub fn length_sq(self) -> DistanceSq;                  // (WorldVec) = dot(self,self)
  pub fn distance_sq(self, o: WorldPos) -> DistanceSq;   // (WorldPos) uses unsigned_abs, no try_from/unwrap
  pub fn within_range(self, o: WorldPos, r: Radius) -> bool;   // distance_sq(o) <= r.squared()
  ```
  ```rust
  fn distance_sq(self, o: WorldPos) -> DistanceSq {
      let dx = u128::from((self.x.raw() - o.x.raw()).unsigned_abs());   // |·| ≤ 2^24, no overflow
      let dy = u128::from((self.y.raw() - o.y.raw()).unsigned_abs());
      DistanceSq(dx * dx + dy * dy)                                     // ≤ 2^49, exact in u128
  }
  ```
  This is the **only** distance predicate — no `length()`/sqrt exists in the foundation (the `within_range ⇔ dist_sq ≤ r²` equivalence removes every irrational path). Integer `isqrt` arrives only if/when normalize-to-speed lands in the movement wave, never `f64::sqrt`.

### 2.3 `Radius` / `DistanceSq` — linear vs squared, non-mixable

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "u64", into = "u64")]
pub struct Radius(u64);                        // arch-review REQUIRED #1: u64, NOT i64

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DistanceSq(u128);                   // NOT serialized (a computed return, never on the wire)
```

- `Radius::new(u64) -> Result<Radius, SpatialError>` rejects `> RADIUS_MAX` → `RadiusOutOfBounds`. Wire = bare integer.
- `Radius::squared(self) -> DistanceSq` = `DistanceSq(u128::from(self.0).pow(2))` — lossless (`u64→u128`), `≤ 2^50`, lint-clean (no sign-loss cast; the `i64` repr in the draft would have tripped `clippy::cast_sign_loss` under `-D warnings`).
- `DistanceSq::get(self) -> u128`.

### 2.4 `Facing` — continuous heading as a nonzero direction vector

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "Vec2Wire", into = "Vec2Wire")]
pub struct Facing(WorldVec);   // invariants: not (0,0); each |component| ≤ WORLD_EXTENT
```

- **Wire form:** `{"x":<i64>,"y":<i64>}`; zero vector rejected on the wire → `SpatialError::ZeroFacing`; out-of-bounds component → `ComponentOutOfBounds`.
- `Facing::new(WorldVec) -> Result<Facing, SpatialError>` — rejects zero and bounds. Magnitude is **not** normalized; the cone test is magnitude-invariant.
- **Documented preconditions (canon-review REQUIRED #3):**
  1. **Snap-facing only.** Facing is set instantaneously (snapped to a direction vector, e.g. `target − pos`). It is never *gradually rotated*. If turn-rate-limited rotation ("turn toward target at N°/tick") is ever required, the canonical representation is a **Binary Angle (BAM)** — not this vector form — and this type must be revisited then.
  2. **Cone half-widths are ≤ 90°.** The `d ≥ 0` gate plus squared-cosine (which discards `cos θ`'s sign) makes any half-width > 90° unrepresentable; `ConeHalfWidth`'s `num ≤ den` invariant already bounds this.
- **`Facing::toward(from, to)`** (= `Facing::new(to − from)`, fails on coincident points) is trivial sugar with no foundation consumer — ships with its first consumer, not now. The foundation returns `Result`; a "what to face when there is no direction" fallback is **service policy**, folded by the caller's explicit `match`, never baked in.
- **Cone predicate** (angle only — exact integer, dot form):
  ```rust
  pub fn frontal_contains(&self, apex: WorldPos, target: WorldPos, half: ConeHalfWidth) -> bool {
      let (fx, fy) = (i128::from(self.0.x().raw()), i128::from(self.0.y().raw()));
      let (vx, vy) = (i128::from(target.x().raw() - apex.x().raw()),
                      i128::from(target.y().raw() - apex.y().raw()));
      let d = fx * vx + fy * vy;
      if d < 0 { return false; }                       // behind the apex → out
      let f_sq = fx * fx + fy * fy;
      let v_sq = vx * vx + vy * vy;
      let lhs = d * d * i128::from(half.den_get());     // cos²∠ · |F|²|V|² cleared of denominator
      let rhs = i128::from(half.num()) * f_sq * v_sq;
      lhs >= rhs                                        // inclusive edge
  }
  ```
  Derivation: `cos∠ = d/(|F||V|) ≥ cosθ`, gated `d≥0`, squared to `d² ≥ cos²θ·|F|²·|V|²`, `cos²θ = num/den` cleared. Magnitude-invariant (both sides scale with `|F|²`). Bounds (§4): every operand `< 2^127`, exact in `i128`. **No LUT, no trig, no sqrt, no normalize** — sidesteps the most-cited fixed-point trap.

### 2.5 `ConeHalfWidth` — exact squared-cosine rational

```rust
use core::num::NonZeroU64;
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "ConeWire", into = "ConeWire")]
pub struct ConeHalfWidth { num: u64, den: NonZeroU64 }   // cos²θ = num/den
```

- Invariants (arch-review SUGGESTED #6 — `den` as `NonZeroU64` removes the zero-check): `num ≤ den`, `den ≤ CONE_DEN_MAX (2^27)`.
- `new(num: u64, den: NonZeroU64) -> Result<ConeHalfWidth, SpatialError>` → `ConeRatioInvalid` on `num > den` or `den > CONE_DEN_MAX`.
- Named consts: `DEG_45 = {num:1, den:2}` (cos²45°=½), `DEG_90 = {num:0, den:1}` (cos²90°=0). Larger θ ⇒ smaller `num`.
- **Wire form:** `{"num":1,"den":2}`.

### 2.6 `WorldRect` and `Region`

```rust
#[derive(Serialize, Deserialize)] struct RectWire { min: WorldPos, max: WorldPos }

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "RectWire", into = "RectWire")]
pub struct WorldRect { min: WorldPos, max: WorldPos }   // invariant: min.x ≤ max.x, min.y ≤ max.y

impl WorldRect {
    pub fn new(min: WorldPos, max: WorldPos) -> Result<WorldRect, SpatialError>;  // RectInverted on reversed
    pub fn spanning(a: WorldPos, b: WorldPos) -> WorldRect;   // total: componentwise min/max, cannot invert
    pub fn contains(&self, p: WorldPos) -> bool;             // inclusive on all four edges
}
```

`spanning` is the total constructor the tile→world conversion uses (it sorts corners, so no inverted rect is representable and no fallible-Result-with-unreachable-Err arises). `new` is the parse-boundary validating constructor mirroring `geometry.rs`'s reversed-edge rejection.

```rust
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Region {
    Circle { center: WorldPos, radius: Radius },
    Rect   { rect: WorldRect },
    Cone   { apex: WorldPos, facing: Facing, half_width: ConeHalfWidth, range: Radius },
}

impl Region {
    pub fn contains(&self, p: WorldPos) -> bool {
        match self {
            Region::Circle { center, radius }          => center.within_range(p, *radius),
            Region::Rect   { rect }                     => rect.contains(p),
            Region::Cone   { apex, facing, half_width, range } =>
                apex.within_range(p, *range) && facing.frontal_contains(*apex, p, *half_width),
        }
    }
}
```

Each field is self-validating, so `Region` derives serde with **no custom mirror** (composition beats duplication). `contains` is total and exhaustive — a new variant breaks the build (`wildcard_enum_match_arm`). **Wire pins:**
- `{"kind":"circle","center":{"x":0,"y":0},"radius":5}`
- `{"kind":"rect","rect":{"min":{"x":10,"y":10},"max":{"x":20,"y":30}}}`
- `{"kind":"cone","apex":{"x":0,"y":0},"facing":{"x":1,"y":0},"half_width":{"num":1,"den":2},"range":10}`

### 2.7 `Movement` — traversal mode (new file `core/src/components/movement.rs`)

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Movement { Grounded, Flying }

impl Movement {
    pub fn checks_walkability(self) -> bool {
        match self { Movement::Grounded => true, Movement::Flying => false }
    }
}
```

- **Wire form:** bare snake-case string `"grounded"` / `"flying"` (consistent repo rule: fieldless enums → string, fieldful → `#[serde(tag="kind")]`).
- The **only** exposed rule is `checks_walkability`, derived from the variant, never stored (no `is_flying: bool` companion). Nothing about altitude/air-combat/anti-air. Exhaustive match; a third mode breaks the build.
- **Wave-B watch-point (arch-review SUGGESTED #8):** if this predicate ever takes a `WalkGrid` argument or returns anything but a classification of the mode itself, it has become a *rule* and must move to the movement service.

### 2.8 Tile authoring types + conversions (new file `core/src/components/tile.rs`)

Authoring vocabulary — survives **only** as the data-authoring unit; retired as live types. **`tile.rs` depends on `spatial.rs`, never the reverse** (arch-review SUGGESTED #5 — the permanent module stays unaware tiles exist; the transient authoring side knows how to project itself into world space).

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TileCoord { x: u8, y: u8 }          // was geometry::Point

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "TileAreaWire", into = "TileAreaWire")]
pub struct TileArea { /* x1,y1,x2,y2: u8 */ }  // was geometry::Rect; x1≤x2, y1≤y2 at construction

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TileFacing { /* 8-way */ }            // was geometry::Direction; authoring-only now

pub enum Footprint { /* moved, over TileCoord/TileArea */ }

pub enum TileError { AreaInverted { /* … */ } }   // arch-review SUGGESTED #7: geometry::GeometryError → TileError (pinned)
```

Renames only — **JSON is unchanged** (serde keys are field/variant names, not type names), so zero data migration.

**Conversions (Wave A) — the sole sanctioned float-free integer→integer boundary:**
```rust
impl TileCoord {
    pub fn to_world(self) -> WorldPos {                 // tile CENTER anchor (D6)
        WorldPos::clamped(i64::from(self.x) * UNITS_PER_TILE + HALF_TILE,
                          i64::from(self.y) * UNITS_PER_TILE + HALF_TILE)
    }
    pub fn from_world(pos: WorldPos) -> TileCoord {     // floor; pos ≥ 0, arithmetic >> floors
        let tx = (pos.x().raw() >> TILE_SHIFT).clamp(0, 255);   // clamp: extent>>16 == 256
        let ty = (pos.y().raw() >> TILE_SHIFT).clamp(0, 255);
        TileCoord { x: narrow_u8(tx), y: narrow_u8(ty) }        // tx∈[0,255]; saturating u8 narrow, no as-cast
    }
}
impl TileArea {
    pub fn to_world(self) -> WorldRect {                // whole-cell coverage
        let min = WorldPos::clamped(i64::from(self.x1()) * UNITS_PER_TILE,
                                    i64::from(self.y1()) * UNITS_PER_TILE);
        let max = WorldPos::clamped((i64::from(self.x2()) + 1) * UNITS_PER_TILE,
                                    (i64::from(self.y2()) + 1) * UNITS_PER_TILE);
        WorldRect::spanning(min, max)                  // total, cannot invert
    }
}
```
All widening is `i64::from`/`u128::from`; every narrow is a checked/saturating conversion — no `as`. Round-trip `TileCoord::from_world(t.to_world()) == t` holds for all `256×256` tiles (`HALF_TILE < UNITS_PER_TILE` ⇒ center floors back to origin tile).

### 2.9 `WalkGrid` — occupancy bitset (shape fixed Wave A; loaded/consumed Wave B)

```rust
pub struct WalkGrid([u64; 1024]);   // 65_536-bit set, one bit per tile
impl WalkGrid {
    pub fn walkable(&self, tile: TileCoord) -> bool {
        let bit  = (u32::from(tile.y()) << 8) | u32::from(tile.x());   // 0..65_536
        let word = (bit >> 6) as usize;                               // written via usize::try_from in code
        let mask = 1u64 << (bit & 63);
        self.0.get(word).is_some_and(|w| w & mask != 0)               // total: slice::get + is_some_and, no [i], no unwrap
    }
}
```

### 2.10 `SpatialError` (peer of `UnitError`/`TileError`)

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]   // + Display + core::error::Error
pub enum SpatialError {
    ComponentOutOfBounds { axis: Axis, value: i64 },
    RectInverted { min_x: i64, min_y: i64, max_x: i64, max_y: i64 },
    ZeroFacing,
    RadiusOutOfBounds { value: u64 },
    ConeRatioInvalid { num: u64, den: u64 },
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Axis { X, Y }
```

---

## 3. Canon decisions table

| Area | Chosen canonical form | One-line justification | Citation |
|---|---|---|---|
| **Q-format** | `Q40.24` in `i64` — `UNITS_PER_TILE = 2^16`; squared work in `i128`/`u128` | 16 fractional bits give Q32.32-class richness for the movement wave's narrowing ops at zero range cost, since storage is `i64` regardless | Tonc (document your fixed-point choice); Quantum `FP`; Book of Hook / Mike Day (widen the accumulator) |
| **Overflow policy** | Saturating, total, provably unreachable for in-world values | `checked_*`→`Option` is a per-target-divergent branch (banned); default `+/-/*` is debug-panic/release-wrap (banned); saturating clamps to a map edge rather than teleporting | MathWorks (define one policy for cross-platform consistency); speytech (saturate over wrap) |
| **Rounding** | Round-to-nearest, ties **away from zero** — one rule for both `mul` and `div` | Single documented rule kills the desync trap; away-from-zero is bias-free about zero (unlike round-half-up) | Tonc (add half an LSB, decide once, apply everywhere) |
| **Distance / range** | Squared-distance compare, no sqrt; `DistanceSq(u128)` distinct from linear `Radius(u64)` | `dx²+dy² ≤ r²` is exact, sqrt-free, and the only irrational op removed; squared is a widening op with its own newtype | Szabó; GameDev.net; Book of Hook; Mike Day |
| **Facing** | Nonzero direction vector (zero rejected at construction), snap-set only, cones ≤ 90° | Vector form keeps the cone test in exact integer dot/cross with no trig LUT; magnitude-invariant test sidesteps the normalize trap | Brief §4/§5 (dot-not-angle; unit-vector form); BAM noted as the fallback if gradual turning is ever needed |
| **Trig** | None. No sin/cos, no LUT, no CORDIC in the foundation | Cone/range/region predicates are all integer dot/compare; a LUT would add a determinism surface for zero benefit | Brief §4 ("dot product, not angle math") |
| **Regions** | Flat `kind`-tagged enum, total exhaustive `contains`, composed from self-validating fields | Standard shape-predicate enum; a new variant must break the `contains` match | Project enum canon; `Rect::contains`/`Interval::contains` precedent |
| **Tile→world scale** | `<< 16` to world, `>> 16` floor back; integer→integer, load boundary only, center-anchor spots / whole-cell areas | Power-of-two shift is exact and deterministic; float conversion inside the sim desyncs 100% | Quantum (convert at edit time, never in-sim); brief mistake #1 |

---

## 4. Determinism & world-bounds proof

**Claim:** every foundation operation on in-world values is exact integer arithmetic with no overflow and no saturation on the live path, hence bit-identical on native/wasm/FFI.

Ranges (all powers of two; `2^127` = `i128` positive limit, `2^128` = `u128` limit):

| Quantity | Bound | Fits |
|---|---|---|
| Position component | `[0, 2^24]` | `i64` |
| Position difference component | `|·| ≤ 2^24` | `i64` (no overflow) |
| `add`/`sub`/`scale` of in-world values | `|·| < 2^25 ≪ 2^63` | `i64`; saturation **unreachable** |
| `dist_sq = dx²+dy²` | `≤ 2^49` | `u128` exact (also `u64`) |
| `Radius` | `≤ 2^25` → `r² ≤ 2^50` | `u128` exact |
| Facing component | `|·| ≤ 2^24` → `|F|² ≤ 2^49` | `i128` |
| Cone `d = F·V` | `|d| ≤ 2^49` → `d² ≤ 2^98` | `i128` |
| Cone LHS `d²·den` (`den ≤ 2^27`) | `≤ 2^125` | `i128` exact (`< 2^127`) |
| Cone RHS `num·|F|²·|V|²` (`num ≤ 2^27`) | `≤ 2^125` | `i128` exact (`< 2^127`) |

The cone denominator bound `CONE_DEN_MAX = 2^27` is the direct cascade of choosing 16 fractional bits (`2^27 · 2^49 · 2^49 = 2^125 < 2^127`); it leaves a two-bit margin and still gives `cos²` resolution of `1/2^27` — far finer than any gameplay cone.

**Why bit-identical across targets:** all arithmetic is integer with explicit widening via `From` (no lossy `as`, no float, no `unsafe`); overflow is unreachable for in-world values so no platform ever hits the saturating fallback on the live path; narrowing back to `Fixed`/`u8` is a defined saturating conversion identical on every target; rounding is one rule (nearest, ties away from zero) applied identically to `mul` and `div`. No clock, no RNG, no hidden state, no interior mutability. This is the whole reason fixed-point was locked in — the illegal state (overflow / divergence) is made unrepresentable by **range**, not by a runtime check.

---

## 5. Migration

### Wave A (this spec — mechanical rename + new modules; no data-file change)

| File / item | Before | After |
|---|---|---|
| `core/src/components/geometry.rs` | `Point, Rect, GeometryError, Footprint, Direction` | **deleted**, split into the three modules below |
| `core/src/components/spatial.rs` | — | **new**: `Fixed, Axis, WorldPos, WorldVec, Radius, DistanceSq, Facing, ConeHalfWidth, WorldRect, Region, SpatialError, WalkGrid`; consts `TILE_SHIFT, UNITS_PER_TILE, HALF_TILE, WORLD_EXTENT, RADIUS_MAX, CONE_DEN_MAX`. (`NonZeroFixed`, `Fixed::mul`/`div` specified, **not exposed** until the movement wave.) |
| `core/src/components/tile.rs` | — | **new**: `TileCoord`(was `Point`), `TileArea`(was `Rect`), `TileFacing`(was `Direction`), `Footprint`, `TileError`(was `GeometryError`); `TileCoord::{to_world,from_world}`, `TileArea::to_world` |
| `core/src/components/movement.rs` | — | **new**: `Movement` + `checks_walkability` |
| `core/src/components/mod.rs` | `pub mod geometry;` | `pub mod spatial; pub mod tile; pub mod movement;` |
| `core/src/data/map_definitions.rs` | `use geometry::{Point, Rect}`; `SoccerPitch.{ground,left_goal,right_goal}: Rect`, `.{left_spawn,right_spawn}: Point` | `use tile::{TileCoord, TileArea}`; fields → `TileArea`/`TileCoord` (rename only) |
| `core/src/data/gates_warps.rs` | `use geometry::{Direction, Rect}`; `SpawnGate/TargetGate.area: Rect`, `.direction: Option<Direction>`; `EnterGate.area: Rect` | `use tile::{TileFacing, TileArea}`; `area: TileArea`, `direction: Option<TileFacing>` (rename only) |
| `core/src/data/spawns.rs` | `use geometry::{Direction, Point, Rect}`; `SpawnPlacement::{Fixed{position,facing}, Spot{position}, Area{area}}` | `use tile::{TileFacing, TileCoord, TileArea}` (rename only) |
| `core/src/data/atlas.rs` | `use geometry::{Direction, Point, Rect}`; `Landing{area:Rect, facing:Option<Direction>}`; `enter_gate_at(map, tile: Point)` | `use tile::{TileFacing, TileCoord, TileArea}` (rename only; still tile-space) |

No `data/*.json` change. The only live `Direction` consumers are `atlas::Landing.facing`, `gates_warps::{SpawnGate,TargetGate}.direction`, `spawns::SpawnPlacement::Fixed.facing`; the only `Point`/`Rect` consumers are `map_definitions`, `gates_warps`, `spawns`, `atlas` (verified — the `Direction`/`Facing`/`Fixed`/`FlyingMount` hits in `skills.rs`/`monster_definitions.rs`/`item` code are unrelated variant names).

### Wave B (named complete follow-up — not built here)

| Target | Before | After |
|---|---|---|
| `atlas::Landing.area` | `TileArea` | `WorldRect` via `TileArea::to_world` at `Atlas::parse` |
| `atlas::Landing.facing` (**canon D9**) | `Option<TileFacing>` | `Facing` (8-way sentinel resolved to a direction vector at load) |
| `atlas` resolved spawn positions | `TileCoord` | `WorldPos` via `TileCoord::to_world` |
| `atlas::enter_gate_at` query | `TileCoord` | `WorldPos` (world-space live query) |
| walk grid | `data/terrain/*.bin` (65 536-byte, unloaded) | `WalkGrid` loaded per map; consumed by the movement service via `Movement::checks_walkability` |
| Flight transition | — | `FlightChange` input + `MovementModeChanged` outcome + `apply_flight_change` service (see §8) |

---

## 6. Test plan

Hand-rolled deterministic sampling (repo's plain-`#[test]` style; only `serde_json` dev-dep). Concrete distance/cone values below are **raw sub-units** (scale-independent math tests, unchanged from the BDD); tile-derived expected values are recomputed for `S = 65536`.

### Smart-constructor round-trips → BDD "Constructing a fixed-point scalar", "Facing cannot have no direction", "rect rejects inverted"
- `Fixed::from_tile_parts(3, 128).raw() == 196_736` (= `3<<16 + 128`); `Fixed::from_raw(-5000).raw() == -5000`.
- `Facing::new((0,0))` → `Err(ZeroFacing)`; `Facing::new` out-of-bounds component → `Err(ComponentOutOfBounds)`.
- `WorldRect::new(min beyond max on either axis)` → `Err(RectInverted)`.
- `Radius::new(> RADIUS_MAX)` → `Err(RadiusOutOfBounds)`.
- `WorldPos::new`/wire beyond `[0, WORLD_EXTENT]` → `Err(ComponentOutOfBounds)`.

### Vector algebra → BDD "Vector add / sub / scale"
- `(10,20)+(3,-5) == (13,15)`; `(10,20)−(4,32) == WorldVec(6,-12)`; `(6,-12).scale(3) == (18,-36)`.
- Property (sampled grid): `(A+d)−d == A`; `A−A == 0`.
- Corner-pair subtract at world extent: defined, no panic in debug or release (guards saturating, not default-overflow).

### Distance / within-range → BDD "Distance and within-range"
- `distance_sq((0,0),(3,4)).get() == 25`; `within_range(...,Radius(5)) == true`, `Radius(4) == false`.
- Large boundary: `within_range((0,0),(65535,0),Radius(65535)) == true`, `Radius(65534) == false`.
- Property: `distance_sq` symmetric; self → `0` and `within_range(Radius(0)) == true`.

### Facing + cone → BDD "Facing and frontal-cone membership" (six cases, all verified arithmetically)
- `(10,0)` in 45° ✓; `(-10,0)` out (d<0) ✓; `(5,5)` edge-in 45° (`50 ≥ 50`) ✓; `(5,6)` just-out 45° (`50 ≥ 61` false) ✓; `(0,5)` in 90° / out 45° ✓; facings `(1,0)` vs `(7,0)` on `(5,5)` identical (magnitude-invariant) ✓.

### Regions → BDD "Region containment", "Region serializes kind-tagged"
- Circle at/inside/outside radius; `WorldRect` inclusive edges + just-outside; cone in-both / in-range-out-angle `(8,9)` / in-angle-out-range `(100,0)` / angular-edge-in `(5,5)`.
- Exhaustive `contains` dispatch — each variant matched explicitly, no wildcard (compile-guard).

### Movement → BDD "Grounded / Flying movement mode"
- `Grounded.checks_walkability() == true`, `Flying == false`; both variants matched explicitly (no wildcard).

### Tile ↔ world → BDD "Converting a classic tile coordinate"
- `TileCoord{2,3}.to_world() == WorldPos(163_840, 229_376)` (= `x·65536+32768`).
- Extreme `{255,255}.to_world()` in-bounds (component `16_744_448 ≤ WORLD_EXTENT`).
- **Exhaustive**: `TileCoord::from_world(t.to_world()) == t` for all `256×256` tiles; never fails, never overflows.

### Determinism / replay → BDD "same inputs → identical bytes"
- `Fixed`/`WorldPos`/`WorldVec`/`Region`/`Movement`/`Facing`/`Radius`/`ConeHalfWidth` serialize byte-identical on repeated runs.
- `within_range`/`distance_sq` identical under debug and release (guards no default-overflow reliance).
- Cone membership identical across two evaluations of a fixed sampled target grid (pure, no hidden state).
- Structural cross-target proof: all integer, explicit widening, no float, no default-overflow → single-target byte-identity + a `wasm32` CI run stands in for full native/wasm/FFI bit-identity.

### Property invariants (hand-rolled loops)
- `within_range(a,b,r) == (distance_sq(a,b) <= r.squared())` over the sampled grid (the no-sqrt equivalence).
- `distance_sq` symmetry; `(a+d)−d == a`.
- No `add`/`sub`/`scale` of in-bounds inputs overflows (sampled corner-pairs + max scale, defined under debug).
- `r.squared()` never overflows `u128` for any constructible `Radius`.
- Cone membership scale-invariant in facing magnitude over the grid.

### Serialized-shape drift pins (exact string + round-trip, one per serialized type)
`Fixed`→`196736`(illustrative) / bare int · `WorldPos`/`WorldVec`→`{"x":..,"y":..}` · `Facing`→`{"x":1,"y":0}` (+ zero-vector wire rejected) · `Radius`→bare int (+ out-of-range wire rejected) · `ConeHalfWidth`→`{"num":1,"den":2}` · `Region::Circle/Rect/Cone`→ the three §2.6 pins · `Movement`→`"grounded"`/`"flying"` · `WorldPos` out-of-bounds wire rejected · `WorldRect` inverted wire rejected (mirrors the `geometry.rs` Rect test).

`proptest` as a contract-safe dev-dep is noted but **not** adopted — see OPEN QUESTIONS.

---

## 7. Guardian sign-off

| Guardian | Plan-mode verdict | Resolution | Net |
|---|---|---|---|
| **core-architecture-guardian** | APPROVE-WITH-REQUIRED-CHANGES (4 required) | #1 `Radius(u64)` adopted (kills sign-loss cast); #2 `Pos+Vec` clamps to `[0,WORLD_EXTENT]`; #3 all casts spelled as `From`/`TryFrom`/`unsigned_abs`, saturating narrows, zero `as`; #4 `from_tile_parts` parenthesized + `saturating_add`. Suggested #5–#7 adopted (conversions live in `tile.rs`; `ConeHalfWidth.den: NonZeroU64`; `GeometryError → TileError`). #8 recorded as Wave-B watch-point. | **APPROVE** |
| **canon-guardian** | APPROVE-WITH-REQUIRED-CHANGES (3 required) | #1 raised to 16 fractional bits (`UNITS_PER_TILE = 65536`), cascade carried through (`WORLD_EXTENT = 2^24`, `CONE_DEN_MAX = 2^27`, wire values, bounds proof); #2 single rounding rule (round-nearest, ties away from zero) for both `mul` and `div`; #3 snap-facing-only + ≤90°-cone preconditions documented on `Facing`, BAM named as the fallback. | **APPROVE** |
| **state-model** | Consistent (Facing = constrained value, not FSM; Movement = minimal FSM) | `Facing`/`Movement` ship as Wave-A data; flight transition (`FlightChange` input, `MovementModeChanged` outcome, `apply_flight_change` service) pinned as the movement-wave contract (behavior → `services/`, not now). No per-change "turned" event (effect-spam); redundant flight intent is an idempotent no-op emitting nothing. | **APPROVE** |

### OPEN QUESTIONS for the user (decisions made, confirm before implementation)

1. **Scale is now `UNITS_PER_TILE = 65536` (16 fractional bits), up from the draft's 256.** This is canon-review's preferred resolution and costs no memory (`i64` either way), but the scale is **baked into every serialized `WorldPos` on the wire** — raising it later is a data-format migration, not a refactor. It also fixes `CONE_DEN_MAX = 2^27` (still astronomically fine angular resolution). Confirm 16 bits, or explicitly accept 8 with the understanding that the movement wave's normalize-to-speed op inherits ~1/256-tile quantization.
2. **Testing uses hand-rolled deterministic sampling loops** (repo's existing style, `serde_json`-only). Adding `proptest` as a dev-dependency is contract-safe (dev-deps never reach the lib target) and would give shrinking on the property invariants. Confirm hand-rolled is acceptable, or approve `proptest` as a dev-dep.

No guardian objection was left unresolved; both items above are choices I made and locked, surfaced for a nod rather than blockers.

---

## 8. Implementation waves

Each wave is a complete, shippable unit — no debt, no forbidden-phrase scaffolding.

**Wave A.1 — scalar + vector core (`spatial.rs`).** `Fixed` (incl. `from_tile_parts`, linear saturating ops; `mul`/`div`/`NonZeroFixed` contract present but not exposed), `Axis`, `SpatialError`, `WorldPos`/`WorldVec` with typed affine algebra and clamping `Pos+Vec`, `Radius`, `DistanceSq`, `distance_sq`/`within_range`. Tests: scalar round-trips, vector algebra, distance/within-range, determinism pins for these types. **Complete:** the plane's arithmetic and range core stands alone.

**Wave A.2 — facing, cone, regions (`spatial.rs`).** `Facing` (+ documented preconditions), `ConeHalfWidth` (`NonZeroU64` den), `frontal_contains`, `WorldRect` (`new`/`spanning`/`contains`), `Region` + exhaustive `contains`. Tests: all six cone cases, region containment + exhaustive dispatch, region/facing/cone wire pins. **Complete:** all spatial predicates present and proven.

**Wave A.3 — movement mode (`movement.rs`).** `Movement` + `checks_walkability`. Tests: predicate per variant, exhaustive match, wire pin. **Complete:** traversal mode carried.

**Wave A.4 — tile authoring + conversions + walk-grid shape (`tile.rs`) and the geometry split.** Move/rename `TileCoord`/`TileArea`/`TileFacing`/`Footprint`/`TileError`; `to_world`/`from_world`; `WalkGrid` shape with total `walkable`; delete `geometry.rs`; rewire `mod.rs` and the four `data/*.rs` consumers (rename only — JSON untouched); update `core/tests/data_files.rs` if it names the moved types. Tests: tile↔world round-trip over all `256×256`, extreme-tile bounds, `walkable` totality, conversion determinism. **Complete:** authoring boundary quarantined, no live `Point`/`Direction` remains.

**Wave B (separate, named — not this spec).** `Atlas::parse` world-space migration (`Landing`/spawn positions → `WorldPos`/`WorldRect`, `Landing.facing`/`SpawnGate.direction` → continuous `Facing` = canon D9), `data/terrain/*.bin` → `WalkGrid` loading, and the flight state machine in `services/` (`apply_flight_change` over `(Movement, FlightChange) → (Movement, Vec<MovementModeChanged>)`, total 2×2 exhaustive tuple match, redundant intent = idempotent no-op; the eligibility gate — wings/map/combat-lock — runs before it and emits its own denial). Wave B consumes Wave A and touches the movement/spawn services.

Pipeline for build-out: `state-machine-agent` (Wave B flight FSM only) → implementation → `core-architecture-guardian` (code) → `deep-module-guardian` → `canon-guardian` (code) → `debt-guardian` → `rules-guardian`.