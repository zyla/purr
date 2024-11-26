use crate::prim::PRIM_SOURCE;
use dashmap::{mapref::entry::Entry, DashMap};
use derive_new::new;
use salsa::ParallelDatabase;
use source_span::{SourceSpan, SourceSpanOps, SpanDeclRef, ToSourceSpan};
use std::path::PathBuf;
use std::sync::Arc;
use thiserror::Error;

use crate::ast::declarations;
use crate::errors::Error;

#[macro_use]
extern crate lalrpop_util;

#[salsa::jar(db = Db)]
pub struct Jar(
    crate::ModuleSource,
    crate::ModuleId,
    crate::parsed_module,
    crate::renamed_module::renamed_module,
    crate::indexed_module::indexed_module,
    crate::renamed_module::exported_decls,
    crate::symbol::Symbol,
    crate::Diagnostics,
    crate::ast::QualifiedName,
    crate::ast::AbsoluteName,
    crate::renamed_module::DeclId,
    crate::scc::scc_of,
    crate::scc::decls_in_scc,
    crate::scc::SccId,
    crate::renamed_module::renamed_value_decl,
    crate::typecheck::typechecked_scc,
    crate::typecheck::type_of_value,
    crate::typecheck::typecheck_module,
    crate::codegen::value_decl_code_acc,
    crate::codegen::value_decl_ffi_code_acc,
    crate::codegen::scc_ffi_code_acc,
    crate::codegen::scc_code_acc,
    crate::codegen::scc_code,
    crate::codegen::CodeAccumulator,
    crate::codegen::FfiAccumulator,
);

#[salsa::input]
pub struct ModuleSource {
    id: ModuleId,
    #[return_ref]
    contents: Option<(PathBuf, String)>,
    #[return_ref]
    ffi_contents: Option<(PathBuf, String)>,
}

#[salsa::accumulator]
pub struct Diagnostics(Diagnostic);

#[derive(Clone, Debug, new, PartialEq, Eq)]
pub struct Diagnostic {
    pub span: SourceSpan,
    pub message: String,
}

#[derive(Clone, Debug, new, PartialEq, Eq)]
pub struct DiagnosticFmt {
    pub span: SourceSpan,
    pub message: String,
    pub filename: PathBuf,
    pub source: String,
    pub module_name: String,
}

impl std::fmt::Display for DiagnosticFmt {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let line_number = self.span.to_line_column(&self.source);
        let line = self.span.extract_line(&self.source);

        let offset: String = " ".repeat(line_number.1);

        let message = format!("{}[33m^ {}[0m", offset, self.message);

        write!(
            f,
            "Error found:\n in module [33m{}[0m\n at {}:{}:{}\n\n {}\n {}",
            self.module_name,
            self.filename.to_string_lossy(),
            line_number.0,
            line_number.1,
            line,
            message
        )
    }
}

impl Diagnostic {
    pub fn pp(&mut self, db: &dyn Db) -> DiagnosticFmt {
        match self.span.decl {
            SpanDeclRef::Module(module) => {
                let (filename, source) = &db
                    .module_source(module)
                    .contents(db)
                    .as_ref()
                    .unwrap_or_else(|| panic!("module not found: {:?}", module.name(db)));

                DiagnosticFmt::new(
                    self.span,
                    self.message.to_string(),
                    filename.to_path_buf(),
                    source.clone(),
                    module.name(db),
                )
            }
            SpanDeclRef::Decl(decl_id) => {
                let (filename, source) = &db
                    .module_source(decl_id.module(db))
                    .contents(db)
                    .as_ref()
                    .unwrap_or_else(|| {
                        panic!("module not found: {:?}", decl_id.module(db).name(db))
                    });

                let indexed = crate::indexed_module::indexed_module(db, decl_id.module(db));
                let ref_loc = indexed.decls_ref_loc.get(&decl_id).unwrap();

                DiagnosticFmt::new(
                    *self.span.to_absolute_span(decl_id.module(db), *ref_loc),
                    self.message.to_string(),
                    filename.to_path_buf(),
                    source.clone(),
                    decl_id.module(db).name(db),
                )
            }
            SpanDeclRef::Unknown => DiagnosticFmt::new(
                self.span,
                self.message.to_string(),
                String::new().into(),
                String::new(),
                "UnknownModule".to_string(),
            ),
        }
    }
}

