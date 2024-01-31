// Copyright (c) The Move Contributors
// SPDX-License-Identifier: Apache-2.0

use crate::{
    diag,
    diagnostics::Diagnostic,
    expansion::ast::ModuleIdent,
    naming::ast::{self as N, BlockLabel, Color, TParamID, Type, Type_, UseFuns, Var, Var_},
    parser::ast::FunctionName,
    shared::program_info::FunctionInfo,
    typing::ast::{self as T},
    typing::core::{self, TParamSubst},
};
use move_ir_types::location::*;
use std::collections::{BTreeMap, BTreeSet, VecDeque};

type LambdaMap = BTreeMap<Var_, (N::Lambda, Vec<Type>, Type)>;
type ArgMap = BTreeMap<Var_, (N::Exp, Type)>;

struct Context<'a, 'b> {
    core: &'a mut core::Context<'b>,
    // used for removing unbound params
    all_params: BTreeMap<Var_, /* is bound to a value (by-name or by-value) */ bool>,
    // used for expanding lambda calls in VarCall
    lambdas: LambdaMap,
    // used for expanding by-name arguments in Var usage
    by_name_args: ArgMap,
    tparam_subst: TParamSubst,
    macro_color: Color,
}

pub struct ExpandedMacro {
    pub by_value_args: Vec<(Var, T::Exp)>,
    pub body: Box<N::Exp>,
}

pub enum EitherArg<ByValue, ByName> {
    ByValue(ByValue),
    ByName(ByName),
}

pub type Arg = EitherArg<T::Exp, (N::Exp, Type)>;

pub(crate) fn call(
    context: &mut core::Context,
    call_loc: Loc,
    m: ModuleIdent,
    f: FunctionName,
    type_args: Vec<Type>,
    args: Vec<Arg>,
    return_type: Type,
) -> Option<ExpandedMacro> {
    let next_color = context.next_variable_color();
    // If none, there is no body to expand, likely because of an error in the macro definition
    let macro_body = context.macro_body(&m, &f)?;
    let macro_info = context.function_info(&m, &f);
    let (macro_type_params, macro_params, mut macro_body, return_label, max_color) =
        match recolor_macro(call_loc, &m, &f, macro_info, macro_body, next_color) {
            Ok(res) => res,
            Err(None) => {
                assert!(context.env.has_errors());
                return None;
            }
            Err(Some(diag)) => {
                context.env.add_diag(*diag);
                return None;
            }
        };
    context.set_max_variable_color(max_color);

    if macro_type_params.len() != type_args.len() || macro_params.len() != args.len() {
        assert!(context.env.has_errors());
        return None;
    }
    // tparam subst
    assert_eq!(
        macro_type_params.len(),
        type_args.len(),
        "ICE should be fixed/caught by the module/method call"
    );
    let tparam_subst = macro_type_params.into_iter().zip(type_args).collect();
    // make separate out by-value and by-name arguments
    let mut all_params: BTreeMap<_, _> = macro_params
        .iter()
        .map(|(_, sp!(_, v_), _)| (*v_, false))
        .collect();
    let mut lambdas = BTreeMap::new();
    let mut by_name_args = BTreeMap::new();
    let mut by_value_args = vec![];
    for ((_, param, _param_ty), arg) in macro_params.into_iter().zip(args) {
        if param.value.name == symbol!("_") {
            // skip ignored args
            continue;
        }
        let arg_ty = match &arg {
            Arg::ByValue(e) => e.ty.clone(),
            Arg::ByName((_, ty)) => ty.clone(),
        };
        let unfolded = core::unfold_type(&context.subst, arg_ty);
        if let sp!(_, Type_::Fun(param_tys, result_ty)) = unfolded {
            let arg_exp = match arg {
                Arg::ByValue(_) => {
                    assert!(
                        context.env.has_errors(),
                        "ICE lambda args should never be by value"
                    );
                    continue;
                }
                Arg::ByName((e, _)) => e,
            };
            bind_lambda(
                context,
                &mut lambdas,
                param.value,
                arg_exp,
                param_tys,
                *result_ty,
            )?;
        } else {
            match arg {
                Arg::ByValue(e) => by_value_args.push((param, e)),
                Arg::ByName((e, expected_ty)) => {
                    by_name_args.insert(param.value, (e, expected_ty));
                }
            }
        }
        all_params.insert(param.value, true);
    }
    let break_labels: BTreeSet<_> = BTreeSet::from([return_label]);
    let mut context = Context {
        core: context,
        lambdas,
        all_params,
        by_name_args,
        tparam_subst,
        macro_color: next_color,
    };
    block(&mut context, &mut macro_body);
    let mut wrapped_body = Box::new(sp(call_loc, N::Exp_::Block(macro_body)));
    for label in break_labels {
        let seq = (
            N::UseFuns::new(next_color),
            VecDeque::from([sp(call_loc, N::SequenceItem_::Seq(wrapped_body))]),
        );
        let block = N::Block {
            name: Some(label),
            from_lambda_expansion: None,
            seq,
        };
        wrapped_body = Box::new(sp(call_loc, N::Exp_::Block(block)));
    }
    let body = Box::new(sp(call_loc, N::Exp_::Annotate(wrapped_body, return_type)));
    Some(ExpandedMacro {
        by_value_args,
        body,
    })
}

