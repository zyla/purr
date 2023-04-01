use crate::ast::Literal;
use crate::ast::Located;
use crate::ast::Pat;
use crate::ast::PatKind;
use crate::ast::TypeParameter;
use crate::ast::{Expr, ExprKind, Module, Type};
use crate::ast::{QualifiedName, TypeKind};
use crate::lexer;
use crate::symbol::Symbol;
use crate::token::Token;
use lalrpop_util::ErrorRecovery;
use lalrpop_util::ParseError;

lalrpop_mod!(pub parser);

pub(self) fn constraint_to_instance_head(c: Type) -> Option<(QualifiedName, Vec<Type>)> {
    let mut t = c;
    let mut args = vec![];
    loop {
        match t.into_inner() {
            TypeKind::TypeConstructor(con) => {
                args.reverse();
                return Some((con, args));
            }
            TypeKind::TypeApp(f, x) => {
                args.push(*x);
                t = *f;
            }
            _ => return None,
        }
    }
}

pub(self) fn constraint_to_class_head(c: Type) -> Option<(Symbol, Vec<TypeParameter>)> {
    let mut t = c;
    let mut params = vec![];
    loop {
        match t.into_inner() {
            TypeKind::TypeConstructor(con) => {
                if con.is_actually_qualified() {
                    return None;
                }
                params.reverse();
                return Some((con.0, params));
            }
            TypeKind::TypeApp(f, x) => match *x {
                Located(_, TypeKind::Var(v)) => {
                    params.push((v, None));
                    t = *f;
                }
                // TODO: handle kinded types
                _ => return None,
            },
            _ => return None,
        }
    }
}

pub(self) fn apply_record_updates(f: Expr, args: Vec<Expr>) -> ExprKind {
    let mut result = vec![f];
    for expr in args {
        match expr {
            Located(suffix_span, ExprKind::RecordUpdateSuffix(update)) => {
                let last = result.pop().expect("should be non-empty");
                result.push(Located(
                    suffix_span,
                    ExprKind::RecordUpdate(Box::new(last), update),
                ));
            }
            _ => result.push(expr),
        }
    }
    let f = result.remove(0);
    ExprKind::App(Box::new(f), result)
}

pub(self) fn expr_to_pat(expr: Expr) -> Result<Pat, String> {
    let Located(span, kind) = expr;
    Ok(Located(
        span,
        match kind {
            ExprKind::Literal(lit) => PatKind::Literal(lit_expr_to_pat(lit)?),
            ExprKind::Infix(x, xs) => PatKind::Infix(
                Box::new(expr_to_pat(*x)?),
                xs.into_iter()
                    .map(|(k, x)| Ok::<_, String>((k, expr_to_pat(x)?)))
                    .collect::<Result<_, _>>()?,
            ),
            ExprKind::Accessor(_, _) => return Err("Illegal record accessor in pattern".into()),
            ExprKind::RecordUpdate(x, xs) => PatKind::Infix(
                Box::new(expr_to_pat(*x)?),
                xs.into_iter()
                    .map(|(k, x)| Ok::<_, String>((k, expr_to_pat(x)?)))
                    .collect::<Result<_, _>>()?,
            ),
            ExprKind::Var(name) => {
                if name.is_actually_qualified() {
                    return Err("Illegal qualified name in pattern".into());
                } else {
                    PatKind::Var(name.0)
                }
            }
            ExprKind::DataConstructor(name) => PatKind::DataConstructorApp(name, vec![]),
            ExprKind::App(f, args) => match f.into_inner() {
                ExprKind::DataConstructor(name) => PatKind::DataConstructorApp(
                    name,
                    args.into_iter()
                        .map(|x| expr_to_pat(x))
                        .collect::<Result<_, _>>()?,
                ),
                _ => return Err("illegal pattern in data constructor position".into()),
            },
            ExprKind::Lam(_, _) => return Err("Illegal lambda in pattern".into()),
            ExprKind::Case { .. } => return Err("Illegal case in pattern".into()),
            ExprKind::If { .. } => return Err("Illegal if in pattern".into()),
            ExprKind::Typed(x, ty) => PatKind::Typed(Box::new(expr_to_pat(*x)?), ty),
            ExprKind::Let { .. } => return Err("Illegal let in pattern".into()),
            ExprKind::Wildcard => PatKind::Wildcard,
            ExprKind::RecordUpdateSuffix(_) => {
                return Err("Illegal record update in pattern".into())
            }
            ExprKind::Do(_) => return Err("Illegal do in pattern".into()),
            ExprKind::NamedPat(name, x) => PatKind::Named(name, Box::new(expr_to_pat(*x)?)),
        },
    ))
}

