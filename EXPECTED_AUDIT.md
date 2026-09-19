# Varn — Architecture & Value Representation Audit

## Objective

Perform a deep architectural audit of the Varn compiler, VM, runtime, type system, TIR/SSA pipeline, bytecode backend, and native/JIT backend.

The objective is **not** to redesign features that already work.

The objective is to determine:

1. What the current architecture already does correctly.
2. Where the current implementation performs redundant processing.
3. Whether the current value representation is unnecessarily universal.
4. Whether `i128` as the VM register representation is an architectural bottleneck.
5. Whether type information is preserved continuously from semantic analysis to execution.
6. Whether the current architecture can naturally evolve from the initial primitive set:

   * `int` → `i64`
   * `float` → `f64`

   toward explicitly sized types such as:

   * `i8`, `i16`, `i32`, `i64`, `i128`
   * `u8`, `u16`, `u32`, `u64`, `u128`
   * `f32`, `f64`
   * and potentially future vector/SIMD types.
7. What should be deleted and replaced rather than preserved for compatibility.
8. What architectural changes would provide the cleanest foundation for high-level Varn code to eventually approach systems-language performance.

This is an early-stage language.

**Backward compatibility is NOT a constraint.**

If an existing abstraction is architecturally inferior, it should be considered removable even if that requires breaking every dependent component.

Do not preserve an abstraction merely because it already exists.

---

# 1. Core Architectural Principle

Evaluate the project against this principle:

> Static type information should become progressively more concrete as compilation proceeds, never less concrete.

The desired conceptual flow is:

```text
Source
  ↓
AST
  ↓
Typed AST / HIR
  ↓
TIR
  ↓
SSA
  ↓
Physical representation
  ↓
Bytecode / Native instructions
```

A statically known value should not unnecessarily become a universal value and later be specialized again.

For example:

```text
i64
 ↓
universal Value
 ↓
inspect tag
 ↓
infer i64
 ↓
specialize
 ↓
ADD_I64
```

should be treated as suspicious.

Prefer:

```text
i64
 ↓
typed IR value
 ↓
i64 representation
 ↓
ADD_I64
```

The audit must determine whether the current implementation actually behaves like one of these models.

---

# 2. Important Existing Constraint

The current VM uses a universal 128-bit value representation.

Conceptually:

```text
VmValue
├── tag: u64
└── payload: u64
```

and VM registers currently use this representation universally.

Therefore, investigate this explicitly.

Do NOT assume that the existence of specialized bytecode instructions means the representation problem is solved.

The key question is:

> Specialized instructions already exist, but do they operate on values that were unnecessarily converted into the universal 128-bit representation before reaching them?

This distinction is central to the audit.

---

# 3. Do Not Assume Missing Features

The project already contains specialized bytecode and machine-code generation.

The audit must verify what actually exists before recommending implementation work.

Specifically investigate:

* specialized bytecode instructions;
* typed instruction selection;
* TIR typing;
* SSA representation;
* native instruction selection;
* register allocation;
* JIT lowering;
* runtime representations;
* specialized arithmetic;
* specialized comparisons;
* specialized calls;
* specialized field access;
* specialized array access;
* dynamic operations.

If something already exists, document it and evaluate its architecture rather than proposing to implement it again.

---

# 4. Repository Reconnaissance

First map the complete repository.

Identify:

* workspace structure;
* crates;
* compiler stages;
* type system;
* AST;
* parser;
* checker;
* TIR;
* SSA;
* optimizer;
* register allocator;
* bytecode;
* VM;
* runtime;
* GC;
* object model;
* shapes;
* arrays;
* strings;
* closures;
* function representation;
* modules;
* JIT;
* Cranelift integration;
* CLI;
* tests;
* benchmarks.

Produce an architecture map based on the actual code.

Do not rely exclusively on README documentation.

If documentation and implementation disagree, report the discrepancy.

---

# 5. Compilation Pipeline Audit

Trace a representative expression through the complete compiler.

At minimum trace:

```text
let a: int = 10;
let b: int = 20;
let c = a + b;
```

and:

```text
let a: float = 10.0;
let b: float = 20.0;
let c = a + b;
```

Also trace:

```text
let x: dynamic = 10;
```

For each example document:

1. AST representation.
2. Type-checker representation.
3. Typed AST/HIR representation, if present.
4. TIR representation.
5. SSA representation.
6. Bytecode representation.
7. VM register representation.
8. Native/JIT representation.
9. Runtime representation where applicable.

