use colored::Colorize;
use lattice_repl::{Repl, ReplResult};

fn main() {
    let mut rl = rustyline::DefaultEditor::new().unwrap();
    let mut repl = Repl::new();

    println!(
        "{} Lattice REPL v0.1.0 — :help for commands, :quit to exit",
        "welcome:".cyan().bold()
    );

    loop {
        let prompt = if repl.is_multiline() {
            "  ... > "
        } else {
            "lattice> "
        };

        match rl.readline(prompt) {
            Ok(line) => {
                let _ = rl.add_history_entry(&line);
                match repl.eval_line(&line) {
                    ReplResult::Quit => break,
                    ReplResult::Value(v) => println!("{}", v.green()),
                    ReplResult::TypeInfo(t) => println!("{} {}", "type:".cyan().bold(), t),
                    ReplResult::ProofResult(p) => println!("{}", p),
                    ReplResult::Loaded(msg) => println!("{} {}", "ok:".green().bold(), msg),
                    ReplResult::Help(h) => println!("{h}"),
                    ReplResult::Error(e) => eprintln!("{} {}", "error:".red().bold(), e),
                    ReplResult::Empty => {}
                }
            }
            Err(rustyline::error::ReadlineError::Interrupted | rustyline::error::ReadlineError::Eof) => break,
            Err(e) => {
                eprintln!("{} {}", "error:".red().bold(), e);
                break;
            }
        }
    }
}
