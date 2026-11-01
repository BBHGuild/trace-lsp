use lsp_types::{
    DocumentHighlight, DocumentSymbol, FoldingRange, FoldingRangeKind,
    GotoDefinitionResponse, Hover, HoverContents, Location, MarkupContent,
    MarkupKind, Position, Range, SymbolKind, Url,
};
use std::path::{Path, PathBuf};
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
        TraceItemKind::TransientChanges { .. } => {
            let slot_count = node
                .children
                .iter()
                .filter(|c| matches!(c.kind, TraceItemKind::Other { ref text } if text.starts_with('@')))
                .count();
            let count_tag = if slot_count > 0 {
                format!("{slot_count} slot{}", if slot_count == 1 { "" } else { "s" })
            } else {
                "transient".to_string()
            };
            (
                "transient changes".to_string(),
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
            TraceItemKind::TransientChanges { .. } => {
                md.push_str("### **TRANSIENT STORAGE CHANGES (EIP-1153)**\n\nShows transient state (TSTORE) modifications made in this call scope. Discarded at transaction end.\n");
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

    pub fn goto_definition(&self, position: Position, trace_uri: &Url) -> Option<GotoDefinitionResponse> {
        let line_idx = position.line;

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

        let node = find_node(&self.roots, line_idx);
        let query = if let Some(n) = node {
            if n.line_start == line_idx {
                extract_def_query_from_node(n)
            } else {
                extract_def_query_from_line(&n.raw_line).or_else(|| extract_def_query_from_node(n))
            }
        } else {
            None
        }?;

        let trace_path = trace_uri.to_file_path().ok()?;
        let candidate_files = find_candidate_sol_files(&trace_path);
        if candidate_files.is_empty() {
            return None;
        }

        let mut scored_files: Vec<(i32, PathBuf)> = candidate_files
            .into_iter()
            .map(|path| {
                let score = score_file(&path, query.target_name.as_deref(), query.target_addr.as_deref());
                (score, path)
            })
            .collect();
        scored_files.sort_by(|a, b| b.0.cmp(&a.0));

        let mut matches = Vec::new();
        for (score, file_path) in scored_files {
            if !matches.is_empty() && score < 100 {
                break;
            }
            if let Ok(content) = std::fs::read_to_string(&file_path) {
                if let Ok(uri) = Url::from_file_path(&file_path) {
                    let file_matches = find_symbol_definitions(&content, &query, &uri);
                    matches.extend(file_matches);
                }
            }
            if matches.len() >= 10 {
                break;
            }
        }

        if matches.is_empty() {
            None
        } else if matches.len() == 1 {
            Some(GotoDefinitionResponse::Scalar(matches.remove(0)))
        } else {
            Some(GotoDefinitionResponse::Array(matches))
        }
    }
}

#[derive(Debug, Clone)]
enum DefQueryKind {
    Function(String),
    Event(String),
    Contract(String),
}

#[derive(Debug, Clone)]
struct DefQuery {
    target_name: Option<String>,
    target_addr: Option<String>,
    kind: DefQueryKind,
}

fn extract_target_parts(raw: &str) -> (Option<String>, Option<String>) {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return (None, None);
    }
    if let Some(bracket_start) = trimmed.find('[') {
        if let Some(bracket_end) = trimmed[bracket_start..].find(']') {
            let addr = &trimmed[bracket_start + 1..bracket_start + bracket_end];
            let name = trimmed[..bracket_start].trim().trim_end_matches(':').trim();
            return (
                if name.is_empty() { None } else { Some(name.to_string()) },
                if addr.starts_with("0x") { Some(addr.to_string()) } else { None },
            );
        }
    }
    if let Some(paren_start) = trimmed.find('(') {
        if let Some(paren_end) = trimmed[paren_start..].find(')') {
            let addr = &trimmed[paren_start + 1..paren_start + paren_end];
            let name = trimmed[..paren_start].trim();
            return (
                if name.is_empty() { None } else { Some(name.to_string()) },
                if addr.starts_with("0x") { Some(addr.to_string()) } else { None },
            );
        }
    }
    if let Some(colon) = trimmed.find(':') {
        let name = trimmed[..colon].trim();
        let addr = trimmed[colon + 1..].trim();
        return (
            if name.is_empty() { None } else { Some(name.to_string()) },
            if addr.starts_with("0x") { Some(addr.to_string()) } else { None },
        );
    }
    if trimmed.starts_with("0x") {
        (None, Some(trimmed.to_string()))
    } else {
        (Some(trimmed.to_string()), None)
    }
}

fn extract_def_query_from_node(node: &TraceNode) -> Option<DefQuery> {
    match &node.kind {
        TraceItemKind::Call { target, function, .. }
        | TraceItemKind::RootCall { target, function, .. } => {
            let (target_name, target_addr) = extract_target_parts(target);
            Some(DefQuery {
                target_name,
                target_addr,
                kind: DefQueryKind::Function(function.clone()),
            })
        }
        TraceItemKind::Create { contract, address, .. } => {
            let (mut name, mut addr) = extract_target_parts(contract);
            if addr.is_none() && address.starts_with("0x") {
                addr = Some(address.clone());
            }
            if name.is_none() && !contract.starts_with("0x") {
                name = Some(contract.clone());
            }
            let sym = name.clone().unwrap_or_else(|| contract.clone());
            Some(DefQuery {
                target_name: name,
                target_addr: addr,
                kind: DefQueryKind::Contract(sym),
            })
        }
        TraceItemKind::Event { name, .. } => {
            Some(DefQuery {
                target_name: None,
                target_addr: None,
                kind: DefQueryKind::Event(name.clone()),
            })
        }
        TraceItemKind::Other { text } => extract_def_query_from_line(text),
        _ => None,
    }
}

fn extract_def_query_from_line(line: &str) -> Option<DefQuery> {
    if let Some(sep) = line.find("::") {
        let before = line[..sep].trim();
        let target_str = before
            .rsplit(|c: char| c.is_whitespace() || c == ']' || c == '─' || c == '├' || c == '│')
            .next()
            .unwrap_or(before);
        let after = &line[sep + 2..];
        let func_str = after
            .split(|c: char| c == '(' || c == '{' || c.is_whitespace())
            .next()
            .unwrap_or(after);
        if !target_str.is_empty() && !func_str.is_empty() {
            let (target_name, target_addr) = extract_target_parts(target_str);
            return Some(DefQuery {
                target_name,
                target_addr,
                kind: DefQueryKind::Function(func_str.to_string()),
            });
        }
    }
    if let Some(emit_pos) = line.find("emit ") {
        let after = &line[emit_pos + 5..];
        let ev_name = after
            .split(|c: char| c == '(' || c == ':' || c.is_whitespace())
            .next()
            .unwrap_or(after);
        if !ev_name.is_empty() {
            return Some(DefQuery {
                target_name: None,
                target_addr: None,
                kind: DefQueryKind::Event(ev_name.to_string()),
            });
        }
    }
    None
}

fn collect_sol_files_in_dir(dir: &Path, depth: usize, max_depth: usize, out: &mut Vec<PathBuf>) {
    if depth > max_depth {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if name.starts_with('.')
                || name == "node_modules"
                || name == "cache"
                || name == "out"
                || name == "artifacts"
                || name == "target"
                || name == "dist"
                || name == "build"
            {
                continue;
            }
            collect_sol_files_in_dir(&path, depth + 1, max_depth, out);
        } else if path.is_file() && path.extension().and_then(|ext| ext.to_str()) == Some("sol") {
            out.push(path);
        }
    }
}

