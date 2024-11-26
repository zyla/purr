use crate::ast::PatKind;
use crate::ast::{Expr, ExprKind};
use fxhash::FxHashSet;
use std::path::PathBuf;

use crate::ast::AbsoluteName;
use crate::renamed_module::DeclId;
use crate::renamed_module::Namespace;
use crate::scc::scc_of;
use crate::scc::SccId;
use crate::typecheck::typechecked_scc;
use std::fmt::Write;

macro_rules! cg_write {
    ($cg:expr, $($arg:tt)*) => {
        write!(HasCodeBuffer::code_buffer($cg), $($arg)*).expect("formatting to a String shouldn't fail")
    };
}

/// Different ways to reference `AbsoluteName`s.
#[derive(PartialEq, Eq, Hash, Debug, Clone, Copy)]
pub enum ReferenceMode {
    /// Names are imported from their respective modules. `Var(AbsoluteName)` is compiled to
    /// `Some_Module.valueName`.
    ModuleImport,
    /// Names are imported from their respective modules. `Var(AbsoluteName)` is compiled to
    /// `Some_Module__valueName`.
    LocalConstant,
}

#[derive(PartialEq, Eq, Hash, Debug, Clone, Copy)]
pub enum BundleMode {
    /// Bundle is a ES module, exports the entrypoint
    Export,
    /// Bundle is standalone (except possibly imports in FFI), calls the entrypoint
    Main,
    /// Generate code, but don't use it it any way
    None,
}

/// Accumulates code in main source
#[salsa::accumulator]
pub struct CodeAccumulator(pub String);

/// Accumulates FFI code (in separate files)
#[salsa::accumulator]
pub struct FfiAccumulator(pub FfiSourceFile);

#[derive(Debug, Clone)]
pub struct FfiSourceFile {
    pub filename: PathBuf,
    pub contents: String,
}

/// Generate a bundle of code which includes the specified entry point and its dependencies.
/// Depending on `BundleMode`, it's either exported or called as `main()`.
///
/// TODO: support multiple entry points for export mode
pub fn bundle(
    db: &dyn crate::Db,
    bundle_mode: BundleMode,
    entrypoint: AbsoluteName,
) -> (String, Vec<FfiSourceFile>) {
    let mut code = value_decl_code_acc::accumulated::<CodeAccumulator>(db, entrypoint).join("");
    let ffi = value_decl_ffi_code_acc::accumulated::<FfiAccumulator>(db, entrypoint);

    match bundle_mode {
        BundleMode::Export => {
            cg_write!(&mut code, "export default ");
            write_mangled_name(db, ReferenceMode::LocalConstant, &mut code, entrypoint);
            cg_write!(&mut code, ";\n");
        }
        BundleMode::Main => {
            write_mangled_name(db, ReferenceMode::LocalConstant, &mut code, entrypoint);
            cg_write!(&mut code, "();\n");
        }
        BundleMode::None => {}
    }
    (code, ffi)
}

/// Generate code for the given value and its transitive dependencies, collecting it in the
/// `CodeAccumulator` accumulator.
#[salsa::tracked]
pub fn value_decl_code_acc(db: &dyn crate::Db, id: AbsoluteName) {
    scc_code_acc(
        db,
        scc_of(
            db,
            DeclId::new(db, Namespace::Value, id.module(db), id.name(db)),
        ),
    )
}

#[salsa::tracked]
pub fn value_decl_ffi_code_acc(db: &dyn crate::Db, id: AbsoluteName) {
    scc_ffi_code_acc(
        db,
        scc_of(
            db,
            DeclId::new(db, Namespace::Value, id.module(db), id.name(db)),
        ),
    )
}

pub trait HasCodeBuffer {
    fn code_buffer(&mut self) -> &mut String;
}

impl HasCodeBuffer for String {
    fn code_buffer(&mut self) -> &mut String {
        self
    }
}

impl<'d> HasCodeBuffer for CodeGenerator<'d> {
    fn code_buffer(&mut self) -> &mut String {
        &mut self.code_buffer
    }
}

