use crate::indexed_module::ValueDecl;
use crate::symbol::Symbol;
use crate::Db;
use std::collections::{HashMap, HashSet};

use crate::ast::*;
use crate::indexed_module::IndexedModule;
use crate::renamed_module::DeclId;
use crate::ModuleId;

pub fn rename_module(
    db: &dyn Db,
    module: &mut IndexedModule,
    imported_decls: Vec<(Option<ModuleId>, DeclId)>,
) {
    let module_scope = imported_decls
        .iter()
        .map(|(qualified_as, id)| {
            (
                QualifiedName::new(db, *qualified_as, id.name),
                AbsoluteName::new(db, id.module, id.name),
            )
        })
        .collect::<HashMap<_, _>>();
    let mut r = Renamer {
        db,
        module_scope,
        local_scopes: vec![HashSet::new()],
    };
    module.rename(&mut r);
}

struct Renamer<'db> {
    db: &'db dyn Db,
    /// Maps from names as appear in source code to actual absolute names
    module_scope: HashMap<QualifiedName, AbsoluteName>,
    local_scopes: Vec<HashSet<Symbol>>,
}

impl<'db> Renamer<'db> {
    fn push_scope(&mut self) {
        self.local_scopes.push(HashSet::new());
    }

    fn pop_scope(&mut self) {
        let scope = self.local_scopes.pop();
        assert!(scope.is_some(), "pop_scope called when there are no scopes");
    }

    fn top_scope(&mut self) -> &mut HashSet<Symbol> {
        self.local_scopes
            .last_mut()
            .expect("top_scope called when there are no scopes")
    }
}

trait Rename {
    fn rename(&mut self, r: &mut Renamer);
}

impl<T> Rename for Option<T>
where
    T: Rename,
{
    fn rename(&mut self, r: &mut Renamer) {
        match self {
            None => {}
            Some(x) => x.rename(r),
        }
    }
}

impl<T> Rename for Located<T>
where
    T: Rename,
{
    fn rename(&mut self, r: &mut Renamer) {
        self.1.rename(r);
    }
}

impl Rename for IndexedModule {
    fn rename(&mut self, r: &mut Renamer) {
        for (_, ref mut v) in self.values.clone() {
            v.rename(r);
        }
        // TODO: self.types
        // TODO: self.classes
    }
}

impl Rename for ValueDecl {
    fn rename(&mut self, r: &mut Renamer) {
        self.type_.rename(r);
        for ref mut x in self.equations.clone() {
            x.rename(r);
        }
    }
}

impl Rename for Type {
    fn rename(&mut self, _r: &mut Renamer) {
        // TODO
    }
}

impl Rename for CaseBranch {
    fn rename(&mut self, r: &mut Renamer) {
        r.push_scope();
        for ref mut pat in self.pats.clone() {
            pat.rename(r);
        }
        self.expr.rename(r);
        r.pop_scope();
    }
}

impl Rename for PossiblyGuardedExpr {
    fn rename(&mut self, r: &mut Renamer) {
        match self {
            Self::Unconditional(ref mut e) => e.rename(r),
            Self::Guarded(_) => todo!("Pattern guards not implemented"),
        }
    }
}

impl Rename for PatKind {
    fn rename(&mut self, r: &mut Renamer) {
        match self {
            Self::Var(v) => {
                if !r.top_scope().insert(*v) {
                    todo!("duplicate variable in pattern, TODO: report error");
                }
            }
            _ => todo!("renaming PatKind {:?} not supported", self),
        }
    }
}

impl Rename for ExprKind {
    fn rename(&mut self, r: &mut Renamer) {
        match self {
            Self::Var(ref mut v) => {
                let db = r.db;
                let local_vars = r.top_scope();
                let is_local = v.module(db).is_none() && local_vars.contains(&v.name(db));
                if !is_local {
                    match r.module_scope.get(&v) {
                        None => todo!("report error: unknown variable {v:?}"),
                        Some(abs) => *v = abs.to_qualified_name(db),
                    }
                }
            }
            Self::Lam(ref mut pats, ref mut expr) => {
                r.push_scope();
                for ref mut pat in pats {
                    pat.rename(r);
                }
                expr.rename(r);
                r.pop_scope();
            }
            _ => todo!("renaming ExprKind {:?} not supported", self),
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use indoc::indoc;
    use insta::{self, assert_snapshot};
    use salsa::DebugWithDb;

    fn rename(input: &str) -> String {
        let db = &mut crate::Database::test_single_file_db(input);
        let module_id = ModuleId::new(db, "Test".into());

        let mut module = crate::indexed_module::indexed_module(db, module_id);
        let imported = crate::renamed_module::imported_decls(db, module_id);

        rename_module(db, &mut module, imported);

        format!("{:#?}", module.into_debug_all(db))
    }

    #[test]
    fn smoke() {
        assert_snapshot!(rename(indoc!(
            "
        module Test where
        
        f a = a 
        "
        )))
    }
}

