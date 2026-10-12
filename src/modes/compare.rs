//! `pepe compare`: two reports, and what moved between them.

use super::*;

/// `pepe compare before.json after.json`: what moved, as a report or as
/// JSON; with --gate, exit 1 when it is a regression
pub(crate) fn run_compare(
    args: &Cli,
    what: &cli::CompareArgs,
) -> Result<(), Box<dyn std::error::Error>> {
    // Two reports, or a saved test's last two recorded runs
    let (before, after) = match &what.after {
        Some(after) => (what.before.clone(), after.clone()),
        None => {
            let name = what.before.to_string_lossy().to_string();
            let store = crate::store::Store::local();
            if let Err(e) = store.load(&name) {
                eprintln!("error: compare takes two reports, or the name of a saved test: {e}");
                std::process::exit(2);
            }
            let runs = store.runs(&name);
            if runs.len() < 2 {
                eprintln!(
                    "error: `{name}` has {} recorded run{}; two are needed (pepe --load {name} --record)",
                    runs.len(),
                    if runs.len() == 1 { "" } else { "s" }
                );
                std::process::exit(2);
            }
            (
                runs[runs.len() - 2].path.clone(),
                runs[runs.len() - 1].path.clone(),
            )
        }
    };
    let sides = (
        crate::compare::Side::read(&before),
        crate::compare::Side::read(&after),
    );
    let (before, after) = match sides {
        (Ok(before), Ok(after)) => (before, after),
        (Err(e), _) | (_, Err(e)) => {
            eprintln!("error: {e}");
            std::process::exit(2);
        }
    };
    let comparison = crate::compare::compare(&before, &after);
    if let Some(path) = &what.svg {
        std::fs::write(path, comparison.svg())
            .map_err(|e| format!("can't write {}: {e}", path.display()))?;
    }
    if args.json {
        println!("{}", comparison.to_json()?);
    } else {
        print_report(&comparison.report());
    }
    if what.gate && comparison.regression {
        std::process::exit(1);
    }
    Ok(())
}
