# ⚠️ Deprecated: Zed Extension for EVM Trace

> [!WARNING]
> The Zed WASM extension (`extension.wasm`, `src/trace.rs`, `extension.toml`) is deprecated.
> 
> **`trace-lsp` is now a standalone, editor-agnostic Language Server Protocol implementation.**
> 
> To use `trace-lsp` with Zed or any other editor (Neovim, VS Code, Helix):
> 1. Install `trace-lsp`:
>    ```bash
>    cargo install --path .
>    ```
> 2. Ensure `~/.cargo/bin/trace-lsp` is in your `PATH`.
> 3. Configure `trace-lsp` as the language server for `trace` filetypes.
