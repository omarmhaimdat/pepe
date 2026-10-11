//! `pepe compare`: two reports, and what moved between them.

use super::*;

/// `pepe compare before.json after.json`: what moved, as a report or as
/// JSON; with --gate, exit 1 when it is a regression
pub(crate) fn run_compare(
    args: &Cli,
    what: &cli::CompareArgs,
) -> Result<(), Box<dyn std::error::Error>> {
    let sides = (
        crate::compare::Side::read(&what.before),
        crate::compare::Side::read(&what.after),
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