fn find_candidate_sol_files(trace_path: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    let mut visited_dirs = std::collections::HashSet::new();

    let start_dir = match trace_path.parent() {
        Some(d) => d,
        None => return files,
    };

    collect_sol_files_in_dir(start_dir, 0, 4, &mut files);
    visited_dirs.insert(start_dir.to_path_buf());

    let mut curr = start_dir.parent();
    let mut levels = 0;
    while let Some(parent) = curr {
        if levels >= 4 {
            break;
        }
        for sub in &["src", "test", "script", "contracts", "interfaces"] {
            let sub_path = parent.join(sub);
            if sub_path.is_dir() && !visited_dirs.contains(&sub_path) {
                visited_dirs.insert(sub_path.clone());
                collect_sol_files_in_dir(&sub_path, 0, 3, &mut files);
            }
        }
        if parent.join(".git").exists() {
            break;
        }
        curr = parent.parent();
        levels += 1;
    }

    files.sort();
    files.dedup();
    files
}

fn score_file(path: &Path, target_name: Option<&str>, target_addr: Option<&str>) -> i32 {
    let filename = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
    let filename_lower = filename.to_lowercase();
    let mut score = 0;

    if let Some(addr) = target_addr {
        let addr_clean = addr.strip_prefix("0x").unwrap_or(addr).to_lowercase();
        if !addr_clean.is_empty() && filename_lower.contains(&addr_clean) {
            score += 1000;
            if filename_lower.contains("vuln") {
                score += 200;
            } else if filename_lower.contains("aireconstruction") || filename_lower.contains("clean") {
                score += 150;
            } else if filename_lower.contains("decomp") {
                score += 50;
            }
        }
    }

    if let Some(name) = target_name {
        let name_lower = name.to_lowercase();
        let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("").to_lowercase();
        if stem == name_lower
            || stem == format!("{name_lower}.t")
            || stem == format!("{name_lower}.s")
        {
            score += 800;
        } else if filename_lower.contains(&name_lower) {
            score += 400;
        }
    }

    let path_str = path.to_string_lossy();
    if path_str.contains("/test/") {
        score += 50;
    }
    if path_str.contains("/src/") {
        score += 40;
    }

    score
}