fn recolor_macro(
    call_loc: Loc,
    _m: &ModuleIdent,
    _f: &FunctionName,
    macro_info: &FunctionInfo,
    macro_body: &N::Sequence,
    color: u16,
) -> Result<
    (
        Vec<TParamID>,
        Vec<(Option<Loc>, Var, N::Type)>,
        N::Block,
        BlockLabel,
        Color,
    ),
    Option<Box<Diagnostic>>,
> {
    let FunctionInfo {
        macro_, signature, ..
    } = macro_info;
    if macro_.is_none() {
        // error handled in call type checking
        return Err(None);
    }
    let N::FunctionSignature {
        type_parameters,
        parameters,
        ..
    } = signature;
    let tparam_ids = type_parameters.iter().map(|t| t.id).collect();
    let label = sp(
        call_loc,
        N::Var_ {
            name: N::BlockLabel::MACRO_RETURN_NAME_SYMBOL,
            id: 0,
            color,
        },
    );
    let return_label = BlockLabel {
        label,
        is_implicit: true,
    };
    let recolor_use_funs = true;
    let recolor = &mut Recolor::new(color, Some(return_label), recolor_use_funs);
    recolor.add_params(&parameters);
    let parameters = parameters
        .iter()
        .map(|(mut_, v, t)| (*mut_, recolor_var_owned(recolor, *v), t.clone()))
        .collect();
    let body = {
        let mut body = macro_body.clone();
        recolor_seq(recolor, &mut body);
        N::Block {
            name: None,
            from_lambda_expansion: None,
            seq: body,
        }
    };
    let max_color = recolor.max_color();
    debug_assert_eq!(color, max_color, "ICE should only have one color in macros");
    Ok((tparam_ids, parameters, body, return_label, max_color))
}

fn bind_lambda(
    context: &mut core::Context,
    lambdas: &mut LambdaMap,
    param: Var_,
    arg: N::Exp,
    param_ty: Vec<Type>,
    result_ty: Type,
) -> Option<()> {
    match arg.value {
        N::Exp_::Lambda(lambda) => {
            lambdas.insert(param, (lambda, param_ty, result_ty));
            Some(())
        }
        _ => {
            let msg = format!(
                "Unable to bind lambda to parameter '{}'. The lambda must be passed directly",
                param.name
            );
            context
                .env
                .add_diag(diag!(TypeSafety::CannotExpandMacro, (arg.loc, msg)));
            None
        }
    }
}

//**************************************************************************************************
// recolor
//**************************************************************************************************

use recolor_struct::*;

mod recolor_struct {
    use crate::naming::ast::{self as N, BlockLabel, Color, Var};
    use move_ir_types::location::Loc;
    use std::collections::{BTreeMap, BTreeSet};
    // handles all of the recoloring of variables, labels, and use funs.
    // The mask of known vars and labels is here to handle the case where a variable was captured
    // by a lambda
    pub(super) struct Recolor {
        next_color: Color,
        remapping: BTreeMap<Color, Color>,
        recolor_use_funs: bool,
        return_label: Option<BlockLabel>,
        vars: BTreeSet<Var>,
        block_labels: BTreeSet<BlockLabel>,
    }

