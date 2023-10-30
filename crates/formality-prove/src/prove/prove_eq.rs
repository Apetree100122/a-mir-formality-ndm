use formality_types::{
    cast::{Downcast, Upcast},
    collections::{Deduplicate, Set},
    grammar::{
        AliasTy, ExistentialVar, Goal, Parameter, RigidTy, Substitution, TyData, UniversalVar,
        Variable, Wcs,
    },
    judgment_fn, set,
    visit::Visit,
};

use crate::{
    decls::Decls,
    prove::{constraints::occurs_in, prove_after::prove_after, prove_normalize::prove_normalize},
};

use super::{constraints::Constraints, env::Env};

judgment_fn! {
    /// Compute the constraints that make two parameters `a` and `b` equal
    /// (semantically equivalent), given the `assumptions`.
    pub fn prove_all_eq(
        decls: Decls,
        env: Env,
        assumptions: Wcs,
        a: Vec<Parameter>,
        b: Vec<Parameter>,
    ) => Constraints {
        debug(a, b, assumptions, env, decls)

        trivial(a == b => Constraints::none(env))

        (
            ----------------------------- ("prove-all-none")
            (prove_all_eq(_decls, env, _assumptions, (), ()) => Constraints::none(env))
        )

        (
            (prove_eq(&decls, env, &assumptions, a, b) => c)
            (prove_after(&decls, c, &assumptions, Goal::all_eq(&a_s, &b_s)) => c)
            ----------------------------- ("prove-all-some")
            (prove_all_eq(decls, env, assumptions, (a, a_s), (b, b_s)) => c)
        )
    }
}

judgment_fn! {
    /// Compute the constraints that make two parameters `a` and `b` equal
    /// (semantically equivalent), given the `assumptions`.
    pub fn prove_eq(
        decls: Decls,
        env: Env,
        assumptions: Wcs,
        a: Parameter,
        b: Parameter,
    ) => Constraints {
        debug(a, b, assumptions, env, decls)

        trivial(a == b => Constraints::none(env))

        (
            (prove_syntactically_eq(decls, env, assumptions, a, b) => c)
            ----------------------------- ("syntactic")
            (prove_eq(decls, env, assumptions, a, b) => c)
        )

        (
            (prove_normalize(&decls, env, &assumptions, &a) => (c, a1))
            (prove_after(&decls, c, &assumptions, Goal::eq(a1, &b)) => c)
            ----------------------------- ("normalize-l")
            (prove_eq(decls, env, assumptions, a, b) => c)
        )

        (
            (prove_normalize(&decls, env, &assumptions, &b) => (c, b1))
            (prove_after(&decls, c, &assumptions, Goal::eq(&a, b1)) => c)
            ----------------------------- ("normalize-r")
            (prove_eq(decls, env, assumptions, a, b) => c)
        )
    }
}

judgment_fn! {
    /// Compute the constraints that make two parameters `a` and `b` equal
    /// (semantically equivalent), given the `assumptions`.
    fn prove_syntactically_eq(
        decls: Decls,
        env: Env,
        assumptions: Wcs,
        a: Parameter,
        b: Parameter,
    ) => Constraints {
        debug(a, b, assumptions, env, decls)

        trivial(a == b => Constraints::none(env))

        (
            (let RigidTy { name: a_name, parameters: a_parameters } = a)
            (let RigidTy { name: b_name, parameters: b_parameters } = b)
            (if a_name == b_name)
            (prove_all_eq(decls, env, assumptions, a_parameters, b_parameters) => c)
            ----------------------------- ("rigid")
            (prove_syntactically_eq(decls, env, assumptions, TyData::RigidTy(a), TyData::RigidTy(b)) => c)
        )

        (
            (let AliasTy { name: a_name, parameters: a_parameters } = a)
            (let AliasTy { name: b_name, parameters: b_parameters } = b)
            (if a_name == b_name)
            (prove_all_eq(decls, env, assumptions, a_parameters, b_parameters) => c)
            ----------------------------- ("alias")
            (prove_syntactically_eq(decls, env, assumptions, TyData::AliasTy(a), TyData::AliasTy(b)) => c)
        )

        (
            (prove_existential_var_eq(decls, env, assumptions, v, r) => c)
            ----------------------------- ("existential-l")
            (prove_syntactically_eq(decls, env, assumptions, Variable::ExistentialVar(v), r) => c)
        )

        (
            (prove_existential_var_eq(decls, env, assumptions, v, l) => c)
            ----------------------------- ("existential-r")
            (prove_syntactically_eq(decls, env, assumptions, l, Variable::ExistentialVar(v)) => c)
        )
    }
}