For every transition answer:

* Does type information survive?
* Is type information reconstructed?
* Is a value boxed?
* Is a value unboxed?
* Is a tag inspected?
* Is a universal representation introduced?
* Is the value copied?
* Is there redundant lowering?
* Is there redundant specialization?

---

# 6. Value Representation Audit

Perform a dedicated audit of every value representation in the project.

Find all representations conceptually equivalent to:

```text
Value
VmValue
RuntimeValue
DynamicValue
TypedValue
Constant
Operand
Register
Slot
Object reference
Function reference
Closure value
```

For each one determine:

* size;
* alignment;
* ownership;
* lifetime;
* whether it is compile-time or runtime;
* whether it is dynamically typed;
* whether it can represent references;
* whether it participates in GC;
* whether it is used in static code;
* whether it is used only for `dynamic`;
* whether conversions exist between representations.

Build a dependency graph.

The important question is:

> How many times does a normal statically typed value change representation between type checking and machine execution?

---

# 7. Universal `i128` Register Audit

Inspect the VM register file directly.

Determine:

* whether every register is physically `i128`;
* whether specialized instructions still read/write 128-bit values;
* whether loads/stores are 128-bit;
* whether arithmetic operands are stored in 128-bit slots;
* whether arguments are passed as `i128`;
* whether return values are passed as `i128`;
* whether temporaries are `i128`;
* whether spills are `i128`;
* whether stack frames are composed of `i128` slots;
* whether GC root scanning sees every register as potentially containing a reference;
* whether debug information assumes universal values.

Quantify the consequences.

Do not merely state that 128-bit is "larger".

Identify actual operations where it causes:

* additional memory traffic;
* increased frame size;
* additional copies;
* larger stack/register spills;
* larger bytecode state;
* worse cache locality;
* unnecessary GC scanning;
* unnecessary conversions;
* unnecessary loads/stores;
* ABI complexity.

---

# 8. Typed Register Architecture Investigation

Evaluate whether the VM should move from:

```text
registers: [VmValue; N]
```

toward a typed register-class model.

Investigate a model conceptually similar to:

```text
GPR
FPR
REF
DYN
```

where:

```text
GPR → integer/scalar machine values
FPR → floating-point values
REF → managed references
DYN → dynamically typed values
```

Do not assume these exact names or implementation details.

Determine the architecture that best fits the existing compiler.

Evaluate whether the VM could naturally represent:

```text
i64 → GPR
f64 → FPR
ref<T> → REF
dynamic → DYN
```

without boxing.

---

# 9. Future Primitive Type Evolution

The initial language types are intentionally simple:

```text
int   → i64
float → f64
```

Future versions should be able to introduce:

```text
i8
i16
i32
i64
i128

u8
u16
u32
u64
u128

f32
f64
```

and potentially:

```text
f16
bf16
SIMD/vector types
```

The architecture must therefore be evaluated for extensibility.

Determine whether adding `i32` or `f32` would require:

* modifying `VmValue`;
* modifying the universal register representation;
* adding additional tags;
* adding special cases throughout the compiler;
* changing object layout;
* changing the bytecode format;
* changing the register allocator;
* changing the VM register file;
* or merely adding a new primitive type and lowering rule.

The desired property is:

> Adding a new primitive type should extend the type/representation/lowering system rather than force another redesign of the universal value system.

---

# 10. Semantic Type vs Physical Representation

Explicitly separate these concepts in the audit.

For example:

```text
Varn type:
i8

Physical execution representation:
64-bit GPR

Memory representation:
8-bit

ABI representation:
target-dependent

Vectorized representation:
potentially SIMD
```

Do not assume:

```text
language width == register width
```

The language type determines semantics and memory/layout requirements.

The backend should determine the most appropriate physical representation for the target architecture.

Investigate whether Varn already follows this separation.

If it does not, identify where the concepts are coupled.

---

# 11. Arrays and Aggregate Layout

Audit:

* arrays;
* tuples;
* structs;
* objects;
* strings;
* slices/views;
* buffers.

Determine whether:

```text
i8[]
```

can physically be:

```text
[i8][i8][i8][i8]...
```

rather than:

```text
[VmValue][VmValue][VmValue]...
```

Likewise:

```text
f32[]
```

should be capable of compact contiguous storage.

Determine whether the current object/array system preserves element representation or erases everything into a universal value.