fn is_function_def(line: &str, fn_name: &str) -> bool {
    if let Some(pos) = line.find("function ") {
        let after = line[pos + 9..].trim_start();
        if after.starts_with(fn_name) {
            let remainder = &after[fn_name.len()..];
            if remainder.starts_with('(') || remainder.starts_with(' ') || remainder.starts_with('\t') {
                return true;
            }
        }
    }
    if (fn_name == "fallback" || fn_name == "receive") && line.contains(fn_name) {
        if let Some(pos) = line.find(fn_name) {
            let remainder = line[pos + fn_name.len()..].trim_start();
            if remainder.starts_with('(') {
                return true;
            }
        }
    }
    if let Some(pos) = line.find("modifier ") {
        let after = line[pos + 9..].trim_start();
        if after.starts_with(fn_name) {
            let remainder = &after[fn_name.len()..];
            if remainder.starts_with('(')
                || remainder.starts_with(' ')
                || remainder.starts_with('\t')
                || remainder.starts_with('{')
            {
                return true;
            }
        }
    }
    false
}

fn is_contract_def(line: &str, contract_name: &str) -> bool {
    for kw in &["contract ", "interface ", "library "] {
        if let Some(pos) = line.find(kw) {
            let after = line[pos + kw.len()..].trim_start();
            if after.starts_with(contract_name) {
                let remainder = &after[contract_name.len()..];
                if remainder.is_empty()
                    || remainder.starts_with(' ')
                    || remainder.starts_with('{')
                    || remainder.starts_with(" is")
                {
                    return true;
                }
            }
        }
    }
    if contract_name.to_lowercase() == "constructor" || line.starts_with("constructor") || line.contains(" constructor") {
        if let Some(pos) = line.find("constructor") {
            let remainder = line[pos + 11..].trim_start();
            if remainder.starts_with('(') {
                return true;
            }
        }
    }
    false
}

