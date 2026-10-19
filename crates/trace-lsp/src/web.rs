use crate::parser::{CallType, TraceDocument, TraceItemKind, TraceNode};
use serde_json::json;

pub fn generate_html(doc: &TraceDocument, title: &str) -> String {
    let tree_json = serde_json::to_string(&convert_doc_to_json(doc)).unwrap_or_else(|_| "[]".to_string());

    format!(r#"<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="UTF-8">
  <meta name="viewport" content="width=device-width, initial-scale=1.0">
  <title>{title} - Phalcon Style Trace Explorer</title>
  <style>
    :root {{
      --bg: #0f172a;
      --card-bg: #1e293b;
      --border: #334155;
      --text: #f8fafc;
      --text-muted: #94a3b8;
      --call: #38bdf8;
      --staticcall: #818cf8;
      --delegatecall: #c084fc;
      --create: #f59e0b;
      --event: #34d399;
      --storage: #fb7185;
      --gas: #f59e0b;
      --badge-bg: #1e293b;
    }}
    * {{ box-sizing: border-box; margin: 0; padding: 0; }}
    body {{
      font-family: ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, "Liberation Mono", "Courier New", monospace;
      background: var(--bg);
      color: var(--text);
      font-size: 13px;
      line-height: 1.5;
      padding: 16px;
    }}
    header {{
      display: flex;
      align-items: center;
      justify-content: space-between;
      padding-bottom: 16px;
      border-bottom: 1px solid var(--border);
      margin-bottom: 16px;
    }}
    .title {{
      display: flex;
      align-items: center;
      gap: 12px;
      font-size: 16px;
      font-weight: 700;
      color: #38bdf8;
    }}
    .toolbar {{
      display: flex;
      align-items: center;
      gap: 16px;
      background: var(--card-bg);
      padding: 10px 16px;
      border-radius: 8px;
      border: 1px solid var(--border);
      margin-bottom: 16px;
      flex-wrap: wrap;
    }}
    .toggle-label {{
      display: flex;
      align-items: center;
      gap: 6px;
      color: var(--text-muted);
      cursor: pointer;
      user-select: none;
    }}
    .toggle-label input {{ cursor: pointer; }}
    .search-input {{
      background: #0f172a;
      border: 1px solid var(--border);
      color: var(--text);
      padding: 6px 12px;
      border-radius: 6px;
      outline: none;
      font-family: inherit;
      font-size: 13px;
      margin-left: auto;
      width: 250px;
    }}
    .btn {{
      background: #334155;
      color: #fff;
      border: none;
      padding: 6px 12px;
      border-radius: 6px;
      cursor: pointer;
      font-family: inherit;
      font-size: 12px;
    }}
    .btn:hover {{ background: #475569; }}
    .tree-container {{
      background: #090d16;
      border: 1px solid var(--border);
      border-radius: 8px;
      padding: 16px;
      overflow-x: auto;
    }}
    .node-row {{
      display: flex;
      align-items: center;
      gap: 8px;
      padding: 3px 0;
      border-radius: 4px;
      white-space: nowrap;
    }}
    .node-row:hover {{
      background: rgba(255, 255, 255, 0.04);
    }}
    .toggle-btn {{
      width: 16px;
      height: 16px;
      display: inline-flex;
      align-items: center;
      justify-content: center;
      border: 1px solid var(--border);
      border-radius: 3px;
      color: var(--text-muted);
      background: var(--card-bg);
      cursor: pointer;
      font-size: 11px;
      user-select: none;
    }}
    .badge {{
      display: inline-block;
      padding: 1px 6px;
      border-radius: 4px;
      font-size: 11px;
      font-weight: 600;
      text-transform: uppercase;
    }}
    .badge-call {{ background: rgba(56, 189, 248, 0.15); color: var(--call); border: 1px solid rgba(56, 189, 248, 0.3); }}
    .badge-staticcall {{ background: rgba(129, 140, 248, 0.15); color: var(--staticcall); border: 1px solid rgba(129, 140, 248, 0.3); }}
    .badge-delegatecall {{ background: rgba(192, 132, 252, 0.15); color: var(--delegatecall); border: 1px solid rgba(192, 132, 252, 0.3); }}
    .badge-create {{ background: rgba(245, 158, 11, 0.15); color: var(--create); border: 1px solid rgba(245, 158, 11, 0.3); }}
    .badge-event {{ background: rgba(52, 211, 153, 0.15); color: var(--event); border: 1px solid rgba(52, 211, 153, 0.3); }}
    .badge-storage {{ background: rgba(251, 113, 133, 0.15); color: var(--storage); border: 1px solid rgba(251, 113, 133, 0.3); }}
    .gas-badge {{
      color: var(--gas);
      font-size: 11px;
      background: rgba(245, 158, 11, 0.1);
      padding: 1px 5px;
      border-radius: 3px;
    }}
    .target {{
      color: #93c5fd;
      font-weight: 600;
    }}
    .function-name {{
      color: #f1f5f9;
      font-weight: 600;
    }}
    .params {{
      color: #64748b;
    }}
    .return-val {{
      color: #4ade80;
      font-size: 12px;
    }}
    .guide-line {{
      display: inline-block;
      width: 18px;
      height: 100%;
      border-left: 1px solid rgba(255, 255, 255, 0.1);
      margin-right: 4px;
    }}
    .hidden {{ display: none !important; }}
  </style>
</head>
<body>
  <header>
    <div class="title">
      <svg width="20" height="20" fill="currentColor" viewBox="0 0 24 24"><path d="M4 6h16v2H4zm0 5h16v2H4zm0 5h16v2H4z"/></svg>
      <span>Invocation Flow (Phalcon Style Trace Explorer)</span>
    </div>
    <div style="color: var(--text-muted); font-size: 12px;">Generated by trace-lsp</div>
  </header>

  <div class="toolbar">
    <button class="btn" onclick="expandAll()">Expand All</button>
    <button class="btn" onclick="collapseAll()">Collapse All</button>
    <label class="toggle-label"><input type="checkbox" id="filter-static" checked onchange="render()"> Static Call</label>
    <label class="toggle-label"><input type="checkbox" id="filter-storage" checked onchange="render()"> Storage Changes</label>
    <label class="toggle-label"><input type="checkbox" id="filter-events" checked onchange="render()"> Events</label>
    <input type="text" id="search" class="search-input" placeholder="Search target, method, or param..." oninput="render()">
  </div>

  <div class="tree-container" id="tree-root"></div>

  <script>
    const data = {tree_json};
    const collapsedNodes = new Set();

    function toggleNode(id) {{
      if (collapsedNodes.has(id)) {{
        collapsedNodes.delete(id);
      }} else {{
        collapsedNodes.add(id);
      }}
      render();
    }}

    function expandAll() {{
      collapsedNodes.clear();
      render();
    }}

    function collapseAll() {{
      function collect(nodes) {{
        for (const n of nodes) {{
          if (n.children && n.children.length > 0) {{
            collapsedNodes.add(n.id);
            collect(n.children);
          }}
        }}
      }}
      collect(data);
      render();
    }}

    function renderNode(node, depth, search) {{
      const showStatic = document.getElementById('filter-static').checked;
      const showStorage = document.getElementById('filter-storage').checked;
      const showEvents = document.getElementById('filter-events').checked;

      if (!showStatic && node.callType === 'StaticCall') return '';
      if (!showStorage && node.kind === 'storage') return '';
      if (!showEvents && node.kind === 'event') return '';

      const hasChildren = node.children && node.children.length > 0;
      const isCollapsed = collapsedNodes.has(node.id);

      let toggleHtml = '<span style="width: 16px; display: inline-block;"></span>';
      if (hasChildren) {{
        toggleHtml = `<span class="toggle-btn" onclick="toggleNode(${{node.id}})">${{isCollapsed ? '+' : '-'}}</span>`;
      }}

      // Indent guides
      let guides = '';
      for (let i = 0; i < depth; i++) {{
        guides += '<span class="guide-line"></span>';
      }}

      let badgeHtml = '';
      if (node.badge) {{
        badgeHtml = `<span class="badge badge-${{node.badge.toLowerCase()}}">${{node.badge}}</span>`;
      }}

      let gasHtml = node.gas ? `<span class="gas-badge">[${{node.gas}}]</span>` : '';
      let returnHtml = node.returnVal ? `<span class="return-val">← ${{node.returnVal}}</span>` : '';

      let textMatch = !search || JSON.stringify(node).toLowerCase().includes(search.toLowerCase());
      let rowStyle = textMatch ? '' : 'opacity: 0.3;';

      let html = `
        <div class="node-wrapper">
          <div class="node-row" style="${{rowStyle}}">
            ${{guides}}
            ${{toggleHtml}}
            ${{gasHtml}}
            ${{badgeHtml}}
            <span class="target">${{node.target || ''}}</span>
            <span class="function-name">${{node.function || node.title || ''}}</span>
            <span class="params">${{node.args || ''}}</span>
            ${{returnHtml}}
          </div>
      `;

      if (hasChildren && !isCollapsed) {{
        for (const child of node.children) {{
          html += renderNode(child, depth + 1, search);
        }}
      }}

      html += '</div>';
      return html;
    }}

    function render() {{
      const search = document.getElementById('search').value;
      const container = document.getElementById('tree-root');
      let html = '';
      for (const root of data) {{
        html += renderNode(root, 0, search);
      }}
      container.innerHTML = html;
    }}

    render();
  </script>
</body>
</html>
"#, title = title, tree_json = tree_json)
}

fn convert_doc_to_json(doc: &TraceDocument) -> Vec<serde_json::Value> {
    doc.roots.iter().map(convert_node_to_json).collect()
}

fn convert_node_to_json(node: &TraceNode) -> serde_json::Value {
    let mut obj = json!({
        "id": node.id,
        "lineStart": node.line_start,
        "lineEnd": node.line_end,
    });

    match &node.kind {
        TraceItemKind::RootCall { gas, target, function } => {
            obj["kind"] = json!("call");
            obj["badge"] = json!("ROOT");
            obj["gas"] = json!(gas);
            obj["target"] = json!(target);
            obj["function"] = json!(function);
        }
        TraceItemKind::Call {
            gas,
            target,
            function,
            args,
            call_type,
            ..
        } => {
            let badge = match call_type {
                CallType::Call => "CALL",
                CallType::StaticCall => "STATICCALL",
                CallType::DelegateCall => "DELEGATECALL",
                CallType::Create => "CREATE",
            };
            obj["kind"] = json!("call");
            obj["callType"] = json!(format!("{call_type:?}"));
            obj["badge"] = json!(badge);
            obj["gas"] = json!(gas);
            obj["target"] = json!(target);
            obj["function"] = json!(function);
            obj["args"] = json!(args);
        }
        TraceItemKind::Create { gas, contract, address } => {
            obj["kind"] = json!("create");
            obj["badge"] = json!("CREATE");
            obj["gas"] = json!(gas);
            obj["target"] = json!(contract);
            obj["function"] = json!(format!("new @{address}"));
        }
        TraceItemKind::Event { name, details } => {
            obj["kind"] = json!("event");
            obj["badge"] = json!("EVENT");
            obj["function"] = json!(format!("emit {name}"));
            obj["args"] = json!(details);
        }
        TraceItemKind::StorageChanges { .. } => {
            obj["kind"] = json!("storage");
            obj["badge"] = json!("STORAGE");
            obj["title"] = json!("storage changes");
        }
        TraceItemKind::TransientChanges { .. } => {
            obj["kind"] = json!("transient");
            obj["badge"] = json!("TRANSIENT");
            obj["title"] = json!("transient changes");
        }
        TraceItemKind::Return { status } => {
            obj["kind"] = json!("return");
            obj["title"] = json!(format!("{status:?}"));
        }
        TraceItemKind::Other { text } => {
            obj["kind"] = json!("other");
            obj["title"] = json!(text);
        }
    }

    if let Some(ref st) = node.return_status {
        obj["returnVal"] = json!(format!("{st:?}"));
    }

    let children_json: Vec<serde_json::Value> = node.children.iter().map(convert_node_to_json).collect();
    obj["children"] = json!(children_json);

    obj
}
