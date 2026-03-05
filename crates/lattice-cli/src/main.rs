use std::path::PathBuf;
use std::time::Instant;

use clap::{Parser, Subcommand};
use colored::Colorize;

#[derive(Parser)]
#[command(name = "lattice", version, about = "The Lattice programming language compiler")]
struct Cli {
    #[command(subcommand)]
    command: Commands,

    /// Verbose output
    #[arg(short, long, global = true)]
    verbose: bool,
}

#[derive(Subcommand)]
enum Commands {
    /// Parse a .lattice file and print the AST
    Parse {
        /// Input file path
        file: PathBuf,
        /// Output format (ast, json)
        #[arg(long, default_value = "ast")]
        format: String,
    },
    /// Type-check a .lattice file
    Check {
        /// Path to the .lattice file
        file: PathBuf,
    },
    /// Format a .lattice file
    Fmt {
        /// Path to the .lattice file
        file: PathBuf,
        /// Write result back to file
        #[arg(short, long)]
        write: bool,
    },
    /// Compile a .lattice file to BSG
    Compile {
        /// Path to the .lattice file
        file: PathBuf,
        /// Output file path
        #[arg(short, long)]
        output: Option<PathBuf>,
    },
    /// Verify proof obligations in a .lattice file
    Prove {
        /// Input file path
        file: PathBuf,
        /// Output format (text, json)
        #[arg(long, default_value = "text")]
        format: String,
    },
    /// Dump a BSG file in human-readable format
    BsgDump {
        /// Path to the .bsg file
        file: PathBuf,
    },
    /// Run a .lattice program
    Run {
        /// Input file path
        file: PathBuf,
        /// Enable execution trace
        #[arg(long)]
        trace: bool,
        /// Timeout in milliseconds
        #[arg(long)]
        timeout: Option<u64>,
        /// Input data as JSON (e.g. '{"NodeName": 42}')
        #[arg(long)]
        input: Option<String>,
    },
    /// Show synthesis requests in a .lattice file
    Synthesize {
        /// Input file path
        file: PathBuf,
        /// Dry-run mode: show what would be synthesized without calling LLM
        #[arg(long, default_value_t = true)]
        dry_run: bool,
        /// Output format (text, json)
        #[arg(long, default_value = "text")]
        format: String,
    },
    /// Profile a graph execution and report hotspots
    Profile {
        /// Input file path
        file: PathBuf,
        /// Hotspot threshold percentage (nodes above this are flagged)
        #[arg(long, default_value_t = 30.0)]
        threshold: f64,
        /// Input data as JSON (e.g. '{"NodeName": 42}')
        #[arg(long)]
        input: Option<String>,
    },
}

fn read_source(path: &PathBuf) -> Result<String, String> {
    std::fs::read_to_string(path)
        .map_err(|e| format!("Failed to read {}: {}", path.display(), e))
}

fn parse_source(
    source: &str,
    path: &PathBuf,
    verbose: bool,
) -> Result<lattice_parser::ast::Program, String> {
    let start = Instant::now();
    let result = lattice_parser::parser::parse(source);
    let elapsed = start.elapsed();

    if verbose {
        eprintln!(
            "{} Parsing took {:.2?}",
            "timing:".dimmed(),
            elapsed,
        );
    }

    result.map_err(|errors| {
        let mut msg = format!(
            "{} {} error(s) in {}\n",
            "error:".red().bold(),
            errors.len(),
            path.display()
        );
        for e in &errors {
            msg.push_str(&format!("  {} {}\n", "-->".red(), e));
        }
        msg
    })
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();

    let result = match cli.command {
        Commands::Parse { ref file, ref format } => cmd_parse(file, format, cli.verbose),
        Commands::Check { ref file } => cmd_check(file, cli.verbose),
        Commands::Prove { ref file, ref format } => cmd_prove(file, format, cli.verbose),
        Commands::Fmt { ref file, write } => cmd_fmt(file, write, cli.verbose),
        Commands::Compile { ref file, ref output } => cmd_compile(file, output, cli.verbose),
        Commands::BsgDump { ref file } => cmd_bsg_dump(file),
        Commands::Run {
            ref file,
            trace,
            timeout,
            ref input,
        } => cmd_run(file, trace, timeout, input.as_deref(), cli.verbose).await,
        Commands::Synthesize {
            ref file,
            dry_run,
            ref format,
        } => cmd_synthesize(file, dry_run, format, cli.verbose).await,
        Commands::Profile {
            ref file,
            threshold,
            ref input,
        } => cmd_profile(file, threshold, input.as_deref(), cli.verbose).await,
    };

    if let Err(msg) = result {
        eprint!("{}", msg);
        std::process::exit(1);
    }
}