pub(self) fn lit_expr_to_pat(lit: Literal<Expr>) -> Result<Literal<Pat>, String> {
    Ok(match lit {
        Literal::Integer(x) => Literal::Integer(x),
        Literal::Float(x) => Literal::Float(x),
        Literal::String(x) => Literal::String(x),
        Literal::Char(x) => Literal::Char(x),
        Literal::Boolean(x) => Literal::Boolean(x),
        Literal::Array(xs) => Literal::Array(
            xs.into_iter()
                .map(|x| expr_to_pat(x))
                .collect::<Result<_, _>>()?,
        ),
        Literal::Object(xs) => Literal::Object(
            xs.into_iter()
                .map(|(k, x)| Ok::<_, String>((k, expr_to_pat(x)?)))
                .collect::<Result<_, _>>()?,
        ),
    })
}

pub(self) fn normalize_app(f: Expr, x: Expr) -> ExprKind {
    match f {
        Located(_, ExprKind::App(f0, mut args)) => {
            args.push(x);
            ExprKind::App(f0, args)
        }
        _ => ExprKind::App(Box::new(f), vec![x]),
    }
}

type ParseResult<'a, T> = (
    Vec<ErrorRecovery<usize, Token, &'a str>>,
    Result<T, ParseError<usize, Token, lexer::Error>>,
);

pub fn parse_module(input: &str) -> ParseResult<Module> {
    let mut errors = vec![];
    let lexer = lexer::lex(input);
    let result = parser::ModuleParser::new().parse(&mut errors, lexer);
    (errors, result)
}

pub fn parse_type(input: &str) -> ParseResult<Type> {
    let mut errors = vec![];
    let lexer = lexer::lex(input);
    let result = parser::TypeParser::new().parse(&mut errors, lexer);
    (errors, result)
}

pub fn parse_expr(input: &str) -> ParseResult<Expr> {
    let mut errors = vec![];
    let lexer = lexer::lex(input);
    let result = parser::ExprParser::new().parse(&mut errors, lexer);
    (errors, result)
}

#[cfg(test)]
mod tests {
    use indoc::indoc;
    use insta::{self, assert_debug_snapshot};

    fn expect_success<T>(output: super::ParseResult<T>) -> T {
        let (errors, result) = output;
        assert_eq!(errors, &[]);
        result.unwrap()
    }

    fn parse_module(input: &str) -> crate::ast::Module {
        expect_success(super::parse_module(input))
    }
    fn parse_type(input: &str) -> crate::ast::Type {
        expect_success(super::parse_type(input))
    }
    fn parse_expr(input: &str) -> crate::ast::Expr {
        expect_success(super::parse_expr(input))
    }

    #[test]
    fn test_module_header() {
        assert_debug_snapshot!(parse_module(indoc!(
            "
        module Foo where
        "
        )));
    }

    #[test]
    fn test_module_header_qualified() {
        assert_debug_snapshot!(parse_module(indoc!(
            "
        module Some.Module where
        "
        )));
    }

    #[test]
    fn test_simple_value_decl() {
        assert_debug_snapshot!(parse_module(indoc!(
            "
        module Foo where
        x = 1
        "
        )));
    }

    #[test]
    fn test_typed_value_decl() {
        assert_debug_snapshot!(parse_module(indoc!(
            "
        module Foo where
        x :: Int
        x = 1
        "
        )));
    }

    #[test]
    fn test_export_list() {
        assert_debug_snapshot!(parse_module(indoc!(
            "
          module Control.Applicative
            ( class Applicative
            , pure
            , module Data.Functor
            , Either
            , Foo(..)
            , Maybe(Just, Nothing)
            , (+~)
            , type (<>)
            ) where

        "
        )));
    }