pub trait ToDiagnostic {
    fn to_diagnostic(&self, module_id: ModuleId) -> Diagnostic;
}

impl<T: std::fmt::Debug, E: ToDiagnostic> ToDiagnostic
    for lalrpop_util::ErrorRecovery<usize, T, E>
{
    fn to_diagnostic(&self, module_id: ModuleId) -> Diagnostic {
        self.error.to_diagnostic(module_id)
    }
}

impl ToDiagnostic for Error {
    fn to_diagnostic(&self, module_id: ModuleId) -> Diagnostic {
        let source_span = SourceSpan::new_in_module(self.loc.start, self.loc.end, module_id);
        Diagnostic::new(source_span, format!("{}", self.kind))
    }
}

impl ToDiagnostic for &str {
    fn to_diagnostic(&self, module_id: ModuleId) -> Diagnostic {
        let source_span = SourceSpan::new_in_module(0, 0, module_id);
        Diagnostic::new(source_span, self.to_string())
    }
}

impl<T: std::fmt::Debug, E: ToDiagnostic> ToDiagnostic for lalrpop_util::ParseError<usize, T, E> {
    fn to_diagnostic(&self, module_id: ModuleId) -> Diagnostic {
        use lalrpop_util::ParseError::*;

        match self {
            InvalidToken { location } => {
                let source_span = SourceSpan::new_in_module(*location, *location, module_id);
                Diagnostic::new(source_span, "invalid token".into())
            }

            UnrecognizedEOF { location, .. } => {
                let source_span = SourceSpan::new_in_module(*location, *location, module_id);
                Diagnostic::new(source_span, "unexpected eof".into())
            }

            UnrecognizedToken {
                token: (start, token, end),
                ..
            } => {
                let source_span = SourceSpan::new_in_module(*start, *end, module_id);
                Diagnostic::new(source_span, format!("unrecognized token {:?}", token))
            }
            ExtraToken {
                token: (start, token, end),
            } => {
                let source_span = SourceSpan::new_in_module(*start, *end, module_id);
                Diagnostic::new(source_span, format!("extra token {:?}", token))
            }
            User { error } => error.to_diagnostic(module_id),
        }
    }
}

impl<T, E> ToSourceSpan for lalrpop_util::ParseError<usize, T, E> {
    fn to_source_span(&self, module_id: ModuleId) -> SourceSpan {
        use lalrpop_util::ParseError::*;

        match *self {
            InvalidToken { location } => SourceSpan::new_in_module(location, location, module_id),
            UnrecognizedEOF { location, .. } => {
                SourceSpan::new_in_module(location, location, module_id)
            }

            UnrecognizedToken {
                token: (start, .., end),
                ..
            } => SourceSpan::new_in_module(start, end, module_id),
            ExtraToken {
                token: (start, .., end),
            } => SourceSpan::new_in_module(start, end, module_id),
            User { .. } => SourceSpan::new_in_module(0, 0, module_id),
        }
    }
}

#[salsa::interned]
pub struct ModuleId {
    pub name: String,
}

#[derive(PartialEq, Eq, Debug, Clone)]
pub struct ParsedModule {
    pub filename: PathBuf,
    pub ast: crate::ast::Module,
}

#[salsa::tracked]
pub fn parsed_module(db: &dyn Db, module: ModuleId) -> ParsedModule {
    let (filename, input) = &db
        .module_source(module)
        .contents(db)
        .as_ref()
        .unwrap_or_else(|| panic!("module not found: {:?}", module.name(db)));
    let (errs, result) = crate::parser::parse_module(db, input, module);
    errs.iter().for_each(|err| {
        Diagnostics::push(db, err.to_diagnostic(module));
    });

    ParsedModule {
        filename: filename.clone(),
        ast: match result {
            Err(err) => {
                Diagnostics::push(db, err.to_diagnostic(module));

                declarations::corrupted(db, err.to_source_span(module))
            }
            Ok(module) => module,
        },
    }
}

pub trait Db: salsa::DbWithJar<Jar> {
    fn module_source(&self, module: ModuleId) -> ModuleSource;
}