fn cmd_parse(file: &PathBuf, format: &str, verbose: bool) -> Result<(), String> {
    let source = read_source(file)?;
    let program = parse_source(&source, file, verbose)?;

    match format {
        "ast" => {
            println!("{:#?}", program);
        }
        "json" => {
            let json = serde_json::to_string_pretty(&program)
                .map_err(|e| format!("{} JSON serialization failed: {}\n", "error:".red().bold(), e))?;
            println!("{}", json);
        }
        other => {
            return Err(format!(
                "{} Unknown format '{}'. Use 'ast' or 'json'.\n",
                "error:".red().bold(),
                other,
            ));
        }
    }

    if verbose {
        eprintln!(
            "{} Parsed {} top-level item(s) from {}",
            "info:".green(),
            program.len(),
            file.display(),
        );
    }

    Ok(())
}

fn cmd_check(file: &PathBuf, verbose: bool) -> Result<(), String> {
    let source = read_source(file)?;
    let program = parse_source(&source, file, verbose)?;

    eprintln!(
        "{} Parsed {} top-level item(s) — type checking not yet implemented.",
        "warning:".yellow().bold(),
        program.len(),
    );

    // Run proof verification as part of check
    let obligations = lattice_proof::obligation::extract_obligations(&program);
    if obligations.is_empty() {
        return Ok(());
    }

    let results = run_proof_checker(&obligations);
    let (verified, failed, _unverified, _skipped) = count_results(&results);
    let total = obligations.len();

    eprintln!(
        "{} {verified} of {total} proof obligation(s) verified",
        "proof:".cyan().bold(),
    );

    if failed > 0 {
        for (ob, result) in &results {
            if matches!(result.status, lattice_proof::status::ProofStatus::Failed { .. }) {
                eprintln!(
                    "  {} {} {}",
                    "✗".red().bold(),
                    ob.name.red(),
                    result
                        .message
                        .as_deref()
                        .unwrap_or("")
                        .dimmed(),
                );
            }
        }
        return Err(format!(
            "{} {failed} proof obligation(s) failed\n",
            "error:".red().bold(),
        ));
    }

    Ok(())
}

fn cmd_prove(file: &PathBuf, format: &str, verbose: bool) -> Result<(), String> {
    let source = read_source(file)?;
    let program = parse_source(&source, file, verbose)?;

    let start = Instant::now();
    let obligations = lattice_proof::obligation::extract_obligations(&program);
    let extraction_elapsed = start.elapsed();

    if verbose {
        eprintln!(
            "{} Extracted {} obligation(s) in {:.2?}",
            "timing:".dimmed(),
            obligations.len(),
            extraction_elapsed,
        );
    }

    if obligations.is_empty() {
        match format {
            "json" => {
                let output = serde_json::json!({
                    "file": file.display().to_string(),
                    "obligations": [],
                    "summary": { "total": 0, "verified": 0, "failed": 0, "unverified": 0, "skipped": 0 }
                });
                println!("{}", serde_json::to_string_pretty(&output).unwrap());
            }
            _ => {
                println!(
                    "{} No proof obligations found in {}",
                    "info:".cyan().bold(),
                    file.display(),
                );
            }
        }
        return Ok(());
    }

    let check_start = Instant::now();
    let results = run_proof_checker(&obligations);
    let check_elapsed = check_start.elapsed();

    if verbose {
        eprintln!(
            "{} Proof checking took {:.2?}",
            "timing:".dimmed(),
            check_elapsed,
        );
    }

    let (verified, failed, unverified, skipped) = count_results(&results);
    let total = obligations.len();

    match format {
        "json" => {
            print_prove_json(file, &results, verified, failed, unverified, skipped);
        }
        _ => {
            print_prove_text(file, &results, verified, failed, unverified, skipped, total);
        }
    }

    if failed > 0 {
        Err(format!(
            "{} {failed} of {total} proof obligation(s) failed\n",
            "error:".red().bold(),
        ))
    } else {
        Ok(())
    }
}

