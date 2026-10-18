use lsp_types::{
    DocumentHighlight, DocumentSymbol, FoldingRange, FoldingRangeKind, Hover,
    HoverContents, MarkupContent, MarkupKind, Position, Range, SymbolKind,
};
use crate::parser::{CallType, TraceDocument, TraceItemKind, TraceNode};

pub fn format_gas(gas: u64) -> String {
    if gas >= 1_000_000 {
        format!("{:.2}M", gas as f64 / 1_000_000.0)
    } else if gas >= 1_000 {
        format!("{:.1}k", gas as f64 / 1_000.0)
    } else {
        format!("{gas}")
    }
}

fn truncate_addr(addr: &str) -> String {
    if addr.starts_with("0x") && addr.len() >= 10 {
        format!("{}...{}", &addr[..6], &addr[addr.len() - 4..])
    } else {
        addr.to_string()
    }
}

fn truncate_chars(val: &str, max_chars: usize) -> String {
    let trimmed = val.trim();
    if trimmed.len() <= max_chars {
        return trimmed.to_string();
    }
    if let Some((byte_idx, _)) = trimmed.char_indices().nth(max_chars) {
        format!("{}...", &trimmed[..byte_idx])
    } else {
        trimmed.to_string()
    }
}

fn truncate_val(val: &str) -> String {
    truncate_chars(val, 24)
}

pub const MAX_SYMBOL_TREE_DEPTH: usize = 50;

fn node_symbol_info(node: &TraceNode) -> Option<(String, Option<String>, SymbolKind)> {
    use crate::parser::ReturnStatus;

    let (name, detail, kind) = match &node.kind {
        TraceItemKind::RootCall { gas, target, function } => {
            let ret = match &node.return_status {
                Some(ReturnStatus::Success(v)) if !v.is_empty() => format!(" -> [Return] {}", truncate_val(v)),
                Some(ReturnStatus::Stop) => " -> [Stop]".to_string(),
                Some(ReturnStatus::Revert(r)) => format!(" -> [Revert] {}", truncate_val(r)),
                _ => String::new(),
            };
            (
                format!("{target}::{function}"),
                Some(format!("[{gas_str}]{ret}", gas_str = format_gas(*gas))),
                SymbolKind::METHOD,
            )
        }
        TraceItemKind::Call {
            gas,
            target,
            function,
            call_type,
            value,
            ..
        } => {
            let type_tag = match call_type {
                CallType::StaticCall => " [staticcall]",
                CallType::DelegateCall => " [delegatecall]",
                CallType::Create => " [create]",
                CallType::Call => "",
            };
            let val_tag = if let Some(v) = value {
                format!(" {{val: {}}}", truncate_addr(v))
            } else {
                String::new()
            };
            let ret = match &node.return_status {
                Some(ReturnStatus::Success(v)) if !v.is_empty() => format!(" -> [Return] {}", truncate_val(v)),
                Some(ReturnStatus::Stop) => " -> [Stop]".to_string(),
                Some(ReturnStatus::Revert(r)) => format!(" -> [Revert] {}", truncate_val(r)),
                _ => String::new(),
            };
            (
                format!("{target}::{function}"),
                Some(format!("[{gas_str}{val_tag}]{type_tag}{ret}", gas_str = format_gas(*gas))),
                SymbolKind::FUNCTION,
            )
        }
        TraceItemKind::Create { gas, contract, address } => (
            format!("new {contract}"),
            Some(format!("@{address} [{}]", format_gas(*gas))),
            SymbolKind::CONSTRUCTOR,
        ),
        TraceItemKind::Event { name: ev_name, details } => {
            let short_details = truncate_chars(details, 40);
            (
                format!("emit {ev_name}"),
                Some(short_details),
                SymbolKind::EVENT,
            )
        }
        TraceItemKind::StorageChanges { .. } => {
            let slot_count = node
                .children
                .iter()
                .filter(|c| matches!(c.kind, TraceItemKind::Other { ref text } if text.starts_with('@')))
                .count();
            let count_tag = if slot_count > 0 {
                format!("{slot_count} slot{}", if slot_count == 1 { "" } else { "s" })
            } else {
                "storage".to_string()
            };
            (
                "storage changes".to_string(),
                Some(count_tag),
                SymbolKind::STRUCT,
            )
        }
        // Individual `@ slot: before → after` lines are intentionally NOT symbols.
        // Zed converts and highlights every symbol on its UI thread, and slot diffs
        // alone made up ~30% of the payload on large traces. They remain foldable,
        // hoverable and syntax-highlighted; the parent `storage changes` symbol
        // marks where they are.
        TraceItemKind::Other { .. } | TraceItemKind::Return { .. } => return None,
    };

    Some((name, detail, kind))
}