This is critical for future data-oriented performance.

---

# 12. Struct and Object Layout

Audit how the following are represented:

```text
struct
object
fields
methods
Shape
```

Determine:

* field offsets;
* field sizes;
* alignment;
* padding;
* reference fields;
* primitive fields;
* dynamic fields;
* inline vs boxed representation;
* shape metadata;
* field lookup;
* static field access;
* dynamic field access.

Determine whether:

```text
struct {
    i8
    i8
    i16
    i32
}
```

can have an efficient compact memory layout.

Determine whether static field access bypasses dynamic Shape lookup.

---

# 13. Dynamic Value Strategy

Evaluate the existing `dynamic` representation separately from statically typed values.

The audit must answer:

> What is the minimum representation required for a truly dynamic Varn value?

Compare at least:

### Option A

```text
tag: u64
payload: u64
```

### Option B

64-bit NaN-boxed representation.

### Option C

Another representation justified by the existing runtime architecture.

For each option evaluate:

* size;
* pointer representation;
* GC integration;
* portability;
* debugging;
* arithmetic;
* floating-point preservation;
* integer range;
* reference representation;
* branch cost;
* encoding complexity;
* JIT interaction;
* AOT interaction.

Do not recommend NaN-boxing merely because it is smaller.

Determine whether it actually benefits Varn.

Most importantly:

> A dynamic representation must not become the representation of statically typed values merely because it exists.

---

# 14. GC Interaction

Audit how value representation interacts with the garbage collector.

Determine:

* how roots are identified;
* how VM registers are scanned;
* how stack slots are scanned;
* how native frames are scanned;
* whether roots are precise or conservative;
* whether register types are known;
* whether `dynamic` values require tag inspection;
* whether primitive registers can be excluded from GC scanning;
* whether reference registers can be identified directly.

Evaluate whether typed register classes could simplify:

```text
GPR → never GC root
FPR → never GC root
REF → GC root
DYN → inspect
```

without sacrificing correctness.

---

# 15. Calling Convention Audit

Trace function calls.

Inspect:

* argument representation;
* return value representation;
* local variables;
* temporaries;
* closures;
* captured variables;
* function pointers/references;
* native calls;
* runtime calls;
* dynamic calls.

Determine whether every call currently passes universal 128-bit values.

Evaluate a typed convention such as:

```text
i64 → integer argument register
f64 → floating argument register
ref → reference register
dynamic → dynamic register
```

for the VM and native backend.

Determine whether this can be shared conceptually across VM and native lowering.

---

# 16. Register Allocation Audit

Inspect the existing register allocator.

Determine whether it understands:

* value types;
* register classes;
* lifetimes;
* references;
* spills;
* calling conventions;
* machine register classes.

Determine whether the current allocator treats everything as one homogeneous value.

If so, determine whether this is an architectural simplification that is now becoming a bottleneck.

Evaluate how typed register classes would interact with:

* SSA;
* liveness;
* spilling;
* GC maps;
* Cranelift;
* bytecode registers.

---

# 17. Bytecode Audit

Because specialized bytecode already exists, audit its actual relationship with the value representation.

For every representative opcode:

```text
ADD_I64
ADD_F64
MUL_I64
CMP_I64
CMP_F64
LOAD
STORE
CALL
RETURN
FIELD_GET
FIELD_SET
ARRAY_GET
ARRAY_SET
```

determine:

* operand types;
* operand width;
* register representation;
* boxing;
* unboxing;
* runtime calls;
* tag checks;
* conversion operations.

Find cases where an opcode is specialized but the underlying operands are still universal `i128`.

These are high-priority candidates for architectural simplification.

---

# 18. Native/JIT Backend Audit

Determine whether native code generation receives:

```text
typed SSA
```

or something already lowered into:

```text
VmValue
```

If native lowering goes through the universal VM representation, investigate whether this is unnecessarily coupling the native backend to the VM.

Desired conceptual architecture:

```text
Typed SSA
   ├── VM lowering
   └── Native lowering
```

rather than:

```text
Typed SSA
   ↓
VmValue
   ↓
VM IR
   ↓
Native
```

If the current implementation already has the better architecture, document that explicitly.

---

# 19. VM vs Native Backend Boundary

Determine whether VM-specific concepts leak into:

* TIR;
* SSA;
* optimizer;
* type checker;
* native backend.

A VM backend should not dictate the semantic representation of the language.

