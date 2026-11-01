use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq)]
pub enum CallType {
    Call,
    StaticCall,
    DelegateCall,
    Create,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ReturnStatus {
    Success(String),
    Revert(String),
    Stop,
}

#[derive(Debug, Clone, PartialEq)]
pub struct StorageDiff {
    pub slot: String,
    pub old_val: String,
    pub new_val: String,
}

#[derive(Debug, Clone, PartialEq)]
pub enum TraceItemKind {
    RootCall {
        gas: u64,
        target: String,
        function: String,
    },
    Call {
        gas: u64,
        target: String,
        function: String,
        args: String,
        call_type: CallType,
        value: Option<String>,
    },
    Create {
        gas: u64,
        contract: String,
        address: String,
    },
    Event {
        name: String,
        details: String,
    },
    StorageChanges {
        diffs: Vec<StorageDiff>,
    },
    TransientChanges {
        diffs: Vec<StorageDiff>,
    },
    Return {
        status: ReturnStatus,
    },
    Other {
        text: String,
    },
}

#[derive(Debug, Clone)]
pub struct TraceNode {
    pub id: usize,
    pub depth: usize,
    pub line_start: u32,
    pub line_end: u32,
    pub raw_line: String,
    pub kind: TraceItemKind,
    pub return_status: Option<ReturnStatus>,
    pub children: Vec<TraceNode>,
}

#[derive(Debug, Clone, Default)]
pub struct TraceDocument {
    pub roots: Vec<TraceNode>,
    pub line_count: u32,
    pub line_lengths: Vec<u32>,
    pub addresses: BTreeMap<String, String>, // Address -> Known Name/Label
    pub raw_text: String,
}

impl TraceDocument {
    pub fn line_len(&self, line: u32) -> u32 {
        self.line_lengths.get(line as usize).copied().unwrap_or(0)
    }
}

/// Calculate the indentation depth of a trace line and extract its payload.
pub fn extract_depth_and_payload(line: &str) -> Option<(usize, &str)> {
    let trimmed = line.trim();
    if trimmed.is_empty() || trimmed == "Traces:" {
        return None;
    }

    let mut num_bars = 0usize;
    let mut prefix_chars = 0usize;
    let mut byte_offset = 0usize;
    let mut has_branch = false;

    for (idx, ch) in line.char_indices() {
        match ch {
            ' ' | '\t' => {
                prefix_chars += 1;
                byte_offset = idx + 1;
            }
            '│' => {
                num_bars += 1;
                prefix_chars += 1;
                byte_offset = idx + ch.len_utf8();
            }
            '├' | '└' => {
                has_branch = true;
                byte_offset = idx + ch.len_utf8();
                break;
            }
            _ => break,
        }
    }

    if has_branch {
        // Consume any trailing '─' and spaces after '├' or '└'
        let rest = &line[byte_offset..];
        let mut extra = 0usize;
        for ch in rest.chars() {
            if ch == '─' || ch == ' ' {
                extra += ch.len_utf8();
            } else {
                break;
            }
        }
        let payload = line[byte_offset + extra..].trim();
        if payload.is_empty() {
            return None;
        }
        let space_depth = prefix_chars / 4;
        let depth = space_depth.max(num_bars + 1);
        return Some((depth, payload));
    }

    if num_bars == 0 {
        if trimmed.starts_with('[') {
            return Some((0, trimmed));
        }
    }

    let payload = line[byte_offset..].trim();
    if !payload.is_empty() {
        let depth = (prefix_chars / 4).max(num_bars);
        Some((depth, payload))
    } else {
        None
    }
}

pub fn parse_line_kind(payload: &str) -> TraceItemKind {
    // 1. Return / Stop / Revert / Halt
    if payload.starts_with("←") || payload.starts_with("<-") {
        let clean = payload.trim_start_matches("←").trim_start_matches("<-").trim();
        if clean.starts_with("[Return]") {
            let val = clean.trim_start_matches("[Return]").trim().to_string();
            return TraceItemKind::Return {
                status: ReturnStatus::Success(val),
            };
        } else if clean.starts_with("[Stop]")
            || clean.starts_with("[SelfDestruct]")
            || clean.starts_with("[Selfdestruct]")
        {
            return TraceItemKind::Return {
                status: ReturnStatus::Stop,
            };
        } else if clean.starts_with('[') {
            if let Some(close) = clean.find(']') {
                let status_tag = &clean[1..close];
                let rest = clean[close + 1..].trim();
                let val = if status_tag == "Revert" {
                    rest.to_string()
                } else if rest.is_empty() {
                    status_tag.to_string()
                } else {
                    format!("[{status_tag}] {rest}")
                };
                return TraceItemKind::Return {
                    status: ReturnStatus::Revert(val),
                };
            }
        } else {
            return TraceItemKind::Return {
                status: ReturnStatus::Success(clean.to_string()),
            };
        }
    }

    // 2. Storage & Transient changes headers
    let payload_lower = payload.to_ascii_lowercase();
    if payload_lower.starts_with("storage changes:") {
        return TraceItemKind::StorageChanges { diffs: Vec::new() };
    }
    if payload_lower.starts_with("transient changes:") || payload_lower.starts_with("transient storage changes:") {
        return TraceItemKind::TransientChanges { diffs: Vec::new() };
    }

    // 3. Storage diff entry: "@ 0x...: old → new"
    if payload.starts_with('@') {
        if let Some(colon_pos) = payload.find(':') {
            let slot = payload[1..colon_pos].trim().to_string();
            let rest = payload[colon_pos + 1..].trim();
            let (old_val, new_val) = if let Some(arrow_pos) = rest.find('→').or_else(|| rest.find("->")) {
                let sep_len = if rest.contains('→') { '→'.len_utf8() } else { 2 };
                (rest[..arrow_pos].trim().to_string(), rest[arrow_pos + sep_len..].trim().to_string())
            } else {
                (rest.to_string(), String::new())
            };
            return TraceItemKind::Other {
                text: format!("@ {slot}: {old_val} → {new_val}"),
            };
        }
    }

    // 4. Contract Creation: "→ new Proof@0x3255..." or "[1268373] → new Proof@0x3255...(0x6080...)"
    if payload.contains("→ new ") || payload.contains("-> new ") {
        let gas = parse_gas_prefix(payload).unwrap_or(0);
        let arrow_pos = payload.find("new ").unwrap_or(0);
        let spec = &payload[arrow_pos + 4..];
        let (contract, address) = if let Some(at_pos) = spec.find('@') {
            let addr_part = spec[at_pos + 1..].trim();
            let clean_addr = if let Some(paren_pos) = addr_part.find('(') {
                addr_part[..paren_pos].trim().to_string()
            } else {
                addr_part.to_string()
            };
            (spec[..at_pos].trim().to_string(), clean_addr)
        } else {
            (spec.trim().to_string(), String::new())
        };
        return TraceItemKind::Create { gas, contract, address };
    }

    // 5. Events: "emit Transfer(...)" or "emit topic 0: 0x..."
    if payload.starts_with("emit ") {
        let rest = &payload[5..];
        let (name, details) = if let Some(paren) = rest.find('(') {
            (rest[..paren].trim().to_string(), rest[paren..].trim().to_string())
        } else if let Some(colon) = rest.find(':') {
            (rest[..colon].trim().to_string(), rest[colon + 1..].trim().to_string())
        } else {
            (rest.trim().to_string(), String::new())
        };
        return TraceItemKind::Event { name, details };
    }

    // 6. Regular Call or Root Call: e.g. "[2534] 0xbb4C...::balanceOf(SENDER : [0x...]) [staticcall]"
    if let Some((gas, rest)) = split_gas_prefix(payload) {
        let mut call_type = CallType::Call;
        let mut clean_rest = rest;
        if clean_rest.ends_with("[staticcall]") {
            call_type = CallType::StaticCall;
            clean_rest = clean_rest.trim_end_matches("[staticcall]").trim();
        } else if clean_rest.ends_with("[delegatecall]") {
            call_type = CallType::DelegateCall;
            clean_rest = clean_rest.trim_end_matches("[delegatecall]").trim();
        } else if clean_rest.ends_with("[callcode]") {
            call_type = CallType::DelegateCall;
            clean_rest = clean_rest.trim_end_matches("[callcode]").trim();
        } else if clean_rest.ends_with("[call]") {
            call_type = CallType::Call;
            clean_rest = clean_rest.trim_end_matches("[call]").trim();
        }

        // Value transfer: {value: 402831596040670860741203}
        let mut value = None;
        let owned_rest: String;
        if let Some(val_start) = clean_rest.find("{value:") {
            if let Some(val_end) = clean_rest[val_start..].find('}') {
                let v = &clean_rest[val_start + 7..val_start + val_end];
                value = Some(v.trim().to_string());
                owned_rest = format!("{}{}", &clean_rest[..val_start], &clean_rest[val_start + val_end + 1..]);
                clean_rest = &owned_rest;
            }
        }

        if let Some(sep) = clean_rest.find("::") {
            let target = clean_rest[..sep].trim().to_string();
            let func_part = &clean_rest[sep + 2..];
            let (function, args) = if let Some(paren) = func_part.find('(') {
                (func_part[..paren].trim().to_string(), func_part[paren..].trim().to_string())
            } else {
                (func_part.trim().to_string(), "()".to_string())
            };

            return TraceItemKind::Call {
                gas,
                target,
                function,
                args,
                call_type,
                value,
            };
        }
    }

    TraceItemKind::Other {
        text: payload.to_string(),
    }
}

fn split_gas_prefix(s: &str) -> Option<(u64, &str)> {
    if !s.starts_with('[') {
        return None;
    }
    let close = s.find(']')?;
    let gas_str = &s[1..close];
    let gas = gas_str.parse::<u64>().ok()?;
    let rest = s[close + 1..].trim();
    Some((gas, rest))
}

fn parse_gas_prefix(s: &str) -> Option<u64> {
    split_gas_prefix(s).map(|(g, _)| g)
}

impl TraceDocument {
    pub fn parse(content: &str) -> Self {
        let mut doc = TraceDocument::default();
        doc.raw_text = content.to_string();
        let lines: Vec<&str> = content.lines().collect();
        doc.line_count = lines.len() as u32;
        doc.line_lengths = lines.iter().map(|l| l.encode_utf16().count() as u32).collect();

        let mut node_counter = 0;
        // Stack of (depth, TraceNode)
        let mut stack: Vec<(usize, TraceNode)> = Vec::new();

        for (line_idx, line) in lines.iter().enumerate() {
            let line_num = line_idx as u32;
            let trimmed = line.trim();

            // Track "Compiling: ContractName 0xAddress" header entries
            if let Some(comp_rest) = trimmed
                .strip_prefix("Compiling:")
                .or_else(|| trimmed.strip_prefix("compiling:"))
            {
                let parts: Vec<&str> = comp_rest.split_whitespace().collect();
                if parts.len() == 2 && parts[1].starts_with("0x") {
                    doc.addresses.insert(parts[1].to_string(), parts[0].to_string());
                }
                continue;
            }

            let Some((depth, payload)) = extract_depth_and_payload(line) else {
                continue;
            };

            let kind = parse_line_kind(payload);

            // Pop any nodes with depth >= current depth
            while let Some((last_depth, _)) = stack.last() {
                if *last_depth >= depth {
                    let (_, mut closed_node) = stack.pop().unwrap();
                    closed_node.line_end = closed_node.line_end.max(line_num.saturating_sub(1));
                    if let Some((_, parent)) = stack.last_mut() {
                        parent.children.push(closed_node);
                    } else {
                        doc.roots.push(closed_node);
                    }
                } else {
                    break;
                }
            }

            // If it's a Return node, attach to current top of stack if available
            if let TraceItemKind::Return { ref status } = kind {
                if let Some((_, parent)) = stack.last_mut() {
                    parent.return_status = Some(status.clone());
                    parent.line_end = parent.line_end.max(line_num);
                }
            }

            // Track deployed contract addresses from Create nodes
            if let TraceItemKind::Create { ref contract, ref address, .. } = kind {
                if !address.is_empty() && !contract.is_empty() && contract != "<unknown>" {
                    doc.addresses.insert(address.clone(), contract.clone());
                }
            }

            // Track contract labels from VM::label(target, "label")
            if let TraceItemKind::Call { ref target, ref function, ref args, .. } = kind {
                if target == "VM" && function == "label" {
                    // Extract address and label from args: (0x..., "LABEL")
                    if let Some(first_comma) = args.find(',') {
                        let addr_part = &args[1..first_comma];
                        let label_part = args[first_comma + 1..].trim_end_matches(')').trim().trim_matches('"').trim();
                        let clean_addr = if let Some(bracket) = addr_part.find('[') {
                            let end_b = addr_part.find(']').unwrap_or(addr_part.len());
                            addr_part[bracket + 1..end_b].trim().to_string()
                        } else {
                            addr_part.trim().to_string()
                        };
                        doc.addresses.insert(clean_addr, label_part.to_string());
                    }
                }
            }

            let new_node = TraceNode {
                id: node_counter,
                depth,
                line_start: line_num,
                line_end: line_num,
                raw_line: line.to_string(),
                kind,
                return_status: None,
                children: Vec::new(),
            };
            node_counter += 1;
            stack.push((depth, new_node));
        }

        // Drain remainder of the stack
        while let Some((_, mut closed_node)) = stack.pop() {
            closed_node.line_end = closed_node.line_end.max(doc.line_count.saturating_sub(1));
            if let Some((_, parent)) = stack.last_mut() {
                parent.children.push(closed_node);
            } else {
                doc.roots.push(closed_node);
            }
        }

        doc
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_depth() {
        let sample = "    ├─ [0] VM::createSelectFork(\"bsc\", 118384060 [1.183e8])";
        let (depth, payload) = extract_depth_and_payload(sample).unwrap();
        assert_eq!(depth, 1);
        assert_eq!(payload, "[0] VM::createSelectFork(\"bsc\", 118384060 [1.183e8])");

        let nested = "    │   ├─ [2534] 0xbb4CdB9CBd36B01bD1cBaEBF2De08d9173bc095c::balanceOf(0x8F73b65B4caAf64FBA2aF91cC5D4a2A1318E5D8C) [staticcall]";
        let (depth2, payload2) = extract_depth_and_payload(nested).unwrap();
        assert_eq!(depth2, 2);
        assert!(payload2.starts_with("[2534]"));
    }



    #[test]
    fn test_parse_sample() {
        let content = include_str!("../sample.trace");
        let doc = TraceDocument::parse(content);
        assert_eq!(doc.roots.len(), 2, "Expected 2 root test traces");
        assert_eq!(doc.roots[0].depth, 0);
        assert_eq!(doc.roots[1].depth, 0);

        // Check root 0: CCCTest::setUp()
        if let TraceItemKind::Call { ref function, ref target, gas, .. } = doc.roots[0].kind {
            assert_eq!(target, "CCCTest");
            assert_eq!(function, "setUp");
            assert_eq!(gas, 1312683);
        } else {
            panic!("Root 0 should be CCCTest::setUp()");
        }

        // Check root 1: CCCTest::test_profit_success()
        if let TraceItemKind::Call { ref function, ref target, gas, .. } = doc.roots[1].kind {
            assert_eq!(target, "CCCTest");
            assert_eq!(function, "test_profit_success");
            assert_eq!(gas, 14739827);
        } else {
            panic!("Root 1 should be CCCTest::test_profit_success()");
        }

        // Check children of test_profit_success()
        assert!(!doc.roots[1].children.is_empty());
    }
}
