use lsp_types::{
    SemanticToken, SemanticTokenModifier, SemanticTokenType, SemanticTokens,
    SemanticTokensFullOptions, SemanticTokensLegend, SemanticTokensOptions,
    SemanticTokensServerCapabilities,
};

// Indices into TOKEN_TYPES:
// 0: KEYWORD    -> #c678ddff (purple)
// 1: TYPE       -> #08e7c5ff (cyan/teal)
// 2: FUNCTION   -> #DCDCAAff (yellow/gold)
// 3: NUMBER     -> #B4CDA8ff (green)
// 4: STRING     -> #CD9069ff (warm orange)
// 5: OPERATOR   -> #ffd866ff (bright yellow)
// 6: VARIABLE   -> #7cc6f8ff (light blue)
// 7: PROPERTY   -> #9cdcfeff (light cyan)
// 8: COMMENT    -> #6A9955ff (forest green)
// 9: MODIFIER   -> #c678ddff (purple)

pub const TYPE_KEYWORD: u32 = 0;
pub const TYPE_TYPE: u32 = 1;
pub const TYPE_FUNCTION: u32 = 2;
pub const TYPE_NUMBER: u32 = 3;
pub const TYPE_STRING: u32 = 4;
pub const TYPE_OPERATOR: u32 = 5;
pub const TYPE_VARIABLE: u32 = 6;
pub const TYPE_PROPERTY: u32 = 7;
pub const TYPE_COMMENT: u32 = 8;
pub const TYPE_MODIFIER: u32 = 9;

pub fn legend() -> SemanticTokensLegend {
    SemanticTokensLegend {
        token_types: vec![
            SemanticTokenType::KEYWORD,
            SemanticTokenType::TYPE,
            SemanticTokenType::FUNCTION,
            SemanticTokenType::NUMBER,
            SemanticTokenType::STRING,
            SemanticTokenType::OPERATOR,
            SemanticTokenType::VARIABLE,
            SemanticTokenType::PROPERTY,
            SemanticTokenType::COMMENT,
            SemanticTokenType::MODIFIER,
        ],
        token_modifiers: vec![
            SemanticTokenModifier::DECLARATION,
            SemanticTokenModifier::DEFINITION,
            SemanticTokenModifier::READONLY,
            SemanticTokenModifier::STATIC,
        ],
    }
}