    impl Recolor {
        pub fn new(color: u16, return_label: Option<BlockLabel>, recolor_use_funs: bool) -> Self {
            Self {
                next_color: color,
                remapping: BTreeMap::new(),
                recolor_use_funs,
                return_label,
                vars: BTreeSet::new(),
                block_labels: BTreeSet::new(),
            }
        }

        pub fn add_params(&mut self, params: &[(Option<Loc>, Var, N::Type)]) {
            for (_, v, _) in params {
                self.vars.insert(*v);
            }
        }

        pub fn add_lvalues(&mut self, lvalues: &N::LValueList) {
            for lvalue in &lvalues.value {
                self.add_lvalue(lvalue)
            }
        }

        pub fn add_lvalue(&mut self, sp!(_, lvalue_): &N::LValue) {
            match lvalue_ {
                N::LValue_::Ignore => (),
                N::LValue_::Var { var, .. } => {
                    self.vars.insert(*var);
                }
                N::LValue_::Unpack(_, _, _, lvalues) => {
                    for (_, _, (_, lvalue)) in lvalues {
                        self.add_lvalue(lvalue)
                    }
                }
            }
        }

        pub fn add_block_label(&mut self, label: BlockLabel) {
            self.block_labels.insert(label);
        }

        // We need to fully remap colors, and not simply set everything to the specified color,
        // to handle the case where a lambda captures another expanded lambda, for example
        // `|i| v.push_back(f(i))`
        // where f is
        // `|i| i``
        // In this case we have
        // two different colored `i`s when applying the outer lambda, e.g.
        // `let i#_#c = arg; v.push_back({ let i#_#d = i#_#c; i#_#d })`
        // we need to make sure `i#_#c` and `i#_#d` remain separated
        //
        // This has similar feeling to lifting  De Bruijn indices, though it is not exactly the same
        // (... I think)
        pub fn remap_color(&mut self, color: Color) -> Color {
            *self.remapping.entry(color).or_insert_with(|| {
                let cur = self.next_color;
                self.next_color += 1;
                cur
            })
        }

        pub fn recolor_use_funs(&self) -> bool {
            self.recolor_use_funs
        }

        pub fn max_color(&self) -> Color {
            if self.remapping.is_empty() {
                // next color never used
                self.next_color
            } else {
                // subtract one to skip the "next" color
                let max = self.next_color - 1;
                debug_assert!(self.remapping.values().all(|&c| c <= max));
                max
            }
        }

        pub fn return_label(&self) -> Option<BlockLabel> {
            self.return_label
        }

        pub fn contains_var(&self, v: &Var) -> bool {
            self.vars.contains(v)
        }

        pub fn contains_block_label(&self, label: &BlockLabel) -> bool {
            self.block_labels.contains(label)
        }
    }
}

fn recolor_var_owned(ctx: &mut Recolor, mut v: Var) -> Var {
    assert!(ctx.contains_var(&v));
    recolor_var(ctx, &mut v);
    v
}

fn recolor_var(ctx: &mut Recolor, v: &mut Var) {
    // do not recolor if not in the ctx
    // this is to handle captured variables in lambda bodies
    if !ctx.contains_var(v) {
        return;
    }
    v.value.color = ctx.remap_color(v.value.color);
}

fn recolor_block_label_owned(ctx: &mut Recolor, mut label: BlockLabel) -> BlockLabel {
    assert!(ctx.contains_block_label(&label));
    recolor_block_label(ctx, &mut label);
    label
}

fn recolor_block_label(ctx: &mut Recolor, label: &mut BlockLabel) {
    // do not recolor if not in the ctx
    // this is to handle captured labels in lambda bodies
    if !ctx.contains_block_label(label) {
        return;
    }
    label.label.value.color = ctx.remap_color(label.label.value.color);
}

fn recolor_use_funs(ctx: &mut Recolor, use_funs: &mut UseFuns) {
    recolor_use_funs_(ctx, &mut use_funs.color);
}

fn recolor_use_funs_(ctx: &mut Recolor, use_fun_color: &mut Color) {
    if ctx.recolor_use_funs() {
        assert_eq!(
            *use_fun_color, 0,
            "ICE only expected to recolor use funs in fresh macro bodies"
        );
        *use_fun_color = ctx.remap_color(*use_fun_color);
    }
}

