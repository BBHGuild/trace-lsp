use std::env;
use std::fs;
use std::path::Path;
use std::process;

use trace_lsp::parser::TraceDocument;
use trace_lsp::server::run_stdio_server;
use trace_lsp::web::generate_html;

fn print_help() {
    println!(
        r#"trace-lsp: Language Server and Interactive Explorer for EVM / Foundry trace files

USAGE:
    trace-lsp [OPTIONS]
    trace-lsp web <TRACE_FILE> [OUTPUT_HTML]

OPTIONS:
    --stdio         Run in Language Server mode communicating over stdio (default)
    web <FILE>      Generate a standalone Phalcon-style interactive HTML viewer
    -h, --help      Print this help message
    -V, --version   Print version info
"#
    );
}

fn main() {
    let args: Vec<String> = env::args().collect();

    if args.len() > 1 {
        let first = &args[1];
        if first == "-h" || first == "--help" {
            print_help();
            return;
        }
        if first == "-V" || first == "--version" {
            println!("trace-lsp {}", env!("CARGO_PKG_VERSION"));
            return;
        }
        if first == "web" || first == "--web" {
            if args.len() < 3 {
                eprintln!("Error: Missing trace file argument for web command.");
                eprintln!("Usage: trace-lsp web <TRACE_FILE> [OUTPUT_HTML]");
                process::exit(1);
            }
            let trace_path = &args[2];
            let content = match fs::read_to_string(trace_path) {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("Error reading file '{trace_path}': {e}");
                    process::exit(1);
                }
            };
            let doc = TraceDocument::parse(&content);
            let file_stem = Path::new(trace_path)
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("trace");
            let output_path = if args.len() >= 4 {
                args[3].clone()
            } else {
                format!("{file_stem}_explorer.html")
            };

            let html = generate_html(&doc, file_stem);
            if let Err(e) = fs::write(&output_path, html) {
                eprintln!("Error writing HTML output to '{output_path}': {e}");
                process::exit(1);
            }
            println!("Successfully generated interactive trace explorer: {output_path}");
            return;
        }
    }

    // Default: run LSP over stdio
    if let Err(e) = run_stdio_server() {
        eprintln!("Language server terminated with error: {e}");
        process::exit(1);
    }
}