fn run_proof_checker(
    obligations: &[lattice_proof::obligation::ProofObligation],
) -> Vec<(lattice_proof::obligation::ProofObligation, lattice_proof::checker::ProofResult)> {
    let mut checker = lattice_proof::checker::ProofChecker::new();
    checker.add_backend(Box::new(lattice_proof::arithmetic_backend::ArithmeticBackend));
    checker.check_all(obligations)
}

fn count_results(
    results: &[(lattice_proof::obligation::ProofObligation, lattice_proof::checker::ProofResult)],
) -> (usize, usize, usize, usize) {
    use lattice_proof::status::ProofStatus;
    let mut verified = 0;
    let mut failed = 0;
    let mut unverified = 0;
    let mut skipped = 0;
    for (_, result) in results {
        match &result.status {
            ProofStatus::Verified => verified += 1,
            ProofStatus::Failed { .. } => failed += 1,
            ProofStatus::Unverified => unverified += 1,
            ProofStatus::Skipped => skipped += 1,
            ProofStatus::Timeout => unverified += 1,
        }
    }
    (verified, failed, unverified, skipped)
}

fn print_prove_text(
    file: &PathBuf,
    results: &[(lattice_proof::obligation::ProofObligation, lattice_proof::checker::ProofResult)],
    verified: usize,
    failed: usize,
    _unverified: usize,
    _skipped: usize,
    total: usize,
) {
    use lattice_proof::status::ProofStatus;

    println!(
        "{} Proof obligations for {}",
        "prove:".cyan().bold(),
        file.display(),
    );
    println!();

    for (ob, result) in results {
        match &result.status {
            ProofStatus::Verified => {
                println!(
                    "  {} {} {}",
                    "✓ VERIFIED".green().bold(),
                    ob.name,
                    format!("({}ms)", result.duration_ms).dimmed(),
                );
            }
            ProofStatus::Failed { .. } => {
                println!(
                    "  {} {} {}",
                    "✗ FAILED".red().bold(),
                    ob.name,
                    format!("({}ms)", result.duration_ms).dimmed(),
                );
                if let Some(ce) = &result.counterexample {
                    println!("    {} {}", "counterexample:".red(), ce);
                }
                if let Some(msg) = &result.message {
                    println!("    {} {}", "reason:".red(), msg);
                }
            }
            ProofStatus::Unverified | ProofStatus::Timeout => {
                println!(
                    "  {} {} {}",
                    "? UNVERIFIED".yellow().bold(),
                    ob.name,
                    format!("({}ms)", result.duration_ms).dimmed(),
                );
                if let Some(msg) = &result.message {
                    println!("    {}", msg.dimmed());
                }
            }
            ProofStatus::Skipped => {
                println!(
                    "  {} {}",
                    "- SKIPPED".dimmed(),
                    ob.name.dimmed(),
                );
                if let Some(msg) = &result.message {
                    println!("    {}", msg.dimmed());
                }
            }
        }
    }

    println!();
    if failed > 0 {
        println!(
            "{} {verified} of {total} proof obligation(s) verified, {} failed",
            "summary:".bold(),
            format!("{failed}").red().bold(),
        );
    } else {
        println!(
            "{} {verified} of {total} proof obligation(s) verified",
            "summary:".bold(),
        );
    }
}