/// Generate code for the given SCC and its transitive dependencies, collecting it in the
/// `CodeAccumulator` accumulator.
#[salsa::tracked]
pub fn scc_code_acc(db: &dyn crate::Db, id: SccId) {
    let mut cg = CodeGenerator::new(db, ReferenceMode::LocalConstant);
    for (id, expr, _) in typechecked_scc(db, id) {
        cg_write!(
            &mut cg,
            "const {}__{} = ",
            id.module(db).name(db),
            id.name(db).text(db)
        );
        cg.expr(&expr);
        cg_write!(&mut cg, ";\n");
    }
    for dep in cg.dependencies {
        value_decl_code_acc(db, dep);
    }
    CodeAccumulator::push(db, cg.code_buffer);
}

#[salsa::tracked]
pub fn scc_ffi_code_acc(db: &dyn crate::Db, id: SccId) {
    for (id, _, _) in typechecked_scc(db, id) {
        if let Some(ffi) = &db
            .module_source(id.module(db))
            .ffi_contents(db)
            .as_ref()
            .map(|(filename, contents)| FfiSourceFile {
                filename: filename.to_path_buf(),
                contents: contents.to_string(),
            })
        {
            FfiAccumulator::push(db, ffi.clone());
        }
    }
}

struct CodeGenerator<'d> {
    db: &'d dyn crate::Db,
    reference_mode: ReferenceMode,
    code_buffer: String,
    dependencies: FxHashSet<AbsoluteName>,
}

impl<'d> CodeGenerator<'d> {
    fn new(db: &'d dyn crate::Db, reference_mode: ReferenceMode) -> Self {
        Self {
            db,
            reference_mode,
            code_buffer: String::new(),
            dependencies: Default::default(),
        }
    }

    fn expr(&mut self, e: &Expr) {
        let db = self.db;
        use ExprKind::*;

        match &**e {
            Var(v) => match v.to_absolute_name(db) {
                Some(abs) => {
                    self.dependencies.insert(abs);
                    write_mangled_name(db, self.reference_mode, self, abs);
                }
                None => {
                    cg_write!(self, "{}", v.name(db).text(db));
                }
            },
            App(f, xs) => {
                cg_write!(self, "(");
                self.expr(f);
                cg_write!(self, ")(");
                for (i, x) in xs.iter().enumerate() {
                    if i > 0 {
                        cg_write!(self, ", ");
                    }
                    self.expr(x);
                }
                cg_write!(self, ")");
            }
            Lam(pats, body) => {
                cg_write!(self, "(");
                for (i, p) in pats.iter().enumerate() {
                    if i > 0 {
                        cg_write!(self, ", ");
                    }
                    match &**p {
                        PatKind::Var(v) => {
                            cg_write!(self, "{}", v.text(db));
                        }
                        _ => panic!("invalid desugared lambda pattern {:?}", p),
                    }
                }
                cg_write!(self, ") => ");
                self.expr(body);
            }
            Literal(crate::ast::Literal::Integer(x)) => {
                cg_write!(self, "{}", x);
            }
            Literal(crate::ast::Literal::String(x)) => {
                cg_write!(self, "{:?}", x.to_string_lossy());
            }
            Error => {
                cg_write!(
                    self,
                    "((() => {{ throw new Error('Reached broken code') }})())",
                );
            }
            _ => todo!("codegen: unsupported expr {:?}", e),
        }
    }
}

fn write_mangled_name<G: HasCodeBuffer>(
    db: &dyn crate::Db,
    reference_mode: ReferenceMode,
    g: &mut G,
    name: AbsoluteName,
) {
    match reference_mode {
        ReferenceMode::ModuleImport => {
            cg_write!(g, "{}.{}", name.module(db).name(db), name.name(db).text(db));
        }
        ReferenceMode::LocalConstant => {
            cg_write!(
                g,
                "{}__{}",
                name.module(db).name(db),
                name.name(db).text(db)
            );
        }
    }
}