    #[test]
    fn test_imports() {
        assert_debug_snapshot!(parse_module(indoc!(
            "
          module Test where

          import Foo.Asd
          import Bar.Asd as Baz
          import Qux.Asd (x)
          import Zzz.Asd (y, z) as Yyy
          import Aaa.Asd hiding (q)

          x = 1

        "
        )));
    }

    #[test]
    fn test_indented_where() {
        assert_debug_snapshot!(parse_module(indoc!(
            "
            module Control.Applicative
              where
            import Control.Apply
        "
        )));
    }

    #[test]
    fn test_function_with_params() {
        assert_debug_snapshot!(parse_module(indoc!(
            r#"
            module Test where
            f x = 1
            g x y = 1
            h [x, y] = 1
            j {x, y: 1} = 1
            k "foo" = 1
            l 42 = 1
            m (x) = 1
        "#
        )));
    }

    #[test]
    fn test_type_synonym() {
        assert_debug_snapshot!(parse_module(indoc!(
            r#"
            module Test where
            type Foo = Int
            type Bar a = a
            type Baz a b = a
        "#
        )));
    }

    #[test]
    fn test_foreign_import() {
        assert_debug_snapshot!(parse_module(indoc!(
            r#"
            module Test where
            foreign import foo :: Int -> Int
        "#
        )));
    }

    #[test]
    fn test_typeclass_1() {
        assert_debug_snapshot!(parse_module(indoc!(
            r#"
            module Test where
            class Foo a where
              foo :: a -> Bool
              bar :: a
        "#
        )));
    }

    #[test]
    fn test_typeclass_2() {
        assert_debug_snapshot!(parse_module(indoc!(
            r#"
            module Test where
            class Bar a <= Foo a where
              bar :: a
        "#
        )));
    }

    #[test]
    fn test_typeclass_3() {
        assert_debug_snapshot!(parse_module(indoc!(
            r#"
            module Test where
            class (Bar a, Baz b) <= Foo a where
              bar :: a
        "#
        )));
    }

    #[test]
    fn test_typeclass_4() {
        assert_debug_snapshot!(parse_module(indoc!(
            r#"
            module Test where
            class Foo a where
        "#
        )));
    }

    #[test]
    fn test_typeclass_5() {
        assert_debug_snapshot!(parse_module(indoc!(
            r#"
            module Test where
            class Foo a
        "#
        )));
    }

    #[test]
    fn test_instance_1() {
        assert_debug_snapshot!(parse_module(indoc!(
            r#"
            module Test where
            instance Foo Int where
              foo x = 1
              bar = 2
        "#
        )));
    }

    #[test]
    fn test_instance_2() {
        assert_debug_snapshot!(parse_module(indoc!(
            r#"
            module Test where
            instance Bar a => Foo a where
              bar = 1
        "#
        )));
    }

    #[test]
    fn test_instance_3() {
        assert_debug_snapshot!(parse_module(indoc!(
            r#"
            module Test where
            instance (Bar a, Baz b) => Foo Int where
              bar = 1
        "#
        )));
    }

    #[test]
    fn test_instance_4() {
        assert_debug_snapshot!(parse_module(indoc!(
            r#"
            module Test where
            instance Foo Int where
        "#
        )));
    }

    #[test]
    fn test_instance_5() {
        assert_debug_snapshot!(parse_module(indoc!(
            r#"
            module Test where
            instance Foo Int
        "#
        )));
    }

    #[test]
    fn test_instance_6() {
        assert_debug_snapshot!(parse_module(indoc!(
            r#"
            module Test where
            instance namedInstance :: Foo Int where
        "#
        )));
    }

    #[test]
    fn test_instance_deriving() {
        assert_debug_snapshot!(parse_module(indoc!(
            r#"
            module Test where
            derive instance Foo Int
            derive newtype instance Foo Int
        "#
        )));
    }