fn utf16_len(s: &str) -> u32 {
    if s.is_ascii() {
        s.len() as u32
    } else {
        s.encode_utf16().count() as u32
    }
}

fn symbol_ranges(node: &TraceNode, doc: &TraceDocument, name: &str) -> (Range, u32) {
    let line_text = &node.raw_line;

    // Start at the first payload character (after `│ ├─ └─` guides and spaces).
    // Zed builds the visible label from range.start..selection_range.end, so this
    // yields `[2534] Token::balanceOf` instead of `│   ├─ [2534] Token::balanceOf`.
    let mut payload_byte = 0usize;
    let mut payload_col = 0u32;
    for (idx, c) in line_text.char_indices() {
        if c == ' ' || c == '\t' || matches!(c, '│' | '├' | '└' | '─') {
            payload_byte = idx + c.len_utf8();
            payload_col += c.len_utf16() as u32;
        } else {
            break;
        }
    }

    let (sel_start, sel_end) = if let Some(rel_idx) = line_text[payload_byte..].find(name) {
        let utf16_start = payload_col + utf16_len(&line_text[payload_byte..payload_byte + rel_idx]);
        let utf16_end = utf16_start + utf16_len(name);
        (utf16_start, utf16_end)
    } else if let Some(byte_idx) = line_text.find(name) {
        let utf16_start = utf16_len(&line_text[..byte_idx]);
        let utf16_end = utf16_start + utf16_len(name);
        (utf16_start, utf16_end)
    } else {
        (0, doc.line_len(node.line_start))
    };

    let selection_range = Range {
        start: Position {
            line: node.line_start,
            character: sel_start,
        },
        end: Position {
            line: node.line_start,
            character: sel_end,
        },
    };

    (selection_range, payload_col.min(sel_start))
}

fn node_to_flat_leaf_symbol(node: &TraceNode, doc: &TraceDocument) -> Option<DocumentSymbol> {
    let (name, detail, kind) = node_symbol_info(node)?;
    let (selection_range, payload_col) = symbol_ranges(node, doc, &name);
    let line_end_col = doc.line_len(node.line_start);
    let range = Range {
        start: Position {
            line: node.line_start,
            character: payload_col,
        },
        end: Position {
            line: node.line_start,
            character: line_end_col,
        },
    };

    #[allow(deprecated)]
    Some(DocumentSymbol {
        name,
        detail,
        kind,
        tags: None,
        deprecated: None,
        range,
        selection_range,
        children: None,
    })
}

fn collect_flat_descendants(node: &TraceNode, doc: &TraceDocument, out: &mut Vec<DocumentSymbol>) {
    for child in &node.children {
        if let Some(sym) = node_to_flat_leaf_symbol(child, doc) {
            out.push(sym);
        }
        collect_flat_descendants(child, doc, out);
    }
}

pub fn node_to_document_symbol_at_depth(
    node: &TraceNode,
    doc: &TraceDocument,
    depth: usize,
) -> Option<DocumentSymbol> {
    let (name, detail, kind) = node_symbol_info(node)?;
    let (selection_range, payload_col) = symbol_ranges(node, doc, &name);

    let children = if depth >= MAX_SYMBOL_TREE_DEPTH {
        let mut flat = Vec::new();
        collect_flat_descendants(node, doc, &mut flat);
        flat
    } else {
        node.children
            .iter()
            .filter_map(|c| node_to_document_symbol_at_depth(c, doc, depth + 1))
            .collect()
    };

    // Calculate range: Start at payload_col of line_start, end at col line_len of line_end,
    // and strictly encompass all children.
    let last_line = node.line_end.min(doc.line_count.saturating_sub(1));
    let last_char = doc.line_len(last_line);
    let mut end_pos = Position {
        line: last_line,
        character: last_char,
    };

    for child in &children {
        if child.range.end.line > end_pos.line
            || (child.range.end.line == end_pos.line && child.range.end.character > end_pos.character)
        {
            end_pos = child.range.end;
        }
    }

    let range = Range {
        start: Position {
            line: node.line_start,
            character: payload_col,
        },
        end: end_pos,
    };

    #[allow(deprecated)]
    Some(DocumentSymbol {
        name,
        detail,
        kind,
        tags: None,
        deprecated: None,
        range,
        selection_range,
        children: if children.is_empty() { None } else { Some(children) },
    })
}

pub fn node_to_document_symbol(node: &TraceNode, doc: &TraceDocument) -> Option<DocumentSymbol> {
    node_to_document_symbol_at_depth(node, doc, 0)
}



impl TraceDocument {
    pub fn to_document_symbols(&self) -> Vec<DocumentSymbol> {
        self.roots.iter().filter_map(|r| node_to_document_symbol(r, self)).collect()
    }

