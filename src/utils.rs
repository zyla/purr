use anyhow::Context;
use std::path::PathBuf;
use std::{fs::File, io::Read};

pub fn load_files(db: &mut crate::Database, files: Vec<PathBuf>) {
    for filename in files {
        if let Err(err) = process_file(db, filename.clone()) {
            println!("FAIL {} error: {}", filename.to_string_lossy(), err);
        }
    }
}

pub fn process_file(db: &mut crate::Database, filename: PathBuf) -> anyhow::Result<()> {
    let mut contents = String::new();

    File::open(&filename)
        .with_context(|| format!("Failed to open file: {:?}", filename))?
        .read_to_string(&mut contents)
        .with_context(|| format!("Failed to read file: {:?}", filename))?;

    let mut ffi_filename = filename.clone();
    ffi_filename.set_extension("js");

    let ffi_contents = if let Ok(mut ffi_file) = File::open(&ffi_filename) {
        let mut ffi_contents = String::new();
        ffi_file
            .read_to_string(&mut ffi_contents)
            .with_context(|| format!("Failed to read ffi file: {:?}", ffi_filename))?;
        Some((ffi_filename, ffi_contents))
    } else {
        None
    };

    db.add_source_file_with_ffi(filename, contents, ffi_contents)
        .map_err(|_| anyhow::format_err!("Invalid module name"))?;
    Ok(())
}

pub mod tests {
    pub trait DropSalsaId {
        fn drop_salsa_id(&mut self) -> Self;
    }

    impl DropSalsaId for String {
        fn drop_salsa_id(&mut self) -> String {
            self.lines()
                .filter(|l| !l.contains("[salsa id]"))
                .collect::<Vec<_>>()
                .join("\n")
        }
    }

    pub fn parse_module_id(input: &str, db: &dyn crate::Db) -> crate::ModuleId {
        crate::parser::parse_module_name(input)
            .map(|name| crate::ModuleId::new(db, name))
            .expect("The input should be valid purescript module")
    }

    pub fn dummy_module(db: &dyn crate::Db) -> crate::ModuleId {
        crate::ModuleId::new(db, "Test".into())
    }
}
