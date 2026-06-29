#![feature(coroutines, coroutine_trait, yield_expr)]

use effects::{Effect, Suspend, forward, suspend};

#[derive(Clone)]
pub struct Env {}

#[derive(Clone)]
pub enum Type {}

#[derive(Clone)]
pub enum Term {
    App(Box<Term>, Box<Term>),
}

pub enum Stack {
    Beta(Term, Env),
    Subst,
}

pub struct Push(Type);
impl Effect for Push {
    type Resume = Type;
}

pub struct Pop;
impl Effect for Pop {
    type Resume = Type;
}

pub struct Enter(Stack);
impl Effect for Enter {
    type Resume = ();
}

pub struct Exit;
impl Effect for Exit {
    type Resume = Option<Stack>;
}

#[suspend]
fn beta_cc(term: Term, env: Env) -> Suspend<(), (Push, Pop, Enter, Exit)> {
    match term {
        Term::App(callee, arg) => {
            yield Enter(Stack::Subst);
            yield Enter(Stack::Beta(*callee, env.clone()));
            yield Enter(Stack::Beta(*arg, env.clone()))
        }
    }
}

#[suspend]
fn eval_cc() -> Suspend<Type, (Push, Pop, Enter, Exit)> {
    while let Some(stack) = (yield Exit) {
        match stack {
            Stack::Beta(term, env) => {
                forward!(beta_cc(term, env));
            }
            Stack::Subst => todo!(),
        }
    }
    todo!()
}
