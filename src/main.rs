use clap::Parser;
use purs_oxide::codegen::BundleMode;
use purs_oxide::{Diagnostic, Diagnostics};
use rayon::prelude::*;
use salsa::ParallelDatabase;
use std::fmt::Write;
use std::path::PathBuf;

#[derive(clap::Parser)]
#[command(version, about)]
enum Command {
    Parse {
        #[arg(value_name = "FILES", help = "The input .purs file(s).")]
        files: Vec<PathBuf>,
    },
    Bundle {
        #[arg(value_name = "FILES", help = "The input .purs file(s)")]
        files: Vec<PathBuf>,
        #[arg(
            short,
            long,
            value_name = "MODULE",
            help = "Specify the qualified entrypoint function (e.g., App.main)"
        )]
        entrypoint: String,
    },
}

fn main() -> std::io::Result<()> {
    let mut db = purs_oxide::Database::new_with_prelude();
    match Command::parse() {
        Command::Parse { files } => {
            purs_oxide::utils::load_files(&mut db, files);
            let db = db.snapshot();
            db.module_ids().par_iter().for_each_with(
                purs_oxide::DbSnapshot(db.snapshot()),
                |db, module| {
                    let db: &dyn purs_oxide::Db = &*db.0;
                    let parsed_module = purs_oxide::parsed_module(db, *module);
                    let accumulated: Vec<Diagnostic> =
                        purs_oxide::parsed_module::accumulated::<Diagnostics>(db, *module);

                    println!(
                        "parsed {}, {} declarations, diagnostics: {}",
                        module.name(db),
                        parsed_module.ast.declarations.len(),
                        accumulated
                            .into_iter()
                            .fold(String::new(), |mut output, d| {
                                write!(output, "\n - {:?}", d).expect("write to String");
                                output
                            })
                    );
                },
            );
        }
        Command::Bundle { files, entrypoint } => {
            purs_oxide::utils::load_files(&mut db, files);

            //let (err, entry) = purs_oxide::parser::parse_lower_qualified_ident(&db, &entrypoint);

            let entrypoint = match purs_oxide::parser::parse_lower_qualified_ident(&db, &entrypoint)
                .1
                .unwrap()
                .to_absolute_name(&db)
            {
                Some(abs) => abs,
                None => panic!("invalid entrypoint {}", entrypoint),
            };
            let (code, _ffi) = purs_oxide::codegen::bundle(&db, BundleMode::Export, entrypoint);
            println!("{}", code);
        }
    }
    Ok(())
}