The audit should identify any abstractions whose only purpose is satisfying the current VM implementation.

These are candidates for removal.

---

# 20. Optimization Audit

Do not simply list optimizations.

Determine whether the representation enables or inhibits:

* constant folding;
* constant propagation;
* dead-code elimination;
* common subexpression elimination;
* strength reduction;
* range analysis;
* bounds-check elimination;
* devirtualization;
* inlining;
* escape analysis;
* scalar replacement;
* allocation sinking;
* loop-invariant code motion;
* vectorization;
* specialization;
* monomorphization if applicable.

For each, identify whether the universal `i128` representation creates additional work.

---

# 21. Redundant Processing Analysis

This is one of the most important sections.

Find every pipeline stage where the compiler:

1. determines a type;
2. erases it;
3. reconstructs it;
4. specializes again.

Also find:

1. value boxing;
2. value unboxing;
3. representation conversion;
4. tag extraction;
5. tag insertion;
6. runtime type checks;
7. universal temporaries;
8. universal argument passing;
9. universal return values.

For each occurrence report:

```text
Location
Current behavior
Why it exists
Information already available at that point
Whether it is necessary
Potential elimination
Expected architectural consequence
```

Do not optimize individual instances before determining whether the abstraction causing them should be removed.

---

# 22. Architecture Smell Detection

Explicitly search for these smells:

```text
Type → Value → Type
```

```text
Typed IR → Universal Value → Typed opcode
```

```text
Static value → Box → Unbox
```

```text
Known reference → Universal value → GC inspection
```

```text
Known i64 → i128 register
```

```text
Known f64 → i128 register
```

```text
Typed call → universal arguments
```

```text
Typed return → universal return value
```

```text
Machine representation encoded inside semantic type system
```

```text
VM constraints leaking into TIR
```

These are not automatically bugs.

They are audit targets.

---

# 23. Performance Model

Do not benchmark only source-level execution.

Measure the representation itself.

Where possible compare:

```text
Current:
i128 universal register

Potential:
typed 64-bit register classes
```

Measure:

* register/frame size;
* bytecode frame footprint;
* number of loads;
* number of stores;
* number of copies;
* memory bandwidth;
* cache behavior;
* interpreter dispatch cost;
* argument passing;
* return passing;
* GC root scanning;
* dynamic operation cost.

Use microbenchmarks where practical.

At minimum test:

```text
integer arithmetic
floating arithmetic
integer loops
floating loops
function calls
nested calls
local variables
arrays
structs
dynamic values
GC-heavy workloads
```

Do not claim performance improvements without measurement or a concrete architectural reason.

---

# 24. Evolution Test

After analyzing the current architecture, perform a hypothetical design exercise.

Ask:

> If tomorrow Varn adds `i32` and `f32`, what files and systems need modification?

Then ask:

> If Varn later adds `i8`, `i16`, `u8`, `u16`, `u32`, `f16`, SIMD vectors, what systems need modification?

The ideal architecture should localize most changes to:

```text
type definitions
type checking rules
lowering rules
backend-specific representation rules
```

and should NOT require redesigning:

```text
VM value representation
entire register file
GC
object model
bytecode architecture
```

unless technically justified.

---

# 25. Breaking-Change Policy

The project is explicitly allowed to break itself.

Do not recommend:

* compatibility layers;
* transitional dual representations;
* legacy code paths;
* feature flags preserving obsolete architecture;
* migration wrappers;
* adapters whose only purpose is backwards compatibility.

If the correct architecture is:

```text
delete X
replace with Y
```

say exactly that.

Prefer one clean architecture over:

```text
old system + new system + compatibility bridge
```

Git provides historical recovery.

---

# 26. No Premature Implementation

Do not modify the repository during this audit.

Do not create a PR.

Do not rewrite code.

Do not "fix" individual problems discovered during analysis.

First produce the architectural audit.

Implementation should only happen after the architectural target is agreed upon.

---

# 27. Required Final Report

Produce a rigorous report with exactly these major sections:

## A. Executive Summary

Maximum 15 concise bullets.

State:

* what is already architecturally strong;
* what is unnecessarily complicated;
* whether universal `i128` registers are a real problem;
* whether `VmValue` itself is the problem or merely its universal use;
* whether the current architecture can evolve cleanly;
* the highest-value architectural changes.

Do not exaggerate.

---

## B. Current Architecture