fn is_event_def(line: &str, event_name: &str) -> bool {
    if let Some(pos) = line.find("event ") {
        let after = line[pos + 6..].trim_start();
        if after.starts_with(event_name) {
            let remainder = &after[event_name.len()..];
            if remainder.starts_with('(') || remainder.starts_with(' ') || remainder.starts_with('\t') {
                return true;
            }
        }
    }
    false
}

fn find_symbol_definitions(content: &str, query: &DefQuery, uri: &Url) -> Vec<Location> {
    let mut locations = Vec::new();

    for (line_idx, line) in content.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.starts_with("//") || trimmed.starts_with("/*") || trimmed.starts_with('*') {
            continue;
        }

        let is_match = match &query.kind {
            DefQueryKind::Function(fn_name) => is_function_def(trimmed, fn_name),
            DefQueryKind::Contract(contract_name) => is_contract_def(trimmed, contract_name),
            DefQueryKind::Event(event_name) => is_event_def(trimmed, event_name),
        };

        if is_match {
            let symbol_name = match &query.kind {
                DefQueryKind::Function(n) => n.as_str(),
                DefQueryKind::Contract(n) => n.as_str(),
                DefQueryKind::Event(n) => n.as_str(),
            };

            let start_char = if let Some(byte_pos) = line.find(symbol_name) {
                utf16_len(&line[..byte_pos])
            } else {
                0
            };
            let end_char = start_char + utf16_len(symbol_name);

            locations.push(Location {
                uri: uri.clone(),
                range: Range {
                    start: Position {
                        line: line_idx as u32,
                        character: start_char,
                    },
                    end: Position {
                        line: line_idx as u32,
                        character: end_char,
                    },
                },
            });
        }
    }

    locations
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sample_symbols_and_folding() {
        let content = include_str!("../sample.trace");
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

    #[test]
    fn test_goto_definition() {
        let temp_dir = tempfile::tempdir().expect("create tempdir");
        let project_dir = temp_dir.path();
        let src_dir = project_dir.join("src");
        let test_dir = project_dir.join("test");
        std::fs::create_dir_all(&src_dir).unwrap();
        std::fs::create_dir_all(&test_dir).unwrap();

        let trace_content = r#"Traces:
  [14739827] CCCTest::test_profit_success()
    ├─ [14716127] Proof::run()
    │   ├─ [27962] 0xbb4CdB9CBd36B01bD1cBaEBF2De08d9173bc095c::transfer(0x1234, 100)
    │   │   └─ emit Transfer(from: 0x1, to: 0x2, amount: 100)
    └─ ← [Stop]
"#;
        let trace_path = project_dir.join("tx-v1.trace");
        std::fs::write(&trace_path, trace_content).unwrap();

        let vuln_sol = r#"// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

contract WBNB {
    event Transfer(address indexed from, address indexed to, uint256 amount);

    function transfer(address to, uint256 amount) external returns (bool) {
        emit Transfer(msg.sender, to, amount);
        return true;
    }
}
"#;
        let vuln_path = src_dir.join("0xbb4CdB9CBd36B01bD1cBaEBF2De08d9173bc095c-AIReconstruction.sol");
        std::fs::write(&vuln_path, vuln_sol).unwrap();

        let test_sol = r#"// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

contract Proof {
    function run() external {
    }
}

contract CCCTest {
    function test_profit_success() public {
    }
}
"#;
        let test_sol_path = test_dir.join("CCCTest.t.sol");
        std::fs::write(&test_sol_path, test_sol).unwrap();

        let doc = TraceDocument::parse(trace_content);
        let trace_uri = Url::from_file_path(&trace_path).unwrap();

        // 1. Jump from CCCTest::test_profit_success() on line 1
        let def1 = doc.goto_definition(Position { line: 1, character: 15 }, &trace_uri);
        assert!(def1.is_some(), "Should find definition for CCCTest::test_profit_success");
        if let Some(GotoDefinitionResponse::Scalar(loc)) = def1 {
            assert!(loc.uri.as_str().contains("CCCTest.t.sol"));
            assert_eq!(loc.range.start.line, 9); // function test_profit_success line in test_sol
        } else {
            panic!("Expected scalar location for test_profit_success");
        }

        // 2. Jump from Proof::run() on line 2
        let def2 = doc.goto_definition(Position { line: 2, character: 15 }, &trace_uri);
        assert!(def2.is_some(), "Should find definition for Proof::run");
        if let Some(GotoDefinitionResponse::Scalar(loc)) = def2 {
            assert!(loc.uri.as_str().contains("CCCTest.t.sol"));
            assert_eq!(loc.range.start.line, 4); // function run line in test_sol
        } else {
            panic!("Expected scalar location for Proof::run");
        }

        // 3. Jump from 0xbb4CdB...::transfer on line 3
        let def3 = doc.goto_definition(Position { line: 3, character: 20 }, &trace_uri);
        assert!(def3.is_some(), "Should find definition for transfer in 0xbb4C...-AIReconstruction.sol");
        if let Some(GotoDefinitionResponse::Scalar(loc)) = def3 {
            assert!(loc.uri.as_str().contains("0xbb4CdB9CBd36B01bD1cBaEBF2De08d9173bc095c-AIReconstruction.sol"));
            assert_eq!(loc.range.start.line, 6); // function transfer line in vuln_sol
        } else {
            panic!("Expected scalar location for transfer");
        }

        // 4. Jump from emit Transfer on line 4
        let def4 = doc.goto_definition(Position { line: 4, character: 20 }, &trace_uri);
        assert!(def4.is_some(), "Should find definition for emit Transfer");
        if let Some(GotoDefinitionResponse::Scalar(loc)) = def4 {
            assert!(loc.uri.as_str().contains("0xbb4CdB9CBd36B01bD1cBaEBF2De08d9173bc095c-AIReconstruction.sol"));
            assert_eq!(loc.range.start.line, 4); // event Transfer line in vuln_sol
        } else {
            panic!("Expected scalar location for emit Transfer");
        }
    }

    #[test]
    fn test_transient_changes_symbols_and_hover() {
        let trace = r#"
  [49923] VaultTest::test_DepositWithTransientLock()
    ├─ [44531] Vault::deposit(100000000000000000000 [1e20])
    │   ├─ [100] TLOAD 0x0 → (0x0)
    │   ├─ [100] TSTORE 0x0: 0x0 → 0x1
    │   ├─  storage changes:
    │   │   @ 0: 0 → 0x0000000000000000000000000000000000000000000000056bc75e2d63100000
    │   ├─  transient changes:
    │   │   @ 0: 0 → 0
    │   │   @ 256: 0 → 0x0000000000000000000000000000000000000000000000056bc75e2d63100000
    │   └─ ← [Stop]
    └─ ← [Stop]
"#;
        let doc = TraceDocument::parse(trace);
        let symbols = doc.to_document_symbols();
        assert_eq!(symbols.len(), 1);
        let root = &symbols[0];
        assert_eq!(root.name, "VaultTest::test_DepositWithTransientLock");
        let root_children = root.children.as_ref().unwrap();
        assert_eq!(root_children.len(), 1);
        let call = &root_children[0];
        assert_eq!(call.name, "Vault::deposit");
        let call_children = call.children.as_ref().unwrap();
        assert_eq!(call_children.len(), 2);
        assert_eq!(call_children[0].name, "storage changes");
        assert_eq!(call_children[0].detail.as_deref(), Some("1 slot"));
        assert_eq!(call_children[1].name, "transient changes");
        assert_eq!(call_children[1].detail.as_deref(), Some("2 slots"));

        let hover = doc.hover_at(Position { line: 7, character: 15 }).unwrap();
        if let HoverContents::Markup(content) = hover.contents {
            assert!(content.value.contains("TRANSIENT STORAGE CHANGES (EIP-1153)"));
        } else {
            panic!("Expected MarkupContent");
        }
    }
}

