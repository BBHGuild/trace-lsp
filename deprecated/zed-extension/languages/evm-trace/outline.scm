; ==============================================================================
; Fallback Tree-sitter Outline Query for EVM Execution Traces (evm_trace)
; ==============================================================================

(call_trace
  call: (call_expression
    function: (identifier) @name) @context) @item

(call_trace
  call: (call_expression
    function: (hex_selector) @name) @context) @item

(creation_trace
  contract: (_) @name) @item

(emit_trace
  name: (identifier) @name) @item

(storage_header) @item