/// Generate code for the given SCC without its dependencies. For use in multi-module code
/// generation mode.
#[salsa::tracked]
pub fn scc_code(db: &dyn crate::Db, id: SccId) -> String {
    let mut cg = CodeGenerator::new(db, ReferenceMode::ModuleImport);
    for (id, expr, _) in typechecked_scc(db, id) {
        cg_write!(&mut cg, "const {} = ", id.name(db).text(db));
        cg.expr(&expr);
        cg_write!(&mut cg, ";\n");
    }
    cg.code_buffer
}

#[cfg(test)]
mod bundle_tests {
    use super::*;
    use crate::symbol::Symbol;
    use crate::utils::tests::dummy_module;
    use crate::ModuleId;
    use indoc::indoc;
    use insta::*;

    fn test_bundle_mode(
        inputs: &[&str],
        entrypoint: (&str, &str),
        bundle_mode: BundleMode,
    ) -> String {
        let _ = env_logger::builder().is_test(true).try_init();
        let db = &mut crate::Database::new();

        for (source, i) in inputs.iter().zip(1..) {
            db.add_source_file(format!("Lib{}.purs", i).into(), source.to_string())
                .unwrap();
        }

        let entrypoint = AbsoluteName::new(
            db,
            ModuleId::new(db, entrypoint.0.to_string()),
            Symbol::new(db, entrypoint.1.to_string()),
        );
        let (code, _ffi) = bundle(db, bundle_mode, entrypoint);
        code
    }

    fn test_bundle(inputs: &[&str], entrypoint: (&str, &str)) -> String {
        test_bundle_mode(inputs, entrypoint, BundleMode::None)
    }

    fn codegen_expr(expr_str: &str) -> String {
        let _ = env_logger::builder().is_test(true).try_init();
        let db = &mut crate::Database::new();
        let mut g = CodeGenerator::new(db, ReferenceMode::LocalConstant);
        let expr = crate::parser::parse_expr(db, expr_str, dummy_module(db))
            .1
            .unwrap();
        g.expr(&expr);
        g.code_buffer
    }

    #[test]
    fn simple1() {
        assert_snapshot!(test_bundle(
            &[indoc!(
                r"
                module Test where
                foo :: Int
                foo = 1
                "
            )],
            ("Test", "foo")
        ));
    }

    #[test]
    fn simple_dep() {
        assert_snapshot!(test_bundle(
            &[indoc!(
                r"
                module Test where
                unused :: Int
                unused = 1
                foo :: Int
                foo = answer
                answer :: Int
                answer = 42
                "
            )],
            ("Test", "foo")
        ));
    }

    #[test]
    fn transitive_dep() {
        assert_snapshot!(test_bundle(
            &[indoc!(
                r"
                module Test where
                foo :: Int
                foo = bar
                bar :: Int
                bar = baz
                baz :: Int
                baz = 42
                "
            )],
            ("Test", "foo")
        ));
    }

    #[test]
    fn function_call() {
        assert_snapshot!(test_bundle(
            &[indoc!(
                r"
                module Test where
                foo :: Int
                foo = frob answer
                answer :: Int
                answer = 42
                frob :: Int -> Int
                -- note: explicit lambda because no equations desugaring yet
                frob = \x -> x
                "
            )],
            ("Test", "foo")
        ));
    }

    #[test]
    fn bundle_main() {
        assert_snapshot!(test_bundle_mode(
            &[indoc!(
                r"
                module Test where
                foo :: Int -> Int
                foo = \x -> x
                "
            )],
            ("Test", "foo"),
            BundleMode::Main
        ));
    }

    #[test]
    fn bundle_export() {
        assert_snapshot!(test_bundle_mode(
            &[indoc!(
                r"
                module Test where
                foo :: Int -> Int
                foo = \x -> x
                "
            )],
            ("Test", "foo"),
            BundleMode::Export
        ));
    }

    #[test]
    fn string_literal() {
        assert_snapshot!(codegen_expr(
            "\"foo\""
        ), @r###""foo""###);
    }

    #[test]
    #[ignore = "codegen currently loses data for invalid unicode characters; also we seem to parse it wrong"]
    fn string_literal_invalid_unicode() {
        assert_snapshot!(codegen_expr(
            r###""\u110000""###
        ), @r###""\u110000""###);
    }
}
