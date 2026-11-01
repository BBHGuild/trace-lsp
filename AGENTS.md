# 🤖 AGENTS.md — Architecture & Engineering Guide for `trace-lsp`

This document captures the critical design decisions, performance constraints, and protocol invariants of the `trace-lsp` codebase for AI agents and human contributors.

---

## 📌 Mission & Architecture

`trace-lsp` provides instant, native EVM and Foundry execution trace exploration across LSP-compatible editors (Neovim, VS Code, Helix, Zed). The architecture consists of:

1. **`tree-sitter-evm-trace`**: High-performance Tree-sitter grammar providing instant syntax highlighting in editor buffers and symbol preview modals.
2. **`trace-lsp`**: Standalone Rust language server communicating over stdio JSON-RPC. Parses ASCII execution tree guides (`├─`, `│`, `└─`, `→`, `←`), gas brackets, call expressions, events, and storage diffs. Provides `textDocument/documentSymbol`, `textDocument/semanticTokens/full`, `textDocument/foldingRange`, `textDocument/hover`, and `textDocument/definition`.
3. **Legacy Zed WASM Extension**: Deprecated and archived in `deprecated/zed-extension/`. Editors now run `trace-lsp` directly via stdio.

---

## 🚨 The 128 JSON Recursion Limit in Zed (`serde_json`)

### 1. Root Cause
Zed deserializes language server JSON-RPC responses in [`crates/lsp/src/lsp.rs:1561`](file:///home/sean/Github/zed/crates/lsp/src/lsp.rs#L1561) and line 251:
```rust
fn deserialize_result<T: DeserializeOwned + 'static>(result: &str) -> serde_json::Result<T> {
    if TypeId::of::<T>() == TypeId::of::<()>() {
        serde_json::from_str("null")
    } else {
        serde_json::from_str(result)
    }
}
```
`serde_json::from_str` uses a hardcoded, compiled-in recursion limit of **128**.

### 2. The Mathematics of Tree Nesting
In the Language Server Protocol, a nested `DocumentSymbol` tree serializes as:
```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "result": [
    {
      "name": "root",
      "children": [
        {
          "name": "child_1",
          "children": [ ... ]
        }
      ]
    }
  ]
}
```
Each level of parent-child hierarchy in `children` introduces **2 JSON nesting levels**:
- `"children": [` (+1 array nesting)
- `{ ... }` (+1 object nesting)

Inside the deepest leaf symbol, `"range": { "start": { "line": 0, "character": 0 } }` adds another 2 nesting levels.
For a symbol tree of depth $D$:
$$\text{JSON Nesting Depth} = 2 \times D + 4 \quad (\text{or up to } 2D + 6)$$

| Tree Depth | JSON Nesting Depth | Headroom to 128 | Result in Zed |
| :---: | :---: | :---: | :--- |
| 30 | 64 | 64 levels | Safe |
| 45 | 94 | 34 levels | Safe |
| **50** | **104** | **24 levels** | **Safe (our limit)** |
| 60 | 124 | 4 levels | Dangerously close |
| **61** | **126** | **2 levels** | **Absolute mathematical limit** |
| **$\ge 62$** | **$\ge 128$** | **0** | **CRASH (`recursion limit exceeded`)** |

### 3. Real-World Trace Impact
Real Foundry execution traces containing recursive flash loans, callbacks, and multi-hop DEX swaps (e.g. `Fox/tx-trace-V1.trace` and `Fox/tx-trace-V5.trace`) reach nesting depths of **73**.
When serialized naively, the response reaches **164 levels of JSON nesting**.
Zed crashes with:
```text
ERROR [lsp] failed to deserialize response from language server: recursion limit exceeded at line 1 column 231415
```
Because the deserializer fails, Zed **drops the entire response and renders 0 symbols**.

---

## 🛠️ The Design Solution: Depth Clamping with Invariant-Preserving Hoisting

In [`crates/trace-lsp/src/analysis.rs`](file:///home/sean/Github/zed-trace/crates/trace-lsp/src/analysis.rs), structural nesting is capped at:
```rust
pub const MAX_SYMBOL_TREE_DEPTH: usize = 50;
```

### Algorithm
1. **Natural Hierarchy ($\le 50$)**:
   Calls and events at depths $0 \dots 49$ recurse normally with full parent-child hierarchy. Traces up to depth 50 are 100% unaffected.
2. **Hoisting ($> 50$)**:
   When a node reaches `depth >= MAX_SYMBOL_TREE_DEPTH`, it collects **all** descendants across its entire subtree into its `children` list as flat leaf symbols (`children: None`).
3. **Zed Well-Formed Outline Compliance**:
   Zed verifies outlines using `BufferOutlines::outlines_are_well_formed` in [`crates/outline_panel/src/outline_panel.rs:451`](file:///home/sean/Github/zed/crates/outline_panel/src/outline_panel.rs#L451):
   - **Sibling Non-Overlap**: When popping sibling ancestors, `sibling_1.range.end <= sibling_2.range.start` must hold.
     *Solution*: Every hoisted leaf symbol has its `range.end` set to its **own line** (`line: node.line_start, character: doc.line_len(node.line_start)`). Because line numbers in the trace are strictly monotonic, consecutive siblings never overlap ($L_1 < L_2$).
   - **Parent Enclosure**: For every child, `child.range.end <= parent.range.end`.
     *Solution*: The depth-50 ancestor's `range.end` expands to encompass the maximum `range.end` of all its descendants.

### Verification Matrix
- **`Fox/tx-trace-V1.trace`**: 1,149 / 1,149 symbols retained (**100%**), max JSON depth 67, 0 well-formed violations.
- **`Fox/tx-trace-V5.trace`**: 1,297 / 1,297 symbols retained (**100%**), max JSON depth 67, 0 well-formed violations.
- **`CCC/tx-trace-V5.trace`**: 4,782 / 4,782 symbols retained (**100%**), max depth 9, 0 well-formed violations.

---

## ⚡ Performance: Slot Diff Leaf Aggregation

- Trace files contain hundreds to thousands of individual storage slot diffs:
  `@ 0x7476...: 0x00... → 0x0de0...`
- Zed parses, creates UI entities, and highlights every symbol on its main UI thread. Emitting every slot diff as an individual symbol added ~30% payload bloat and caused perceptible UI lag on 9,000+ line traces.
- **Design Choice**: Individual `@ slot` lines are **not** emitted as individual `DocumentSymbol` items. Instead, they are aggregated into their parent `storage changes` symbol (e.g., `storage changes — 2 slots`).
- Slot diffs remain completely syntax-highlighted, foldable in the editor, and hoverable.

---

## 🧪 Testing & Benchmarks

Always run the full test suite before committing:
```bash
cargo test --all
```
To benchmark with stdio JSON-RPC and verify Zed outline invariants:
```bash
python3 scratch/lsp_bench.py /path/to/trace.trace
```
The test verifies:
- `out-of-bounds positions: 0`
- `name != buffer text: 0`
- `well_formed: True`
- Timing: `documentSymbol < 20ms`, `semanticTokens < 35ms`.