judgment_fn! {
    pub fn prove_existential_var_eq(
        decls: Decls,
        env: Env,
        assumptions: Wcs,
        v: ExistentialVar,
        b: Parameter,
    ) => Constraints {
        debug(v, b, assumptions, env, decls)

        (
            (if let None = t.downcast::<Variable>())
            (equate_variable(decls, env, assumptions, v, t) => c)
            ----------------------------- ("existential-nonvar")
            (prove_existential_var_eq(decls, env, assumptions, v, t) => c)
        )

        (
            // Map the higher rank variable to the lower rank one.
            (let (a, b) = env.order_by_universe(l, r))
            ----------------------------- ("existential-existential")
            (prove_existential_var_eq(_decls, env, _assumptions, l, Variable::ExistentialVar(r)) => (env, (b, a)))
        )

        (
            (if env.universe(p) < env.universe(v))
            ----------------------------- ("existential-universal")
            (prove_existential_var_eq(_decls, env, _assumptions, v, Variable::UniversalVar(p)) => (env, (v, p)))
        )
    }
}

fn equate_variable(
    decls: Decls,
    mut env: Env,
    assumptions: Wcs,
    x: ExistentialVar,
    p: impl Upcast<Parameter>,
) -> Set<Constraints> {
    let p: Parameter = p.upcast();

    let span = tracing::debug_span!("equate_variable", ?x, ?p, ?env);
    let _guard = span.enter();

    // Preconditions:
    // * Environment contains all free variables
    // * `p` is some compound type, not a variable
    //   (variables are handled via special rules above)
    assert!(env.encloses((x, (&assumptions, &p))));
    assert!(!p.is_a::<Variable>());

    let fvs = p.free_variables().deduplicate();

    // Ensure that `x` passes the occurs check for the free variables in `p`.
    if occurs_in(x, &fvs) {
        return set![];
    }

    // Map each free variable `fv` in `p` that is of higher universe than `x`
    // to a fresh variable `y` of lower universe than `x`.
    //
    // e.g., in an environment `[X, Y]`, if we have `X = Vec<Y>`:
    // * we would create `Z` before `X` (so new env is `[Z, X, Y]`)
    // * and map `Y` to `Z`
    let universe_x = env.universe(x);
    let universe_subst: Substitution = fvs
        .iter()
        .flat_map(|fv| {
            if universe_x < env.universe(fv) {
                let y = env.insert_fresh_before(fv.kind(), universe_x);
                Some((fv, y))
            } else {
                None
            }
        })
        .collect();

    // Introduce the following constraints:
    //
    // * `fv = universe_subst(fv)` for each free existential variable `fv` in `p` (e.g., `Y => Z` in our example above)
    // * `x = universe_subst(p)` (e.g., `Vec<Z>` in our example above)
    let constraints: Constraints = Constraints::from(
        env,
        universe_subst
            .iter()
            .filter(|(v, _)| v.is_a::<ExistentialVar>())
            .chain(Some((x, universe_subst.apply(&p)).upcast())),
    );

    // For each universal variable that we replaced with an existential variable
    // above, we now have to prove that goal. e.g., if we had `X = Vec<!Y>`, we would replace `!Y` with `?Z`
    // (where `?Z` is in a lower universe than `X`), but now we must prove that `!Y = ?Z`
    // (this may be posible due to assumptions).
    let (variables, values): (Vec<Parameter>, Vec<Parameter>) = universe_subst
        .iter()
        .filter(|(v, _)| v.is_a::<UniversalVar>())
        .map(|(v, p)| (v.upcast(), p))
        .unzip();

    tracing::debug!(
        "equated: constraints={:?}, variables={:?} values={:?}",
        constraints,
        variables,
        values
    );

    prove_after(
        decls,
        constraints,
        assumptions,
        Goal::all_eq(variables, values),
    )
}