fn recolor_seq(ctx: &mut Recolor, (use_funs, seq): &mut N::Sequence) {
    recolor_use_funs(ctx, use_funs);
    for sp!(_, item_) in seq {
        match item_ {
            N::SequenceItem_::Seq(e) => recolor_exp(ctx, e),
            N::SequenceItem_::Declare(lvalues, _) => {
                ctx.add_lvalues(lvalues);
                recolor_lvalues(ctx, lvalues)
            }
            N::SequenceItem_::Bind(lvalues, e) => {
                ctx.add_lvalues(lvalues);
                recolor_lvalues(ctx, lvalues);
                recolor_exp(ctx, e)
            }
        }
    }
}

fn recolor_lvalues(ctx: &mut Recolor, lvalues: &mut N::LValueList) {
    for lvalue in &mut lvalues.value {
        recolor_lvalue(ctx, lvalue)
    }
}

fn recolor_lvalue(ctx: &mut Recolor, sp!(_, lvalue_): &mut N::LValue) {
    match lvalue_ {
        N::LValue_::Ignore => (),
        N::LValue_::Var { var, .. } => recolor_var(ctx, var),
        N::LValue_::Unpack(_, _, _, lvalues) => {
            for (_, _, (_, lvalue)) in lvalues {
                recolor_lvalue(ctx, lvalue)
            }
        }
    }
}

fn recolor_exp(ctx: &mut Recolor, sp!(_, e_): &mut N::Exp) {
    match e_ {
        N::Exp_::Value(_) | N::Exp_::Constant(_, _) => (),
        N::Exp_::Give(_usage, label, e) => {
            recolor_block_label(ctx, label);
            recolor_exp(ctx, e)
        }
        N::Exp_::Continue(label) => recolor_block_label(ctx, label),
        N::Exp_::Unit { .. } | N::Exp_::UnresolvedError => (),
        N::Exp_::Var(var) => recolor_var(ctx, var),
        N::Exp_::Return(e) => {
            recolor_exp(ctx, e);
            if let Some(label) = ctx.return_label() {
                let N::Exp_::Return(e) =
                    std::mem::replace(e_, /* dummy */ N::Exp_::UnresolvedError)
                else {
                    unreachable!()
                };
                *e_ = N::Exp_::Give(N::NominalBlockUsage::Return, label, e)
            }
        }

        N::Exp_::Abort(e)
        | N::Exp_::Dereference(e)
        | N::Exp_::UnaryExp(_, e)
        | N::Exp_::Cast(e, _)
        | N::Exp_::Annotate(e, _) => recolor_exp(ctx, e),
        N::Exp_::Assign(lvalues, e) => {
            recolor_lvalues(ctx, lvalues);
            recolor_exp(ctx, e)
        }
        N::Exp_::IfElse(econd, et, ef) => {
            recolor_exp(ctx, econd);
            recolor_exp(ctx, et);
            recolor_exp(ctx, ef);
        }
        N::Exp_::Loop(name, e) => {
            ctx.add_block_label(*name);
            recolor_block_label(ctx, name);
            recolor_exp(ctx, e)
        }
        N::Exp_::While(name, econd, ebody) => {
            ctx.add_block_label(*name);
            recolor_block_label(ctx, name);
            recolor_exp(ctx, econd);
            recolor_exp(ctx, ebody)
        }
        N::Exp_::Block(N::Block {
            name: _,
            from_lambda_expansion: _,
            seq: s,
        }) => recolor_seq(ctx, s),
        N::Exp_::FieldMutate(ed, e) => {
            recolor_exp_dotted(ctx, ed);
            recolor_exp(ctx, e)
        }
        N::Exp_::Mutate(el, er) | N::Exp_::BinopExp(el, _, er) => {
            recolor_exp(ctx, el);
            recolor_exp(ctx, er)
        }
        N::Exp_::Pack(_, _, _, fields) => {
            for (_, _, (_, e)) in fields {
                recolor_exp(ctx, e)
            }
        }
        N::Exp_::Builtin(_, sp!(_, es))
        | N::Exp_::Vector(_, _, sp!(_, es))
        | N::Exp_::ModuleCall(_, _, _, _, sp!(_, es))
        | N::Exp_::ExpList(es) => {
            for e in es {
                recolor_exp(ctx, e)
            }
        }
        N::Exp_::MethodCall(ed, _, _, _, sp!(_, es)) => {
            recolor_exp_dotted(ctx, ed);
            for e in es {
                recolor_exp(ctx, e)
            }
        }
        N::Exp_::VarCall(v, sp!(_, es)) => {
            recolor_var(ctx, v);
            for e in es {
                recolor_exp(ctx, e)
            }
        }

        N::Exp_::Lambda(N::Lambda {
            parameters: sp!(_, parameters),
            return_type: _,
            return_label,
            use_fun_color,
            body,
        }) => {
            ctx.add_block_label(*return_label);
            for (lvs, _) in &*parameters {
                ctx.add_lvalues(lvs);
            }
            recolor_use_funs_(ctx, use_fun_color);
            for (lvs, _) in parameters {
                recolor_lvalues(ctx, lvs);
            }
            recolor_block_label(ctx, return_label);
            recolor_exp(ctx, body)
        }
        N::Exp_::ExpDotted(_dotted_usage, ed) => recolor_exp_dotted(ctx, ed),
    }
}