pub fn server_capabilities() -> SemanticTokensServerCapabilities {
    SemanticTokensServerCapabilities::SemanticTokensOptions(SemanticTokensOptions {
        work_done_progress_options: Default::default(),
        legend: legend(),
        range: Some(false),
        full: Some(SemanticTokensFullOptions::Bool(true)),
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawToken {
    pub line: u32,
    pub start_char: u32,
    pub length: u32,
    pub token_type: u32,
}

pub fn tokenize_document(content: &str) -> SemanticTokens {
    let mut raw_tokens = Vec::new();
    for (line_idx, line) in content.lines().enumerate() {
        tokenize_line(line_idx as u32, line, &mut raw_tokens);
    }

    // Convert raw tokens to delta-encoded LSP SemanticTokens
    let mut data = Vec::with_capacity(raw_tokens.len() * 5);
    let mut prev_line = 0;
    let mut prev_start = 0;

    for tok in raw_tokens {
        let delta_line = tok.line - prev_line;
        let delta_start = if delta_line == 0 {
            tok.start_char.saturating_sub(prev_start)
        } else {
            tok.start_char
        };

        data.push(SemanticToken {
            delta_line,
            delta_start,
            length: tok.length,
            token_type: tok.token_type,
            token_modifiers_bitset: 0,
        });

        prev_line = tok.line;
        prev_start = tok.start_char;
    }

    SemanticTokens {
        result_id: None,
        data,
    }
}

pub fn tokenize_line(line_idx: u32, line: &str, out: &mut Vec<RawToken>) {
    let chars: Vec<char> = line.chars().collect();
    let len = chars.len();
    if len == 0 {
        return;
    }

    // Precompute UTF-16 cumulative offsets so start_char and length are always UTF-16 code units
    let mut utf16_offsets = Vec::with_capacity(len + 1);
    let mut cur_utf16 = 0u32;
    for &c in &chars {
        utf16_offsets.push(cur_utf16);
        cur_utf16 += c.len_utf16() as u32;
    }
    utf16_offsets.push(cur_utf16);

    let push_tok = |out: &mut Vec<RawToken>, start: usize, end: usize, token_type: u32| {
        let s = utf16_offsets[start];
        let e = utf16_offsets[end];
        if e > s {
            out.push(RawToken {
                line: line_idx,
                start_char: s,
                length: e - s,
                token_type,
            });
        }
    };

    let mut i = 0;

    while i < len {
        let ch = chars[i];

        // 1. Whitespace
        if ch.is_whitespace() {
            i += 1;
            continue;
        }

        // 2. Tree branch markers
        if ch == '├' || ch == '└' {
            let start = i;
            while i < len && (chars[i] == '├' || chars[i] == '└' || chars[i] == '─' || chars[i] == ' ') {
                i += 1;
            }
            push_tok(out, start, i, TYPE_COMMENT);
            continue;
        }

        if ch == '│' {
            push_tok(out, i, i + 1, TYPE_COMMENT);
            i += 1;
            continue;
        }

        // 3. Arrow operators: →, ←
        if ch == '→' || ch == '←' {
            push_tok(out, i, i + 1, TYPE_OPERATOR);
            i += 1;
            continue;
        }

        // 4. Double colon ::
        if ch == ':' && i + 1 < len && chars[i + 1] == ':' {
            push_tok(out, i, i + 2, TYPE_OPERATOR);
            i += 2;
            continue;
        }

        // 5. Single punctuation / operators
        if ch == ':' || ch == ',' || ch == '@' || ch == '=' {
            push_tok(out, i, i + 1, TYPE_OPERATOR);
            i += 1;
            continue;
        }

        // 6. String literals "..."
        if ch == '"' {
            let start = i;
            i += 1;
            while i < len && chars[i] != '"' {
                if chars[i] == '\\' && i + 1 < len {
                    i += 2;
                } else {
                    i += 1;
                }
            }
            if i < len && chars[i] == '"' {
                i += 1;
            }
            push_tok(out, start, i, TYPE_STRING);
            continue;
        }

        // 7. Bracketed modifiers or gas numbers: [staticcall], [delegatecall], [Return], [Stop], [Revert], [12345]
        if ch == '[' {
            let bracket_start = i;
            // Scan content inside [...]
            let mut j = i + 1;
            while j < len && chars[j] != ']' && chars[j] != '\n' {
                j += 1;
            }

            if j < len && chars[j] == ']' {
                let inner: String = chars[i + 1..j].iter().collect();
                let inner_trimmed = inner.trim();

                let token_type = match inner_trimmed {
                    "staticcall" | "delegatecall" | "call" | "callcode" => Some(TYPE_MODIFIER),
                    "Return" | "Stop" | "SelfDestruct" | "Selfdestruct" | "PASS" | "FAIL" => Some(TYPE_KEYWORD),
                    "Revert" | "Panic" | "OutOfGas" | "OutOfFunds" | "CallTooDeep"
                    | "StackOverflow" | "StackUnderflow" | "InvalidOpcode" | "InvalidFEOpcode"
                    | "InvalidJump" | "StateChangeDuringStaticCall" | "CreateCollision"
                    | "OverflowPayment" | "PrecompileError" | "MemoryOOG" | "MemoryLimitOOG" => Some(TYPE_KEYWORD),
                    s if !s.is_empty() && s.chars().all(|c| c.is_ascii_digit()) => Some(TYPE_NUMBER),
                    s if is_scientific_number(s) => Some(TYPE_NUMBER),
                    _ => None,
                };

                if let Some(tt) = token_type {
                    push_tok(out, bracket_start, bracket_start + 1, TYPE_OPERATOR);
                    push_tok(out, bracket_start + 1, j, tt);
                    push_tok(out, j, j + 1, TYPE_OPERATOR);
                    i = j + 1;
                    continue;
                }
            }

            // Fallback: just emit '[' as operator
            push_tok(out, i, i + 1, TYPE_OPERATOR);
            i += 1;
            continue;
        }

        if ch == ']' || ch == '(' || ch == ')' || ch == '{' || ch == '}' {
            push_tok(out, i, i + 1, TYPE_OPERATOR);
            i += 1;
            continue;
        }

        // 8. Hex Address or Hash (0x...)
        if ch == '0' && i + 1 < len && (chars[i + 1] == 'x' || chars[i + 1] == 'X') {
            let start = i;
            i += 2;
            while i < len && chars[i].is_ascii_hexdigit() {
                i += 1;
            }
            let hex_len = i - start;
            let tt = if hex_len == 42 {
                TYPE_PROPERTY // 20-byte address
            } else if hex_len == 66 {
                TYPE_PROPERTY // 32-byte slot hash or topic
            } else {
                TYPE_NUMBER // other hex literal
            };
            push_tok(out, start, i, tt);
            continue;
        }

        // 9. Signed or unsigned decimal numbers / scientific numbers: -20000000000, 416831487011304318246517, 1e18
        if ch.is_ascii_digit() || ((ch == '-' || ch == '+') && i + 1 < len && chars[i + 1].is_ascii_digit()) {
            let start = i;
            if ch == '-' || ch == '+' {
                i += 1;
            }
            while i < len && (chars[i].is_ascii_alphanumeric() || chars[i] == '.' || chars[i] == '_') {
                if (chars[i] == 'e' || chars[i] == 'E') && i + 1 < len && (chars[i + 1] == '+' || chars[i + 1] == '-') {
                    i += 2;
                } else {
                    i += 1;
                }
            }
            push_tok(out, start, i, TYPE_NUMBER);
            continue;
        }

        // 10. Words / Identifiers
        if is_ident_start(ch) {
            let start = i;
            while i < len && is_ident_part(chars[i]) {
                i += 1;
            }
            let word: String = chars[start..i].iter().collect();

            // Look ahead for context
            let next_non_ws = chars[i..].iter().position(|&c| !c.is_whitespace());
            let next_char = next_non_ws.map(|pos| chars[i + pos]);
            let is_followed_by_double_colon = i + 1 < len && chars[i] == ':' && chars[i + 1] == ':';
            let is_followed_by_colon = next_char == Some(':') && !is_followed_by_double_colon;
            let is_followed_by_paren = next_char == Some('(');

            let token_type = if is_followed_by_double_colon {
                TYPE_TYPE // Contract / Caller name before ::
            } else if is_followed_by_paren {
                TYPE_FUNCTION // Function call
            } else if is_followed_by_colon {
                TYPE_VARIABLE // Parameter name before colon, e.g. from:, to:, amount:, param0:
            } else {
                classify_word(&word)
            };

            push_tok(out, start, i, token_type);
            continue;
        }

        // Catch-all: advance by 1
        i += 1;
    }
}

fn is_ident_start(c: char) -> bool {
    c.is_alphabetic() || c == '_' || c == '$'
}

fn is_ident_part(c: char) -> bool {
    c.is_alphanumeric() || c == '_' || c == '$'
}

fn is_scientific_number(s: &str) -> bool {
    let s = s.trim();
    if s.is_empty() {
        return false;
    }
    if s == ".." {
        return true;
    }
    let has_e = s.contains('e') || s.contains('E');
    let valid_chars = s.chars().all(|c| c.is_ascii_digit() || c == '.' || c == 'e' || c == 'E' || c == '+' || c == '-');
    has_e && valid_chars
}

fn classify_word(word: &str) -> u32 {
    match word {
        // Solidity Keywords
        "emit" | "new" | "return" | "returns" | "revert" | "stop" | "require" | "assert"
        | "contract" | "interface" | "function" | "modifier" | "storage" | "changes"
        | "external" | "internal" | "public" | "private" | "view" | "pure" | "payable"
        | "constant" | "immutable" | "override" | "virtual" | "memory" | "calldata" => TYPE_KEYWORD,

        // Booleans
        "true" | "false" => TYPE_KEYWORD, // Will render with keyword/boolean emphasis

        // Solidity Standard Types
        "uint256" | "uint128" | "uint112" | "uint64" | "uint32" | "uint16" | "uint8" | "uint"
        | "int256" | "int128" | "int64" | "int32" | "int16" | "int8" | "int"
        | "address" | "bool" | "bytes" | "bytes32" | "bytes4" | "string" => TYPE_TYPE,

        // Common Core Contracts & Labels in Traces
        "VM" | "Proof" | "CCCTest" | "WBNB" | "USDT" | "SENDER" | "OTHER_CALLER"
        | "Transfer" | "FlashLoan" | "Approval" | "Deposit" | "Withdrawal" | "Bought"
        | "Sold" | "Sync" | "Swap" | "Pair" | "Router" => TYPE_TYPE,

        // Common trace words
        "of" | "code" | "data" | "topic" => TYPE_KEYWORD,

        // Capitalized identifiers are usually types or contracts
        s if s.chars().next().map_or(false, |c| c.is_uppercase()) => TYPE_TYPE,

        _ => TYPE_VARIABLE,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tokenize_sample_line() {
        let line = "    ├─ [2534] 0xbb4CdB9CBd36B01bD1cBaEBF2De08d9173bc095c::balanceOf(SENDER : [0x7977BDeeE3A79Dc85Cc18739692e796B5D2513C4]) [staticcall]";
        let mut tokens = Vec::new();
        tokenize_line(0, line, &mut tokens);

        assert!(!tokens.is_empty());

        // Check gas number 2534
        let gas_tok = tokens.iter().find(|t| t.length == 4 && t.token_type == TYPE_NUMBER);
        assert!(gas_tok.is_some(), "2534 must be tokenized as NUMBER");

        // Check address 0xbb4C...
        let addr_tok = tokens.iter().find(|t| t.length == 42 && t.token_type == TYPE_PROPERTY);
        assert!(addr_tok.is_some(), "Address must be tokenized as PROPERTY");

        // Check :: operator
        let col_tok = tokens.iter().find(|t| t.length == 2 && t.token_type == TYPE_OPERATOR);
        assert!(col_tok.is_some(), ":: must be tokenized as OPERATOR");

        // Check balanceOf function
        let func_tok = tokens.iter().find(|t| t.length == 9 && t.token_type == TYPE_FUNCTION);
        assert!(func_tok.is_some(), "balanceOf must be tokenized as FUNCTION");

        // Check [staticcall] modifier
        let mod_tok = tokens.iter().find(|t| t.length == 10 && t.token_type == TYPE_MODIFIER);
        assert!(mod_tok.is_some(), "staticcall must be tokenized as MODIFIER");
    }

    #[test]
    fn test_tokenize_entire_sample() {
        let content = include_str!("../../../sample.trace");
        let sem_tokens = tokenize_document(content);
        assert!(!sem_tokens.data.is_empty(), "Document should produce semantic tokens");
    }
}