    #[test]
    fn test_instance_chain() {
        assert_debug_snapshot!(parse_module(indoc!(
            r#"
            module Test where
            instance Foo Int where
              x = 1
            else instance Foo a where
              x = 2
        "#
        )));
    }

    #[test]
    fn test_data_decl() {
        assert_debug_snapshot!(parse_module(indoc!(
            r#"
            module Test where
            data Maybe a = Nothing | Just a
            newtype Foo = Foo Int
            foreign import data X
            foreign import data X :: Type
        "#
        )));
    }

    #[test]
    fn test_parse_atomic_type() {
        assert_debug_snapshot!(parse_type("var"));
        assert_debug_snapshot!(parse_type("\"string\""));
        assert_debug_snapshot!(parse_type("42"));
        assert_debug_snapshot!(parse_type("Int"));
        assert_debug_snapshot!(parse_type("Prelude.Int"));
    }

    #[test]
    fn test_parse_complex_type() {
        assert_debug_snapshot!(parse_type("Maybe Int"));
        assert_debug_snapshot!(parse_type("Either String Int"));
        assert_debug_snapshot!(parse_type("Array (Maybe Int)"));
    }

    #[test]
    fn test_parse_forall() {
        assert_debug_snapshot!(parse_type("forall x (y :: Symbol). Maybe x"));
    }

    #[test]
    fn test_parse_constraint() {
        assert_debug_snapshot!(parse_type("Eq a => a"));
    }

    #[test]
    fn test_parse_constraints() {
        assert_debug_snapshot!(parse_type("Eq a => Show a => a"));
    }

    #[test]
    fn test_parse_row_1() {
        assert_debug_snapshot!(parse_type("( foo :: Int, \"Bar\" :: String )"));
    }

    #[test]
    fn test_parse_row_2() {
        assert_debug_snapshot!(parse_type("( foo :: Int | e )"));
    }

    #[test]
    fn test_parse_row_3() {
        assert_debug_snapshot!(parse_type("( | e )"));
    }

    #[test]
    fn test_parse_row_4() {
        assert_debug_snapshot!(parse_type("()"));
    }

    #[test]
    fn test_parse_record() {
        assert_debug_snapshot!(parse_type("{ foo :: Int | e }"));
    }

    #[test]
    fn test_parse_function_type() {
        assert_debug_snapshot!(parse_type("A -> B -> C"));
    }

    #[test]
    fn test_function_as_type_operator() {
        assert_debug_snapshot!(parse_type("(->)"));
    }

