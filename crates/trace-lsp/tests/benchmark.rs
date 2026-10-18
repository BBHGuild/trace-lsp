use std::fs;
use std::time::Instant;
use trace_lsp::parser::TraceDocument;
use lsp_types::SymbolKind;

fn resolve_trace_path(candidates: &[&str]) -> Option<String> {
    for &p in candidates {
        if std::path::Path::new(p).exists() {
            return Some(p.to_string());
        }
    }
    None
}

#[test]
fn test_benchmark_all_traces() {
    let trace_groups: &[&[&str]] = &[
        &[
            "/home/sean/Github/template/test/CCC/tx-v5.trace",
            "/home/sean/Github/bbhguild-backend/test/2026/08/CCC/tx-trace-V5.trace",
            "/home/sean/Github/template/test/CCC/tx-trace-V5.trace",
        ],
        &[
            "/home/sean/Github/template/test/CCC/tx-v1.trace",
            "/home/sean/Github/bbhguild-backend/test/2026/08/CCC/tx-trace-V1.trace",
            "/home/sean/Github/template/test/CCC/tx-trace-V1.trace",
        ],
        &[
            "/home/sean/Github/template/test/CCC/tx-sender1.trace",
            "/home/sean/Github/template/test/CCC/test/tx-trace-test.trace",
        ],
    ];

    for group in trace_groups {
        let Some(path) = resolve_trace_path(group) else {
            println!("Skipping non-existent group: {:?}", group[0]);
            continue;
        };

        let content = fs::read_to_string(&path).unwrap();
        let num_lines = content.lines().count();
        println!("\n=== Benchmarking: {path} ({} lines, {} bytes) ===", num_lines, content.len());

        let start_parse = Instant::now();
        let doc = TraceDocument::parse(&content);
        let parse_time = start_parse.elapsed();
        println!("TraceDocument::parse: {:?}", parse_time);
        assert!(parse_time.as_millis() < 100, "Parse time exceeded 100ms: {:?}", parse_time);
        assert!(!doc.roots.is_empty(), "TraceDocument must have roots");

        let start_sym = Instant::now();
        let symbols = doc.to_document_symbols();
        let sym_time = start_sym.elapsed();
        println!("to_document_symbols: {:?}, root symbols: {}", sym_time, symbols.len());
        assert!(sym_time.as_millis() < 25, "to_document_symbols exceeded 25ms: {:?}", sym_time);

        fn count_symbols(syms: &[lsp_types::DocumentSymbol]) -> (usize, usize, usize, usize) {
            let (mut fns, mut evs, mut structs, mut props) = (0, 0, 0, 0);
            for s in syms {
                match s.kind {
                    SymbolKind::FUNCTION | SymbolKind::METHOD => fns += 1,
                    SymbolKind::EVENT => evs += 1,
                    SymbolKind::STRUCT => structs += 1,
                    SymbolKind::PROPERTY => props += 1,
                    _ => {}
                }
                if let Some(children) = &s.children {
                    let (f, e, st, p) = count_symbols(children);
                    fns += f;
                    evs += e;
                    structs += st;
                    props += p;
                }
            }
            (fns, evs, structs, props)
        }

        let (fns, evs, structs, props) = count_symbols(&symbols);
        let total = fns + evs + structs + props;
        println!(
            "Symbol Counts -> Functions: {}, Events: {}, Storage Structs: {}, Storage Props: {}, Total: {}",
            fns, evs, structs, props, total
        );

        assert!(fns > 1000, "Expected >1000 function symbols, got {}", fns);
        assert!(evs > 100, "Expected >100 event symbols, got {}", evs);
    }
}

