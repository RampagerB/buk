use std::process;

use structopt::StructOpt;

use buk::{backup_root, Opt};

fn main() {
    let opt = Opt::from_args();
    let dest = match backup_root() {
        Ok(d) => d,
        Err(e) => {
            eprintln!("error: {e}");
            process::exit(2);
        }
    };
    if let Err(e) = buk::run(&opt, &dest) {
        eprintln!("error: {e}");
        process::exit(1);
    }
}