fn print_prove_json(
    file: &PathBuf,
    results: &[(lattice_proof::obligation::ProofObligation, lattice_proof::checker::ProofResult)],
    verified: usize,
    failed: usize,
    unverified: usize,
    skipped: usize,
) {
    let obligations: Vec<serde_json::Value> = results
        .iter()
        .map(|(ob, result)| {
            let status_str = match &result.status {
                lattice_proof::status::ProofStatus::Verified => "verified",
                lattice_proof::status::ProofStatus::Failed { .. } => "failed",
                lattice_proof::status::ProofStatus::Unverified => "unverified",
                lattice_proof::status::ProofStatus::Skipped => "skipped",
                lattice_proof::status::ProofStatus::Timeout => "timeout",
            };
            serde_json::json!({
                "id": ob.id,
                "name": ob.name,
                "kind": format!("{:?}", ob.kind),
                "source": {
                    "item_name": ob.source.item_name,
                    "item_kind": ob.source.item_kind,
                },
                "status": status_str,
                "duration_ms": result.duration_ms,
                "message": result.message,
                "counterexample": result.counterexample,
            })
        })
        .collect();

    let output = serde_json::json!({
        "file": file.display().to_string(),
        "obligations": obligations,
        "summary": {
            "total": results.len(),
            "verified": verified,
            "failed": failed,
            "unverified": unverified,
            "skipped": skipped,
        }
    });

    println!("{}", serde_json::to_string_pretty(&output).unwrap());
}

fn cmd_fmt(file: &PathBuf, write: bool, verbose: bool) -> Result<(), String> {
    let source = read_source(file)?;
    let program = parse_source(&source, file, verbose)?;

    let start = Instant::now();
    let formatted = lattice_parser::printer::print_program(&program);
    let elapsed = start.elapsed();

    if verbose {
        eprintln!(
            "{} Formatting took {:.2?}",
            "timing:".dimmed(),
            elapsed,
        );
    }

    if write {
        std::fs::write(file, &formatted)
            .map_err(|e| format!("{} Failed to write {}: {}\n", "error:".red().bold(), file.display(), e))?;
        eprintln!(
            "{} Formatted {}",
            "ok:".green().bold(),
            file.display(),
        );
    } else {
        print!("{}", formatted);
    }

    Ok(())
}

fn cmd_compile(file: &PathBuf, output: &Option<PathBuf>, verbose: bool) -> Result<(), String> {
    let source = read_source(file)?;
    let program = parse_source(&source, file, verbose)?;

    let out = output
        .as_ref()
        .cloned()
        .unwrap_or_else(|| file.with_extension("bsg"));

    eprintln!(
        "{} Parsed {} top-level item(s) — BSG compilation not yet fully implemented.",
        "warning:".yellow().bold(),
        program.len(),
    );

    if verbose {
        eprintln!(
            "{} Would write output to {}",
            "info:".green(),
            out.display(),
        );
    }

    Ok(())
}

fn cmd_bsg_dump(file: &PathBuf) -> Result<(), String> {
    if !file.exists() {
        return Err(format!(
            "{} File not found: {}\n",
            "error:".red().bold(),
            file.display(),
        ));
    }
    eprintln!(
        "{} BSG dump not yet implemented for {}",
        "warning:".yellow().bold(),
        file.display(),
    );
    Ok(())
}

