use expect_test::expect;
use formality_macros::test;
use formality_types::{
    grammar::{Wc, Wcs},
    parse::term,
};

use crate::{program::Program, prove::prove};

/// Simple example program consisting only of two trait declarations.
fn program() -> Program {
    Program {
        max_size: 22,
        trait_decls: vec![term("trait Debug<ty Self> where {}")],
        impl_decls: vec![
            term("impl<ty T> Debug(Vec<T>) where {is_implemented(Debug(T))}"),
            term("impl<> Debug(u32) where {}"),
        ],
        alias_eq_decls: vec![],
        alias_bound_decls: vec![],
    }
}

#[test]
fn vec_u32_debug() {
    let assumptions: Wcs = Wcs::t();
    let goal: Wc = term("is_implemented(Debug(Vec<u32>))");
    let constraints = prove(program(), assumptions, goal);
    expect![[r#"
        {
            <> Constraints { known_true: true, substitution: Substitution { map: {} } },
        }
    "#]]
    .assert_debug_eq(&constraints);
}

#[test]
fn vec_vec_u32_debug() {
    let assumptions: Wcs = Wcs::t();
    let goal: Wc = term("is_implemented(Debug(Vec<Vec<u32>>))");
    let constraints = prove(program(), assumptions, goal);
    expect![[r#"
        {
            <> Constraints { known_true: true, substitution: Substitution { map: {} } },
        }
    "#]]
    .assert_debug_eq(&constraints);
}