#[salsa::db(Jar)]
pub struct Database {
    storage: salsa::Storage<Self>,
    module_sources: Arc<DashMap<String, ModuleSource>>,
}

#[derive(Debug, Error)]
#[error("module name not specified")]
pub struct ModuleNameNotSpecified;

impl Default for Database {
    fn default() -> Self {
        Self::new()
    }
}

impl Database {
    pub fn new() -> Self {
        let storage = Default::default();
        let mut db = Self {
            storage,
            module_sources: Arc::new(DashMap::new()),
        };
        db.add_source_file("<builtin>/Prim.purs".into(), PRIM_SOURCE.into())
            .expect("Prim should have a module name");
        db
    }

    pub fn new_with_prelude() -> Self {
        let mut db = Self::new();

        let prelude_path = std::env::current_dir()
            .expect("Error getting current directory")
            .join("prelude/src");

        if !prelude_path.exists() {
            eprintln!("Warning: Prelude path does not exist: {:?}", prelude_path);
            return db;
        }

        use walkdir::{DirEntry, WalkDir};
        let files: Vec<_> = WalkDir::new(&prelude_path)
            .into_iter()
            .collect::<Result<Vec<_>, _>>()
            .map(|entries| {
                entries
                    .into_iter()
                    .filter(is_purs)
                    .filter_map(|e| {
                        let path = e.into_path();
                        if path.is_file() {
                            Some(path)
                        } else {
                            None
                        }
                    })
                    .collect()
            })
            .unwrap_or_else(|e| {
                eprintln!("Error loading prelude source files: {}", e);

                Vec::new()
            });

        crate::utils::load_files(&mut db, files);

        fn is_purs(entry: &DirEntry) -> bool {
            entry
                .file_name()
                .to_str()
                .map(|s| s.ends_with(".purs"))
                .unwrap_or(false)
        }

        db
    }

    pub fn add_source_file(
        &mut self,
        filename: PathBuf,
        contents: String,
    ) -> Result<ModuleId, ModuleNameNotSpecified> {
        self.add_source_file_with_ffi(filename, contents, None)
    }

    pub fn add_source_file_with_ffi(
        &mut self,
        filename: PathBuf,
        contents: String,
        ffi_contents: Option<(PathBuf, String)>,
    ) -> Result<ModuleId, ModuleNameNotSpecified> {
        if let Some(module_name) = parser::parse_module_name(&contents) {
            let module_id = ModuleId::new(self, module_name.clone());
            let source_file = self.module_source(module_id);
            source_file
                .set_contents(self)
                .to(Some((filename, contents)));
            source_file.set_ffi_contents(self).to(ffi_contents);
            Ok(module_id)
        } else {
            Err(ModuleNameNotSpecified)
        }
    }

    pub fn module_ids(&self) -> Vec<ModuleId> {
        self.module_sources
            .iter()
            .map(|x| ModuleId::new(self, x.key().clone()))
            .collect()
    }

    #[cfg(test)]
    pub fn test_single_file_db(contents: &str) -> Self {
        let mut db = Self::new();
        db.add_source_file("test.purs".into(), contents.into())
            .unwrap();
        db
    }
}

impl Db for Database {
    fn module_source(&self, module: ModuleId) -> ModuleSource {
        match self.module_sources.entry(module.name(self)) {
            Entry::Occupied(entry) => *entry.get(),
            Entry::Vacant(entry) => *entry.insert(ModuleSource::new(self, module, None, None)),
        }
    }
}

impl salsa::Database for Database {}

impl ParallelDatabase for Database {
    fn snapshot(&self) -> salsa::Snapshot<Self> {
        salsa::Snapshot::new(Self {
            storage: self.storage.snapshot(),
            module_sources: self.module_sources.clone(),
        })
    }
}

pub struct DbSnapshot(pub salsa::Snapshot<Database>);

impl Clone for DbSnapshot {
    fn clone(&self) -> Self {
        Self(self.0.snapshot())
    }
}

pub mod ast;
pub mod codegen;
pub mod errors;
pub mod indexed_module;
pub mod lexer;
pub mod parser;
pub mod pretty_printer;
pub mod prim;
pub mod rename;
pub mod renamed_module;
pub mod scc;
pub mod source_span;
pub mod string;
pub mod symbol;
pub mod token;
pub mod typecheck;
pub mod utils;