async fn cmd_synthesize(
    file: &PathBuf,
    dry_run: bool,
    format: &str,
    verbose: bool,
) -> Result<(), String> {
    let source = read_source(file)?;
    let program = parse_source(&source, file, verbose)?;

    let start = Instant::now();
    let requests = lattice_synthesizer::extractor::extract_requests(&program);
    let extraction_elapsed = start.elapsed();

    if verbose {
        eprintln!(
            "{} Extracted {} synthesis request(s) in {:.2?}",
            "timing:".dimmed(),
            requests.len(),
            extraction_elapsed,
        );
    }

    if requests.is_empty() {
        match format {
            "json" => {
                let output = serde_json::json!({
                    "file": file.display().to_string(),
                    "requests": [],
                });
                println!("{}", serde_json::to_string_pretty(&output).unwrap());
            }
            _ => {
                println!(
                    "{} No synthesis requests found in {}",
                    "info:".cyan().bold(),
                    file.display(),
                );
            }
        }
        return Ok(());
    }

    match format {
        "json" => {
            let json_requests: Vec<serde_json::Value> = requests
                .iter()
                .map(|r| {
                    serde_json::json!({
                        "function": r.function_name,
                        "parameters": r.parameters,
                        "return_type": r.return_type,
                        "preconditions": r.preconditions,
                        "postconditions": r.postconditions,
                        "invariants": r.invariants,
                        "strategy": r.strategy,
                        "optimize": r.optimize,
                    })
                })
                .collect();
            let output = serde_json::json!({
                "file": file.display().to_string(),
                "requests": json_requests,
                "dry_run": dry_run,
            });
            println!("{}", serde_json::to_string_pretty(&output).unwrap());
        }
        _ => {
            println!(
                "{} Synthesis requests in {}",
                "synthesize:".cyan().bold(),
                file.display(),
            );
            println!();
            for (i, req) in requests.iter().enumerate() {
                println!(
                    "  {} {}",
                    format!("[{}]", i + 1).dimmed(),
                    req.function_name.bold(),
                );
                if !req.parameters.is_empty() {
                    let params: Vec<String> = req
                        .parameters
                        .iter()
                        .map(|(n, t)| format!("{n}: {t}"))
                        .collect();
                    println!("      {} ({})", "params:".dimmed(), params.join(", "));
                }
                println!("      {} {}", "return:".dimmed(), req.return_type);
                if !req.preconditions.is_empty() {
                    println!(
                        "      {} {}",
                        "pre:".dimmed(),
                        req.preconditions.join(", "),
                    );
                }
                if !req.postconditions.is_empty() {
                    println!(
                        "      {} {}",
                        "post:".dimmed(),
                        req.postconditions.join(", "),
                    );
                }
                if let Some(ref strategy) = req.strategy {
                    println!("      {} {:?}", "strategy:".dimmed(), strategy);
                }
                if let Some(ref opt) = req.optimize {
                    println!("      {} {:?}", "optimize:".dimmed(), opt);
                }
                println!();
            }
        }
    }

    if dry_run {
        eprintln!(
            "{} Dry-run mode — no LLM calls made. Pass --dry-run=false to synthesize.",
            "info:".yellow().bold(),
        );
    } else {
        // Attempt actual synthesis
        let client = lattice_synthesizer::LlmClient::new()
            .map_err(|e| format!("{} {}\n", "error:".red().bold(), e))?;
        let synth = lattice_synthesizer::Synthesizer::new(client);

        for req in &requests {
            eprintln!(
                "{} Synthesizing {}...",
                "synth:".cyan().bold(),
                req.function_name,
            );
            let result = synth.synthesize(req).await;
            match result {
                lattice_synthesizer::SynthesisResult::Synthesized {
                    code,
                    verified,
                    attempts,
                } => {
                    eprintln!(
                        "  {} {} (verified={}, attempts={})",
                        "✓".green().bold(),
                        req.function_name,
                        verified,
                        attempts,
                    );
                    println!("{}", code);
                }
                lattice_synthesizer::SynthesisResult::Cached { code, cache_key } => {
                    eprintln!(
                        "  {} {} (cached: {})",
                        "✓".green().bold(),
                        req.function_name,
                        cache_key,
                    );
                    println!("{}", code);
                }
                lattice_synthesizer::SynthesisResult::ManualRequired { reason } => {
                    eprintln!(
                        "  {} {} — manual implementation required",
                        "✗".red().bold(),
                        req.function_name,
                    );
                    eprintln!("    {}", reason.dimmed());
                }
            }
        }
    }

    Ok(())
}

