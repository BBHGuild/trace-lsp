# 🔍 trace-lsp

High-performance, standalone **Language Server Protocol (LSP)** for **EVM & Foundry execution traces** (`.trace`, `.traces`, `.sol.trace`). 

Turn raw, thousands-of-lines ASCII Solidity/Foundry execution traces (generated via `forge test -vvvv`, `forge script -vvvv`, or `cast run`) into a beautifully highlighted, structured, and navigable call tree in **Neovim**, **VS Code**, **Helix**, **Zed**, or any LSP-compliant editor.

> [!NOTE]
> **Evolution from `zed-trace`**:
> This repository was previously `zed-trace`. The Zed-specific WebAssembly extension wrapper has been deprecated and archived in [`deprecated/zed-extension/`](deprecated/zed-extension/). `trace-lsp` is now an editor-agnostic, native Language Server that communicates directly over standard JSON-RPC stdio.

---

## ⚡ Features

### 1. 🎨 Full Semantic Syntax Highlighting (`textDocument/semanticTokens/full`)
Trace files are not raw Solidity code—they contain dynamic ASCII tree characters (`├─ `, `│  `, `└─ `), gas metrics, storage changes, and execution results that break ordinary parsers.

`trace-lsp` provides context-aware **LSP Semantic Tokens** that cleanly tokenize every element of the trace file:

| Element | Semantic Scope | Trace Examples |
| :--- | :--- | :--- |
| **Keywords & Modifiers** | `keyword` / `modifier` | `emit`, `new`, `storage`, `changes`, `[staticcall]`, `[delegatecall]`, `[Return]`, `[Stop]`, `[Revert]` |
| **Contracts & Types** | `type` | `CCCTest`, `Proof`, `VM`, `WBNB`, `IPancakeRouter`, `uint256`, `address`, `bytes` |
| **Functions & Selectors** | `function` | `setUp`, `test_profit_success`, `run`, `balanceOf`, `flashLoan`, `transfer`, `getAmountsOut`, `sync` |
| **Gas & Wei Values** | `number` | `[14739827]`, `416831487011304318246517`, `[4.168e23]`, `1e18`, `0` |
| **Hex Addresses & Slots** | `property` | `0xbb4CdB9CBd36B01bD1cBaEBF2De08d9173bc095c`, `0x7476b743a16e0315bdd...` |
| **Parameters & Labels** | `variable` | `from:`, `to:`, `amount:`, `param0:`, `owner:`, `spender:`, `SENDER`, `OTHER_CALLER` |
| **Booleans** | `keyword` / `boolean` | `true`, `false` |
| **String Literals** | `string` | `"bsc"`, `"SENDER "` |
| **Arrows & Delimiters** | `operator` | `::`, `→`, `←`, `@`, `:`, `,` |
| **Tree Branch Guides** | `comment` | `├─`, `└─`, `│` |

---

### 2. 🌳 Hierarchical Document Symbols (`textDocument/documentSymbol`)
- Parses ASCII tree branches into structured parent-child call trees.
- Populates symbol outlines (Trouble.nvim, Telescope, Zed Buffer Symbols, VS Code Outline).
- Instant keyboard navigation: fuzzy-search and jump directly to any sub-call (e.g. `flashLoan`, `swap`, `buy`, `sync`, or `withdraw`) in massive trace files.
- Visual categories:
  - 🔷 **Root Test Call**: `CCCTest::setUp()`, `CCCTest::test_profit_success()`
  - ⚙️ **Contract Call**: `Proof::run()`, `0xbb4C...::balanceOf() [staticcall]`
  - 🏗️ **Contract Creation**: `new Proof@0x3255...`
  - 📢 **Event Emission**: `emit FlashLoan(...)`, `emit Transfer(...)`
  - 📦 **State / Storage**: `storage changes`