fn recolor_exp_dotted(ctx: &mut Recolor, sp!(_, ed_): &mut N::ExpDotted) {
    match ed_ {
        N::ExpDotted_::Exp(e) => recolor_exp(ctx, e),
        N::ExpDotted_::Dot(ed, _) => recolor_exp_dotted(ctx, ed),
    }
}

//**************************************************************************************************
// recolor
//**************************************************************************************************

fn types(context: &mut Context, tys: &mut [Type]) {
    for ty in tys {
        type_(context, ty)
    }
}

fn type_(context: &mut Context, ty: &mut N::Type) {
    *ty = core::subst_tparams(&context.tparam_subst, ty.clone())
}

fn block(context: &mut Context, b: &mut N::Block) {
    seq(context, &mut b.seq)
}

fn seq(context: &mut Context, (_use_funs, seq): &mut N::Sequence) {
    for sp!(_, item_) in seq {
        match item_ {
            N::SequenceItem_::Seq(e) => exp(context, e),
            N::SequenceItem_::Declare(lvs, _) => lvalues(context, lvs),
            N::SequenceItem_::Bind(lvs, e) => {
                lvalues(context, lvs);
                exp(context, e)
            }
        }
    }
}

fn lvalues(context: &mut Context, sp!(_, lvs_): &mut N::LValueList) {
    for lv in lvs_ {
        lvalue(context, lv)
    }
}

fn lvalue(context: &mut Context, sp!(_, lv_): &mut N::LValue) {
    match lv_ {
        N::LValue_::Ignore => (),
        N::LValue_::Var {
            var: sp!(_, v_), ..
        } => {
            if context.all_params.contains_key(v_) {
                assert!(
                    context.core.env.has_errors(),
                    "ICE cannot assign to macro parameter"
                );
                *lv_ = N::LValue_::Ignore
            }
        }
        N::LValue_::Unpack(_, _, tys_opt, lvalues) => {
            if let Some(tys) = tys_opt {
                types(context, tys)
            }
            for (_, _, (_, lv)) in lvalues {
                lvalue(context, lv)
            }
        }
    }
}