async fn cmd_profile(
    file: &PathBuf,
    threshold: f64,
    input: Option<&str>,
    verbose: bool,
) -> Result<(), String> {
    use lattice_parser::ast::Item;
    use lattice_runtime::graph::ExecutableGraph;
    use lattice_runtime::node::Value;
    use lattice_runtime::profiler;
    use lattice_runtime::scheduler::Scheduler;
    use std::collections::HashMap;

    let source = read_source(file)?;
    let program = parse_source(&source, file, verbose)?;

    let ast_graph = program
        .iter()
        .find_map(|item| match &item.node {
            Item::Graph(g) => Some(g),
            _ => None,
        })
        .ok_or_else(|| {
            format!(
                "{} No graph found in {}\n",
                "error:".red().bold(),
                file.display(),
            )
        })?;

    if verbose {
        eprintln!(
            "{} Found graph '{}' with {} member(s)",
            "info:".green(),
            ast_graph.name,
            ast_graph.members.len(),
        );
    }

    let exec_graph = ExecutableGraph::from_ast(ast_graph).map_err(|e| {
        format!("{} {}\n", "error:".red().bold(), e)
    })?;

    let inputs: HashMap<String, Value> = if let Some(json_str) = input {
        let json_val: serde_json::Value = serde_json::from_str(json_str).map_err(|e| {
            format!(
                "{} Invalid input JSON: {}\n",
                "error:".red().bold(),
                e,
            )
        })?;
        let obj = json_val.as_object().ok_or_else(|| {
            format!(
                "{} Input must be a JSON object\n",
                "error:".red().bold(),
            )
        })?;
        obj.iter()
            .map(|(k, v)| {
                let value: Value = serde_json::from_value(v.clone()).map_err(|e| {
                    format!("{} Failed to parse value for '{}': {}\n", "error:".red().bold(), k, e)
                })?;
                Ok((k.clone(), value))
            })
            .collect::<Result<HashMap<_, _>, String>>()?
    } else {
        HashMap::new()
    };

    // Run with tracing enabled so we can build a profile
    let scheduler = Scheduler::new().with_trace();
    let result = scheduler
        .execute(&exec_graph, inputs)
        .await
        .map_err(|e| format!("{} Runtime error: {}\n", "error:".red().bold(), e))?;

    let total_duration = std::time::Duration::from_millis(result.duration_ms);
    let report = profiler::build_report(&result.trace, total_duration);

    // Print the summary
    println!(
        "{} Profile for graph '{}'",
        "profile:".cyan().bold(),
        exec_graph.name,
    );
    println!();
    println!("{}", report.summary());

    // Hotspot analysis with user-specified threshold
    let hotspots = report.hotspots(threshold);
    if !hotspots.is_empty() {
        println!(
            "{} {} hotspot(s) above {:.1}% threshold:",
            "analysis:".yellow().bold(),
            hotspots.len(),
            threshold,
        );
        for h in &hotspots {
            let pct = if total_duration.as_nanos() > 0 {
                (h.duration.as_nanos() as f64 / total_duration.as_nanos() as f64) * 100.0
            } else {
                0.0
            };
            println!(
                "  {} {} — {:.2?} ({:.1}%)",
                "●".red(),
                h.node_name.bold(),
                h.duration,
                pct,
            );
        }
    }

    // Run optimizer analysis
    let suggestions =
        lattice_synthesizer::optimizer::analyze_hotspots(&report, &program);
    if !suggestions.is_empty() {
        println!();
        println!(
            "{} {} optimization suggestion(s):",
            "optimize:".green().bold(),
            suggestions.len(),
        );
        for s in &suggestions {
            let action = match &s.suggested_action {
                lattice_synthesizer::optimizer::SuggestedAction::Parallelize => {
                    "parallelize".to_string()
                }
                lattice_synthesizer::optimizer::SuggestedAction::Cache => "cache".to_string(),
                lattice_synthesizer::optimizer::SuggestedAction::Rewrite(hint) => {
                    format!("rewrite: {hint}")
                }
                lattice_synthesizer::optimizer::SuggestedAction::Synthesize => {
                    "re-synthesize".to_string()
                }
            };
            println!(
                "  {} {} — {} ({})",
                "→".cyan(),
                s.node_name.bold(),
                s.reason.dimmed(),
                action,
            );
        }
    }

    Ok(())
}