```text
▼ CCCTest::test_profit_success() [14.74M gas]
  ├─ 0xbb4C...::balanceOf [2.5k gas] [staticcall]
  ├─ VM::prank [0 gas]
  ▼ Proof::run [14.72M gas]
    ├─ 0xbb4C...::balanceOf [2.5k gas] [staticcall]
    ▼ 0x8F73...::flashLoan [14.66M gas]
      ▼ 0x9321...::flashLoan [14.65M gas] [delegatecall]
        ├─ emit FlashLoan(param0: Proof: [0x3255...], ...)
        ▼ 0xbb4C...::transfer [28.0k gas]
          ├─ emit Transfer(...)
          ▼ storage changes
            ├─ @ 0x7476...: 0x... → 0
            └─ @ 0x086b...: 0 → 0x...
        ▼ Proof::onMoolahFlashLoan [14.61M gas]
          ▼ 0x10ED...::getAmountsOut [9.7k gas] [staticcall]
```

---

### 3. 📂 Deep Code Folding (`textDocument/foldingRange`)
- Automatically generates folding ranges for every nested execution branch.
- Collapse or expand any sub-call tree with editor folding shortcuts (`za`, `zc`, `zo`, `zM`, `zR`).
- Folds storage change blocks and sub-traces cleanly.

---

### 4. 💡 Rich Hover Analysis (`textDocument/hover`)
- **Gas Metrics**: Formats raw gas into human-readable metrics (e.g., `14,716,127 gas (~14.72M)`).
- **Address Labels**: Resolves contract names (e.g., `WBNB`, `PancakeRouter`, `ListaDAO`) and labels registered via `VM::label`.
- **Value & Parameters**: Decodes wei amounts into formatted token units.
- **Transient Storage**: Highlights and displays transient storage diffs.

---

### 5. 🎯 Go to Definition (`textDocument/definition`)
- Jump directly from a call or sub-trace to the contract source or call origin definition.

---

### 6. 🌐 Interactive Web Viewer (`trace-lsp web`)
Built-in command to export or view any `.trace` file in a browser with an interactive **Phalcon Explorer** flow interface (collapsible nodes, search filter, and toggle pills for Static Call, Storage Changes, and Events).

```bash
trace-lsp web sample.trace
# Generates sample_explorer.html
```

---

## 🚀 Installation

### From Source (Cargo)
```bash
git clone https://github.com/BBHGuild/trace-lsp.git
cd trace-lsp
cargo install --path .
```
This builds and installs `trace-lsp` into `~/.cargo/bin/trace-lsp`. Ensure `~/.cargo/bin` is in your `$PATH`.

---

## 🛠️ Editor Integration

### Neovim (`nvim-lspconfig`)
Add the following to your Neovim LSP configuration:

```lua
local lspconfig = require("lspconfig")
local configs = require("lspconfig.configs")

if not configs.trace_lsp then
  configs.trace_lsp = {
    default_config = {
      cmd = { "trace-lsp", "--stdio" },
      filetypes = { "evm_trace" },
      root_dir = function(fname)
        return vim.fs.dirname(vim.fs.find({ ".git" }, { path = fname, upward = true })[1]) or vim.fn.getcwd()
      end,
      settings = {},
    },
  }
end

lspconfig.trace_lsp.setup({})
```

Register trace file extensions in `autocmds.lua`:
```lua
vim.filetype.add({
  extension = {
    trace = "evm_trace",
    traces = "evm_trace",
  },
  pattern = {
    [".*%.sol%.trace"] = "evm_trace",
  },
})
```

### Zed Editor
Add to your Zed `settings.json`:
```json
{
  "lsp": {
    "trace-lsp": {
      "binary": {
        "path": "trace-lsp",
        "arguments": ["--stdio"]
      }
    }
  }
}
```

---

## 🧪 Development & Testing

```bash
# Run test suite
cargo test

# Run benchmarks
cargo test --test benchmark -- --nocapture

# Build release binary
cargo build --release
```

---

## 📜 License

MIT License