    pub fn to_folding_ranges(&self) -> Vec<FoldingRange> {
        let mut ranges = Vec::new();
        fn collect_ranges(node: &TraceNode, out: &mut Vec<FoldingRange>) {
            if node.line_end > node.line_start {
                out.push(FoldingRange {
                    start_line: node.line_start,
                    start_character: None,
                    end_line: node.line_end,
                    end_character: None,
                    kind: Some(FoldingRangeKind::Region),
                    collapsed_text: None,
                });
            }
            for child in &node.children {
                collect_ranges(child, out);
            }
        }

        for root in &self.roots {
            collect_ranges(root, &mut ranges);
        }
        ranges
    }

    pub fn hover_at(&self, position: Position) -> Option<Hover> {
        let line_idx = position.line;
        // Find line content
        fn find_node<'a>(nodes: &'a [TraceNode], line: u32) -> Option<&'a TraceNode> {
            for node in nodes {
                if node.line_start == line {
                    return Some(node);
                }
                if line >= node.line_start && line <= node.line_end {
                    if let Some(child_match) = find_node(&node.children, line) {
                        return Some(child_match);
                    }
                    return Some(node);
                }
            }
            None
        }

        let node = find_node(&self.roots, line_idx)?;
        let mut md = String::new();

        match &node.kind {
            TraceItemKind::Call {
                gas,
                target,
                function,
                args,
                call_type,
                value,
            } => {
                let call_type_str = match call_type {
                    CallType::Call => "CALL",
                    CallType::StaticCall => "STATICCALL",
                    CallType::DelegateCall => "DELEGATECALL",
                    CallType::Create => "CREATE",
                };
                md.push_str(&format!("### **{call_type_str}**: `{target}::{function}`\n\n"));
                md.push_str(&format!("- **Gas Consumed**: `{} gas` (`{}`)\n", gas, format_gas(*gas)));

                if let Some(known) = self.addresses.get(target) {
                    md.push_str(&format!("- **Known Label**: `{known}`\n"));
                }
                if let Some(val) = value {
                    md.push_str(&format!("- **Value Attached**: `{val}` wei\n"));
                }
                if !args.is_empty() && args != "()" {
                    md.push_str(&format!("- **Arguments**: `{args}`\n"));
                }
                if let Some(ref status) = node.return_status {
                    md.push_str(&format!("- **Result**: `{:?}`\n", status));
                }
            }
            TraceItemKind::Create { gas, contract, address } => {
                md.push_str(&format!("### **CONTRACT CREATION**: `{contract}`\n\n"));
                md.push_str(&format!("- **Deployed Address**: `{address}`\n"));
                md.push_str(&format!("- **Gas Consumed**: `{} gas` (`{}`)\n", gas, format_gas(*gas)));
            }
            TraceItemKind::Event { name, details } => {
                md.push_str(&format!("### **EVENT EMITTED**: `{name}`\n\n"));
                md.push_str(&format!("- **Parameters**: `{details}`\n"));
            }
            TraceItemKind::StorageChanges { .. } => {
                md.push_str("### **STORAGE CHANGES**\n\nShows contract state modifications made in this call scope.\n");
            }
            _ => {
                md.push_str(&format!("```\n{}\n```", node.raw_line.trim()));
            }
        }

        Some(Hover {
            contents: HoverContents::Markup(MarkupContent {
                kind: MarkupKind::Markdown,
                value: md,
            }),
            range: None,
        })
    }

    pub fn highlights_at(&self, _position: Position) -> Option<Vec<DocumentHighlight>> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sample_symbols_and_folding() {
        let content = include_str!("../../../sample.trace");
        let doc = TraceDocument::parse(content);

        // Document symbols
        let symbols = doc.to_document_symbols();
        assert_eq!(symbols.len(), 2, "Expected 2 root symbols");
        assert!(symbols[0].name.contains("CCCTest::setUp"));
        assert!(symbols[1].name.contains("CCCTest::test_profit_success"));

        // Children under test_profit_success
        let test_children = symbols[1].children.as_ref().expect("test_profit_success must have children");
        assert!(!test_children.is_empty(), "test_profit_success should have multiple children");

        // Look for Proof::run() in children
        let proof_run = test_children.iter().find(|s| s.name.contains("Proof::run"));
        assert!(proof_run.is_some(), "Proof::run should exist in symbol tree");
        let proof_children = proof_run.unwrap().children.as_ref().expect("Proof::run should have nested sub-calls");
        assert!(!proof_children.is_empty(), "Proof::run children must not be empty");

        // Folding ranges
        let folding = doc.to_folding_ranges();
        assert!(!folding.is_empty(), "Folding ranges must be generated for nested calls");
        for range in &folding {
            assert!(range.end_line > range.start_line, "Folding range must span across lines");
        }

        // Hover test on root
        let hover = doc.hover_at(Position { line: 12, character: 4 });
        assert!(hover.is_some(), "Hover should return documentation for line 12");
    }
}