    #[test]
    fn test_parse_literals() {
        assert_debug_snapshot!(parse_expr("123"));
        assert_debug_snapshot!(parse_expr(r#" "hello" "#));
        assert_debug_snapshot!(parse_expr(r#" true "#));
        assert_debug_snapshot!(parse_expr(r#" 'a' "#));
    }

    #[test]
    fn test_parse_array() {
        assert_debug_snapshot!(parse_expr(r#" [] "#));
        assert_debug_snapshot!(parse_expr(r#" [1] "#));
        assert_debug_snapshot!(parse_expr(r#" [true, false] "#));
    }

    #[test]
    fn test_parse_record_expr() {
        assert_debug_snapshot!(parse_expr(r#" {} "#));
        assert_debug_snapshot!(parse_expr(r#" { foo: 1 } "#));
        assert_debug_snapshot!(parse_expr(r#" { foo } "#));
        assert_debug_snapshot!(parse_expr(r#" { foo, bar: 2 } "#));
    }

    #[test]
    fn test_parse_infix_expr() {
        assert_debug_snapshot!(parse_expr(r#" 1 %+ 2 <$> 3 "#));
    }

    #[test]
    fn test_parse_accessor_1() {
        assert_debug_snapshot!(parse_expr(r#"foo.bar"#));
    }

    #[test]
    fn test_parse_accessor_2() {
        assert_debug_snapshot!(parse_expr(r#" foo."Bar" "#));
    }

    #[test]
    fn test_parse_accessor_chain() {
        assert_debug_snapshot!(parse_expr(r#" foo.bar.baz "#));
    }

    #[test]
    fn test_parse_qualified_var() {
        assert_debug_snapshot!(parse_expr(r#"Data.Maybe.fromJust"#));
    }

    #[test]
    fn test_parse_parens() {
        assert_debug_snapshot!(parse_expr(r#"(foo)"#));
    }

    #[test]
    fn test_parse_app_1() {
        assert_debug_snapshot!(parse_expr(r#"f x y"#));
    }

    #[test]
    fn test_parse_app_2() {
        assert_debug_snapshot!(parse_expr(r#"f a.b (g x)"#));
    }

    #[test]
    fn test_parse_lam_1() {
        assert_debug_snapshot!(parse_expr(r#"\x -> y"#));
    }

    #[test]
    fn test_parse_lam_2() {
        assert_debug_snapshot!(parse_expr(r#"\_ y -> y"#));
    }

    #[test]
    fn test_fat_arrows_as_operators() {
        assert_debug_snapshot!(parse_expr(r#"1 <= 2 >= 3"#));
    }

    #[test]
    fn test_case() {
        assert_debug_snapshot!(parse_expr(indoc!(
            "
          case x of
            C a b ->
              1
            D (A c) _ -> 1
            E -> 1
            _ -> 1
        "
        )));
    }

    #[test]
    fn test_typed_expr() {
        assert_debug_snapshot!(parse_expr("foo bar :: Int"));
    }

    #[test]
    fn test_if() {
        assert_debug_snapshot!(parse_expr("if b then 1 else 2"));
    }

    #[test]
    fn test_let_1() {
        assert_debug_snapshot!(parse_expr("let x = 1 in x"));
    }

    #[test]
    fn test_let_2() {
        assert_debug_snapshot!(parse_expr(indoc!(
            "
            let
                x :: Int
                x = 1

                y = 2
                Tuple a b = y
            in \\z -> x + z
        "
        )));
    }

    #[test]
    fn test_wildcard() {
        assert_debug_snapshot!(parse_expr("_.foo"));
    }

    #[test]
    fn test_data_con_expr() {
        assert_debug_snapshot!(parse_expr("Just 1"));
    }

    #[test]
    fn test_block_argument() {
        assert_debug_snapshot!(parse_expr("f \\x -> y"));
    }

    #[test]
    fn test_block_argument_2() {
        assert_debug_snapshot!(parse_expr("f 1 \\x -> y"));
    }

    #[test]
    fn test_lambda_infix() {
        assert_debug_snapshot!(parse_expr("1 + \\x -> y + 2"));
    }

    #[test]
    fn test_lambda_typed() {
        assert_debug_snapshot!(parse_expr("\\x -> 1 :: Int"));
    }

    #[test]
    fn test_named_pattern() {
        assert_debug_snapshot!(parse_expr("\\x@Nothing -> y"));
    }

    #[test]
    fn test_fn_arg_con_arity0() {
        assert_debug_snapshot!(parse_module(indoc!(
            "
        module Some.Module where
        f Nothing = 1
        "
        )));
    }

    #[test]
    fn test_record_update_1() {
        assert_debug_snapshot!(parse_expr("r { x = 1 }"));
    }

    #[test]
    fn test_record_update_2() {
        assert_debug_snapshot!(parse_expr("f r { x = 1, y = 2, \"random label\" = 3 }"));
    }

    #[test]
    fn test_record_update_3() {
        assert_debug_snapshot!(parse_expr("f r { x = 1 } { y: 2 } q"));
    }

    #[test]
    fn test_do_simple() {
        assert_debug_snapshot!(parse_expr(indoc!(
            "
          do
            x <- f
            pure 1
        "
        )));
    }

    #[test]
    fn test_do_let() {
        assert_debug_snapshot!(parse_expr(indoc!(
            "
          do
            let x = 1
            pure 2
        "
        )));
    }

    #[test]
    fn test_do_destructuring_pattern() {
        assert_debug_snapshot!(parse_expr(indoc!(
            "
          do
            Tuple x y <- foo
            pure 2
        "
        )));
    }

    #[test]
    fn test_do_bind_type_sig() {
        assert_debug_snapshot!(parse_expr(indoc!(
            "
          do
            x :: Int <- foo
            pure 2
        "
        )));
    }

    //
}
