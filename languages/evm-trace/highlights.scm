; ==============================================================================
; Tree-sitter Highlights Query for EVM Execution Traces (evm_trace)
; Pixel-perfect alignment with XY-Zed Theme & trace-lsp Semantic Tokens
; ==============================================================================

; --- Comments & Tree Guides (Forest Green / #6A9955) ---
(comment) @comment
(tree_guide) @comment

; --- Operators & Arrows (Bright Yellow / Operator) ---
"::" @operator
"@" @operator
(creation_arrow) @operator
(return_arrow) @operator
(change_arrow) @operator

; --- Brackets & Delimiters ---
"[" @punctuation.bracket
"]" @punctuation.bracket
"{" @punctuation.bracket
"}" @punctuation.bracket
"(" @punctuation.bracket
")" @punctuation.bracket
":" @punctuation.delimiter
"," @punctuation.delimiter

; --- Keywords & Control Flow (Purple / #c678dd) ---
(keyword) @keyword
(header_keyword) @keyword
(call_type) @keyword
(storage_keyword) @keyword
(status_keyword) @keyword
(revert_keyword) @keyword
(bytes_keyword) @keyword

; --- Contracts & Types (Cyan / Teal / #08e7c5) ---
(call_expression
  target: (identifier) @type)

(emit_trace
  name: (identifier) @type)

(labeled_address
  label: (identifier) @type)

(creation_trace
  contract: (identifier) @type)

(creation_trace
  contract: (template_value) @type)

(compile_entry
  (identifier) @type)

(struct_value
  name: (identifier) @type)

(status_entry
  (string) @string)

(log_line) @string

; --- Functions & Selectors (Yellow / Gold / #DCDCAA) ---
(call_expression
  function: (identifier) @function.method)

(call_expression
  function: (hex_selector) @function.method)

(custom_error
  name: (identifier) @function.method)

; --- Addresses & Storage Slots (Light Blue / #9cdcfe / #0550ae) ---
(call_expression
  target: (hex_literal) @property)

(labeled_address
  address: (hex_literal) @property)

(creation_trace
  address: (hex_literal) @property)

(storage_change
  slot: (identifier) @property)

(storage_change
  slot: (hex_literal) @property)

(storage_change
  slot: (number) @property)

(data_entry
  value: (hex_literal) @property)

; --- Parameters & Labels (Sky Blue / #7cc6f8 / #0a3069) ---
(named_argument
  name: (identifier) @variable)

(struct_field
  name: (identifier) @variable)

(topic_entry
  index: (number) @variable
  value: (hex_literal) @property)

; --- Gas, Amounts, Wei & Numbers (Mint Green / #B4CDA8 / #1a7f37) ---
(gas
  amount: (number) @number)

(estimate_value) @number

(scientific_estimate
  estimate: (estimate_value) @number)

(number_with_estimate
  value: (number) @number
  estimate: (scientific_estimate) @number)

(storage_change
  before: (hex_literal) @number)

(storage_change
  before: (number) @number)

(storage_change
  after: (hex_literal) @number)

(storage_change
  after: (number) @number)

(number) @number
(boolean) @boolean

; --- Strings & Revert Messages (Warm Orange / #CD9069) ---
(string) @string
(template_value) @string
(revert_message) @string

; --- Fallback Identifiers & Hex ---
(raw_hex) @property
(hex_literal) @property
(identifier) @variable