fn exp(context: &mut Context, sp!(eloc, e_): &mut N::Exp) {
    match e_ {
        N::Exp_::Value(_)
        | N::Exp_::Constant(_, _)
        | N::Exp_::Continue(_)
        | N::Exp_::Unit { .. }
        | N::Exp_::UnresolvedError => (),
        N::Exp_::Give(_, _, e)
        | N::Exp_::Return(e)
        | N::Exp_::Abort(e)
        | N::Exp_::Dereference(e)
        | N::Exp_::UnaryExp(_, e)
        | N::Exp_::Loop(_, e) => exp(context, e),
        N::Exp_::Cast(e, ty) | N::Exp_::Annotate(e, ty) => {
            exp(context, e);
            type_(context, ty)
        }
        N::Exp_::Assign(lvs, e) => {
            lvalues(context, lvs);
            exp(context, e)
        }
        N::Exp_::IfElse(econd, et, ef) => {
            exp(context, econd);
            exp(context, et);
            exp(context, ef);
        }
        N::Exp_::While(_name, econd, ebody) => {
            exp(context, econd);
            exp(context, ebody)
        }
        N::Exp_::Block(N::Block {
            name: _,
            from_lambda_expansion: _,
            seq: s,
        }) => seq(context, s),
        N::Exp_::FieldMutate(ed, e) => {
            exp_dotted(context, ed);
            exp(context, e)
        }
        N::Exp_::Mutate(el, er) | N::Exp_::BinopExp(el, _, er) => {
            exp(context, el);
            exp(context, er)
        }
        N::Exp_::Pack(_, _, tys_opt, fields) => {
            if let Some(tys) = tys_opt {
                types(context, tys)
            }
            for (_, _, (_, e)) in fields {
                exp(context, e)
            }
        }
        N::Exp_::Builtin(bf, sp!(_, es)) => {
            builtin_function(context, bf);
            exps(context, es)
        }
        N::Exp_::Vector(_, ty_opt, sp!(_, es)) => {
            if let Some(ty) = ty_opt {
                type_(context, ty)
            }
            exps(context, es)
        }
        N::Exp_::ModuleCall(_, _, _, tys_opt, sp!(_, es)) => {
            if let Some(tys) = tys_opt {
                types(context, tys)
            }
            exps(context, es)
        }
        N::Exp_::MethodCall(ed, _, _, tys_opt, sp!(_, es)) => {
            if let Some(tys) = tys_opt {
                types(context, tys)
            }
            exp_dotted(context, ed);
            exps(context, es)
        }
        N::Exp_::ExpList(es) => exps(context, es),
        N::Exp_::Lambda(N::Lambda {
            parameters: sp!(_, parameters),
            body: e,
            ..
        }) => {
            for (lvs, ty_opt) in parameters {
                lvalues(context, lvs);
                if let Some(ty) = ty_opt {
                    type_(context, ty)
                }
            }
            exp(context, e)
        }
        N::Exp_::ExpDotted(_usage, ed) => exp_dotted(context, ed),

        ///////
        // Lambda cases
        ///////
        N::Exp_::Var(sp!(_, v_)) if context.lambdas.contains_key(v_) => {
            let (lambda, _, _) = context.lambdas.get(v_).unwrap();
            *e_ = N::Exp_::Lambda(lambda.clone());
        }
        N::Exp_::VarCall(sp!(_, v_), sp!(argloc, es)) if context.lambdas.contains_key(v_) => {
            exps(context, es);
            // param_ty and result_ty have already been substituted
            let (
                N::Lambda {
                    parameters: sp!(_, mut lambda_params),
                    return_type: _,
                    return_label,
                    use_fun_color,
                    body: mut lambda_body,
                },
                param_tys,
                result_ty,
            ) = context.lambdas.get(&v_).unwrap().clone();
            // recolor in case the lambda is used more than once
            let next_color = context.core.next_variable_color();
            let recolor_use_funs = false;
            let recolor = &mut Recolor::new(
                next_color,
                /* return already labeled */ None,
                recolor_use_funs,
            );
            recolor.add_block_label(return_label);
            for (lvs, _) in &lambda_params {
                recolor.add_lvalues(lvs);
            }
            let return_label = recolor_block_label_owned(recolor, return_label);
            for (lvs, _) in &mut lambda_params {
                recolor_lvalues(recolor, lvs);
            }
            recolor_exp(recolor, &mut lambda_body);
            // set max color when coloring is finished
            context.core.set_max_variable_color(recolor.max_color());
            // check arity before expanding
            let argloc = *argloc;
            core::check_call_arity(
                context.core,
                *eloc,
                || format!("Invalid lambda call of '{}'", v_.name),
                param_tys.len(),
                argloc,
                es.len(),
            );
            // expand the call, replacing with a dummy value to take the args by value
            let N::Exp_::VarCall(_, sp!(_, args)) =
                std::mem::replace(e_, /* dummy */ N::Exp_::UnresolvedError)
            else {
                unreachable!()
            };
            let body_loc = lambda_body.loc;
            let annot_body = Box::new(sp(body_loc, N::Exp_::Annotate(lambda_body, result_ty)));
            let labeled_seq = VecDeque::from([sp(body_loc, N::SequenceItem_::Seq(annot_body))]);
            let labeled_body_ = N::Exp_::Block(N::Block {
                name: Some(return_label),
                // mark lambda expansion for recursive macro check
                from_lambda_expansion: Some(*eloc),
                seq: (N::UseFuns::new(use_fun_color), labeled_seq),
            });
            let labeled_body = Box::new(sp(body_loc, labeled_body_));
            // pad args with errors
            let args = args.into_iter().chain(std::iter::repeat_with(|| {
                sp(argloc, N::Exp_::UnresolvedError)
            }));
            // Unlike other by-name arguments, we try to check the type of the lambda before
            // expanding them macro. That, plus the arity check above, ensures these zips are safe
            let mut result: VecDeque<_> = lambda_params
                .into_iter()
                .zip(args)
                .zip(param_tys)
                .map(|(((lvs, _lv_ty_opt), arg), param_ty)| {
                    let param_loc = param_ty.loc;
                    let arg = Box::new(arg);
                    let annot_arg = Box::new(sp(param_loc, N::Exp_::Annotate(arg, param_ty)));
                    sp(param_loc, N::SequenceItem_::Bind(lvs, annot_arg))
                })
                .collect();
            result.push_back(sp(body_loc, N::SequenceItem_::Seq(labeled_body)));
            *e_ = N::Exp_::Block(N::Block {
                name: None,
                from_lambda_expansion: None,
                seq: (N::UseFuns::new(context.macro_color), result),
            });
        }

        ///////
        // Argument cases
        ///////
        N::Exp_::Var(sp!(_, v_)) if context.by_name_args.contains_key(v_) => {
            let (mut arg, expected_ty) = context.by_name_args.get(v_).cloned().unwrap();
            // recolor the arg in case it is used more than once
            let next_color = context.core.next_variable_color();
            let recolor_use_funs = false;
            let recolor = &mut Recolor::new(
                next_color,
                /* return already labeled */ None,
                recolor_use_funs,
            );
            recolor_exp(recolor, &mut arg);
            context.core.set_max_variable_color(recolor.max_color());

            *e_ = N::Exp_::Annotate(Box::new(arg), expected_ty);
        }
        N::Exp_::VarCall(sp!(_, v_), _) if context.by_name_args.contains_key(v_) => {
            let (arg, _expected_ty) = context.by_name_args.get(v_).unwrap();
            context.core.env.add_diag(diag!(
                TypeSafety::CannotExpandMacro,
                (*eloc, "Cannot call non-lambda argument"),
                (arg.loc, "Expected a lambda argument")
            ));
            *e_ = N::Exp_::UnresolvedError;
        }

        ///////
        // Other var cases
        ///////
        N::Exp_::Var(sp!(_, v_)) => {
            let is_unbound_param = context.all_params.get(v_).is_some_and(|is_bound| !is_bound);
            if is_unbound_param {
                assert!(!context.lambdas.contains_key(v_));
                assert!(!context.by_name_args.contains_key(v_));
                assert!(
                    context.core.env.has_errors(),
                    "ICE unbound param should have already resulted in an error"
                );
                *e_ = N::Exp_::UnresolvedError;
            }
        }
        N::Exp_::VarCall(sp!(_, v_), sp!(_, es)) => {
            exps(context, es);
            let is_unbound_param = context.all_params.get(v_).is_some_and(|is_bound| !is_bound);
            if is_unbound_param {
                assert!(!context.lambdas.contains_key(v_));
                assert!(!context.by_name_args.contains_key(v_));
                assert!(
                    context.core.env.has_errors(),
                    "ICE unbound param should have already resulted in an error"
                );
                *e_ = N::Exp_::UnresolvedError;
            }
        }
    }
}

fn builtin_function(context: &mut Context, sp!(_, bf_): &mut N::BuiltinFunction) {
    match bf_ {
        N::BuiltinFunction_::Freeze(ty_opt) => {
            if let Some(ty) = ty_opt {
                type_(context, ty)
            }
        }
        N::BuiltinFunction_::Assert(_) => todo!(),
    }
}

fn exp_dotted(context: &mut Context, sp!(_, ed_): &mut N::ExpDotted) {
    match ed_ {
        N::ExpDotted_::Exp(e) => exp(context, e),
        N::ExpDotted_::Dot(ed, _) => exp_dotted(context, ed),
    }
}

fn exps(context: &mut Context, es: &mut [N::Exp]) {
    for e in es {
        exp(context, e)
    }
}