#[test]
fn test_deep_hierarchy_preservation() {
    let Some(path) = resolve_trace_path(&[
        "/home/sean/Github/template/test/CCC/tx-v5.trace",
        "/home/sean/Github/bbhguild-backend/test/2026/08/CCC/tx-trace-V5.trace",
        "/home/sean/Github/template/test/CCC/tx-trace-V5.trace",
    ]) else {
        println!("Skipping deep hierarchy test, CCC V5 trace not found");
        return;
    };

    let content = fs::read_to_string(&path).unwrap();
    let doc = TraceDocument::parse(&content);
    let symbols = doc.to_document_symbols();

    // Helper to recursively find a symbol by substring in its name
    fn find_symbol<'a>(syms: &'a [lsp_types::DocumentSymbol], needle: &str) -> Option<&'a lsp_types::DocumentSymbol> {
        for s in syms {
            if s.name.contains(needle) {
                return Some(s);
            }
            if let Some(children) = &s.children {
                if let Some(found) = find_symbol(children, needle) {
                    return Some(found);
                }
            }
        }
        None
    }

    // 1. Find line 63: TransparentUpgradeableProxy::fallback
    let fallback = find_symbol(&symbols, "TransparentUpgradeableProxy::fallback")
        .expect("Should find TransparentUpgradeableProxy::fallback");
    let fallback_children = fallback.children.as_ref().expect("fallback should have children");

    // 2. Fallback -> buy
    let buy = fallback_children
        .iter()
        .find(|c| c.name.contains("buy"))
        .expect("fallback should contain buy");
    let buy_children = buy.children.as_ref().expect("buy should have children");

    // 3. buy -> balanceOf and swapExactETHForTokensSupportingFeeOnTransferTokens
    assert!(
        buy_children.iter().any(|c| c.name.contains("balanceOf")),
        "buy should contain balanceOf child"
    );
    let swap_router = buy_children
        .iter()
        .find(|c| c.name.contains("swapExactETHForTokensSupportingFeeOnTransferTokens"))
        .expect("buy should contain swapExactETH router call");
    let swap_router_children = swap_router.children.as_ref().expect("swap router should have children");

    // 4. swap_router -> deposit
    let deposit = swap_router_children
        .iter()
        .find(|c| c.name.contains("deposit"))
        .expect("swap router should contain deposit");
    let deposit_children = deposit.children.as_ref().expect("deposit should have children");

    // 5. deposit -> emit Deposit AND storage changes (1 slot)
    let emit_deposit = deposit_children
        .iter()
        .find(|c| c.name.contains("emit Deposit"))
        .expect("deposit should contain emit Deposit");
    assert_eq!(emit_deposit.kind, SymbolKind::EVENT);

    let deposit_storage = deposit_children
        .iter()
        .find(|c| c.name.contains("storage changes"))
        .expect("deposit should contain storage changes");
    assert_eq!(deposit_storage.kind, SymbolKind::STRUCT);
    // Slot diffs are summarized in `detail`, not emitted as child symbols (perf).
    assert!(deposit_storage.children.is_none());
    assert_eq!(deposit_storage.detail.as_deref(), Some("1 slot"));

    // 6. swap_router -> PancakePair::swap
    let pair_swap = swap_router_children
        .iter()
        .find(|c| c.name.contains("PancakePair::swap"))
        .expect("swap router should contain PancakePair::swap");
    let pair_swap_children = pair_swap.children.as_ref().expect("pair swap should have children");

    // 7. pair_swap -> transfer
    let transfer = pair_swap_children
        .iter()
        .find(|c| c.name.contains("transfer"))
        .expect("pair swap should contain transfer");
    let transfer_children = transfer.children.as_ref().expect("transfer should have children");

    // 8. transfer -> emit Transfer AND storage changes (5 slots)
    let transfer_events: Vec<_> = transfer_children
        .iter()
        .filter(|c| c.name.contains("emit Transfer"))
        .collect();
    assert!(!transfer_events.is_empty(), "transfer should contain emit Transfer events");

    let transfer_storage = transfer_children
        .iter()
        .find(|c| c.name.contains("storage changes"))
        .expect("transfer should contain storage changes");
    assert_eq!(transfer_storage.kind, SymbolKind::STRUCT);
    assert_eq!(transfer_storage.detail.as_deref(), Some("5 slots"), "transfer storage should report 5 slots");

    // 9. pair_swap -> emit Sync AND emit Swap
    assert!(
        pair_swap_children.iter().any(|c| c.name.contains("emit Sync")),
        "pair swap should contain emit Sync"
    );
    assert!(
        pair_swap_children.iter().any(|c| c.name.contains("emit Swap")),
        "pair swap should contain emit Swap"
    );

    println!("All deep hierarchy assertions passed successfully! Depth >= 8 fully preserved.");
}

#[test]
fn test_fox_trace_depth_clamp_and_well_formed() {
    let Some(path) = resolve_trace_path(&[
        "/home/sean/Github/template/test/Fox/tx-v1.trace",
        "/home/sean/Github/bbhguild-backend/test/2026/08/Fox/tx-trace-V1.trace",
        "/home/sean/Github/template/test/Fox/tx-trace-V1.trace",
    ]) else {
        println!("Skipping Fox trace test, file not found");
        return;
    };

    let content = fs::read_to_string(&path).unwrap();
    let doc = TraceDocument::parse(&content);
    let start_sym = Instant::now();
    let symbols = doc.to_document_symbols();
    let sym_time = start_sym.elapsed();
    println!("Fox to_document_symbols: {:?}", sym_time);

    // 1. Serialize and test with serde_json (must succeed without recursion limit exceeded)
    let json_bytes = serde_json::to_vec(&symbols).expect("serialization failed");
    let deserialized: Vec<lsp_types::DocumentSymbol> =
        serde_json::from_slice(&json_bytes).expect("serde_json deserialization failed");

    // 2. Count total symbols
    fn count_all(syms: &[lsp_types::DocumentSymbol]) -> usize {
        let mut n = syms.len();
        for s in syms {
            if let Some(children) = &s.children {
                n += count_all(children);
            }
        }
        n
    }
    let total = count_all(&deserialized);
    println!("Fox trace total symbols: {}", total);
    assert_eq!(total, 1149, "All 1149 symbols must be preserved");

    // 3. Zed outline well_formed validation
    struct Item {
        depth: usize,
        start: (u32, u32),
        end: (u32, u32),
    }
    let mut flat = Vec::new();
    fn flatten(syms: &[lsp_types::DocumentSymbol], d: usize, out: &mut Vec<Item>) {
        for s in syms {
            out.push(Item {
                depth: d,
                start: (s.range.start.line, s.range.start.character),
                end: (s.range.end.line, s.range.end.character),
            });
            if let Some(ch) = &s.children {
                flatten(ch, d + 1, out);
            }
        }
    }
    flatten(&deserialized, 0, &mut flat);
    flat.sort_by_key(|it| it.start);

    let mut ancestors: Vec<&Item> = Vec::new();
    for it in &flat {
        assert!(it.start <= it.end, "Invalid range: start > end");
        while let Some(&parent) = ancestors.last() {
            if parent.depth >= it.depth {
                ancestors.pop();
                assert!(parent.end <= it.start, "Sibling overlap: parent.end > it.start");
            } else {
                break;
            }
        }
        if let Some(&parent) = ancestors.last() {
            assert!(it.end <= parent.end, "Child exceeds parent: it.end > parent.end");
        }
        ancestors.push(it);
    }
    println!("Fox trace well_formed verified with 0 violations!");
}