Provide the actual architecture discovered in the repository.

Include a diagram.

Example:

```text
Source
  ↓
Parser
  ↓
Checker
  ↓
TIR
  ↓
SSA
  ├── Bytecode
  │     ↓
  │     VM
  │
  └── Native
        ↓
      Cranelift
```

Adapt this to the actual repository.

---

## C. Value Flow

Show the actual journey of:

```text
i64
f64
dynamic
reference
struct
array element
```

through the compiler.

Identify every representation change.

---

## D. Universal Value Analysis

Explain exactly where `i128` is used.

Separate:

```text
necessary uses
```

from:

```text
accidental uses
```

and:

```text
architecturally questionable uses
```

---

## E. Typed Representation Proposal

If justified by the code, propose the clean target architecture.

It should address:

```text
semantic type
physical representation
VM register class
memory layout
GC classification
ABI
bytecode
native/JIT
```

Do not propose implementation details that are not supported by the existing architecture unless clearly marked as a new design decision.

---

## F. Dynamic Representation

Compare:

```text
128-bit tagged value
64-bit NaN-box
other viable representation
```

and explain which is appropriate for Varn and why.

The recommendation must be based on Varn's actual use of `dynamic`, not generic VM design advice.

---

## G. Future Primitive Types

Explain how the proposed architecture accommodates:

```text
i8
i16
i32
i64
i128
u8
u16
u32
u64
u128
f32
f64
```

and future vector types.

---

## H. Redundant Processing

List every unnecessary conversion/specialization/type reconstruction discovered.

Rank them by architectural significance:

```text
CRITICAL
HIGH
MEDIUM
LOW
```

Do not use numeric scores.

---

## I. What Should Be Deleted

This section is mandatory.

List abstractions, layers, representations, or code paths that should disappear entirely if the proposed architecture is adopted.

For each:

```text
Delete:
Reason:
Replacement:
Affected components:
```

Do not preserve something merely because it already works.

---

## J. What Should Remain

Explicitly identify components that are already well-designed and should NOT be rewritten merely for the sake of change.

This is important.

The purpose of this audit is not to maximize code churn.

---

## K. Migration / Rewrite Order

Provide the cleanest rewrite order.

Prioritize foundations.

Example:

```text
1. Value/type representation
2. TIR value model
3. SSA value model
4. register classes
5. VM frame/register architecture
6. bytecode lowering
7. GC root representation
8. calling convention
9. native/JIT boundary
10. benchmarks
```

Adapt this to the actual project.

Do not preserve obsolete architecture during migration unless technically unavoidable.

---

## L. Verification Plan

Define tests proving that the new architecture works.

Include:

* type-system tests;
* TIR tests;
* SSA tests;
* bytecode tests;
* VM tests;
* GC tests;
* ABI tests;
* native/JIT tests;
* representation tests;
* performance benchmarks.

---

# 28. Required Evidence Standard

Every architectural conclusion must reference actual repository evidence.

For each important conclusion include:

```text
File
Symbol/type/function
Relevant behavior
Conclusion
```

Do not make claims such as:

> "The VM probably boxes everything."

Find the actual code.

Do not make claims such as:

> "The JIT probably uses typed values."

Verify it.

If the code is ambiguous, explicitly state:

```text
Unverified
```

and explain what prevented verification.

---

# 29. Priority of Truth

When sources disagree, use this priority:

```text
1. Actual implementation
2. Tests
3. Compiler/runtime behavior
4. Documentation
5. README claims
6. Architectural assumptions
```

Never treat documentation as proof that an implementation exists.

---

# 30. Final Architectural Question

End the report by answering this exact question:

> **If Varn were redesigned today with no backward-compatibility constraints, but retaining every feature that is genuinely architecturally sound in the current implementation, what value representation and execution architecture should be the foundation for the next stage of the language?**

The answer must distinguish clearly between:

```text
what Varn already does correctly
```

and:

```text
what should be replaced
```

Do not recommend changes merely because another language uses them.

The architecture must be justified by Varn's goals:

* high-level syntax;
* strict static typing;
* zero implicit coercion;
* specialized execution;
* efficient memory representation;
* low runtime overhead;
* native/JIT execution;
* future compact primitive types;
* efficient dynamic values;
* clean GC integration;
* multi-ISA support;
* long-term architectural evolution.

The goal is not to make the current implementation incrementally better.

The goal is to determine whether the **foundation itself is correct**.
