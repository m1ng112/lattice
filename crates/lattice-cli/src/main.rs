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
    /// Dump a BSG file in human-readable format
    BsgDump {
        /// Path to the .bsg file
        file: PathBuf,
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

fn main() {
    let cli = Cli::parse();

    let result = match cli.command {
        Commands::Parse { ref file, ref format } => cmd_parse(file, format, cli.verbose),
        Commands::Check { ref file } => cmd_check(file, cli.verbose),
        Commands::Fmt { ref file, write } => cmd_fmt(file, write, cli.verbose),
        Commands::Compile { ref file, ref output } => cmd_compile(file, output, cli.verbose),
        Commands::BsgDump { ref file } => cmd_bsg_dump(file),
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

    Ok(())
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