async fn cmd_run(
    file: &PathBuf,
    trace: bool,
    timeout: Option<u64>,
    input: Option<&str>,
    verbose: bool,
) -> Result<(), String> {
    use lattice_parser::ast::Item;
    use lattice_runtime::graph::ExecutableGraph;
    use lattice_runtime::node::Value;
    use lattice_runtime::scheduler::Scheduler;
    use std::collections::HashMap;

    let source = read_source(file)?;
    let program = parse_source(&source, file, verbose)?;

    // Find the first Graph item in the program
    let ast_graph = program
        .iter()
        .find_map(|item| match &item.node {
            Item::Graph(g) => Some(g),
            _ => None,
        })
        .ok_or_else(|| {
            format!(
                "{} No graph found in {}\n",
                "error:".red().bold(),
                file.display(),
            )
        })?;

    if verbose {
        eprintln!(
            "{} Found graph '{}' with {} member(s)",
            "info:".green(),
            ast_graph.name,
            ast_graph.members.len(),
        );
    }

    // Build executable graph from AST
    let exec_graph = ExecutableGraph::from_ast(ast_graph).map_err(|e| {
        format!("{} {}\n", "error:".red().bold(), e)
    })?;

    // Parse input JSON if provided
    let inputs: HashMap<String, Value> = if let Some(json_str) = input {
        let json_val: serde_json::Value = serde_json::from_str(json_str).map_err(|e| {
            format!(
                "{} Invalid input JSON: {}\n",
                "error:".red().bold(),
                e,
            )
        })?;
        let obj = json_val.as_object().ok_or_else(|| {
            format!(
                "{} Input must be a JSON object mapping node names to values\n",
                "error:".red().bold(),
            )
        })?;
        obj.iter()
            .map(|(k, v)| {
                let value: Value = serde_json::from_value(v.clone()).map_err(|e| {
                    format!("{} Failed to parse value for '{}': {}\n", "error:".red().bold(), k, e)
                })?;
                Ok((k.clone(), value))
            })
            .collect::<Result<HashMap<_, _>, String>>()?
    } else {
        HashMap::new()
    };

    // Configure the scheduler
    let mut scheduler = Scheduler::new();
    if trace {
        scheduler = scheduler.with_trace();
    }
    if let Some(ms) = timeout {
        scheduler = scheduler.with_timeout(ms);
    }

    // Execute the graph
    let result = scheduler
        .execute(&exec_graph, inputs)
        .await
        .map_err(|e| format!("{} Runtime error: {}\n", "error:".red().bold(), e))?;

    // Print outputs
    eprintln!(
        "\n{} Graph '{}' executed in {}ms",
        "ok:".green().bold(),
        exec_graph.name,
        result.duration_ms,
    );

    if !result.outputs.is_empty() {
        eprintln!("\n{}", "Outputs:".cyan().bold());
        for (name, value) in &result.outputs {
            let json = serde_json::to_string(value).unwrap_or_else(|_| format!("{:?}", value));
            eprintln!("  {} {} {}", "●".green(), name.bold(), json);
        }
    }

    // Print trace if enabled
    if trace && !result.trace.is_empty() {
        use lattice_runtime::scheduler::TracePhase;
        eprintln!("\n{}", "Execution trace:".cyan().bold());
        for entry in &result.trace {
            match &entry.phase {
                TracePhase::Start => {
                    eprintln!(
                        "  {} [{}ms] {} started",
                        "→".dimmed(),
                        entry.timestamp_ms,
                        entry.node.bold(),
                    );
                }
                TracePhase::Complete => {
                    let dur = entry
                        .duration_ms
                        .map(|d| format!(" ({}ms)", d))
                        .unwrap_or_default();
                    let val = entry
                        .value
                        .as_ref()
                        .map(|v| {
                            format!(
                                " = {}",
                                serde_json::to_string(v).unwrap_or_else(|_| format!("{:?}", v))
                            )
                        })
                        .unwrap_or_default();
                    eprintln!(
                        "  {} [{}ms] {} completed{}{}",
                        "✓".green(),
                        entry.timestamp_ms,
                        entry.node.bold(),
                        dur.dimmed(),
                        val.dimmed(),
                    );
                }
                TracePhase::Error(msg) => {
                    eprintln!(
                        "  {} [{}ms] {} failed: {}",
                        "✗".red(),
                        entry.timestamp_ms,
                        entry.node.bold(),
                        msg.red(),
                    );
                }
            }
        }
    }

    Ok(())
}
