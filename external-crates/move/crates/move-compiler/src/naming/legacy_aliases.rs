// Copyright (c) The Diem Core Contributors
// Copyright (c) The Move Contributors
// SPDX-License-Identifier: Apache-2.0

use crate::{
    diagnostics::Diagnostic,
    expansion::ast::{ModuleIdent, ModuleIdent_},
    ice,
    naming::{
        alias_map_builder::AliasMapBuilder, aliases::NameSet, name_resolver::ResolvedDefinition,
    },
    parser::ast::ModuleName,
    shared::{unique_map::UniqueMap, *},
};
use move_ir_types::location::*;

use super::aliases::NameMapKind;

type ScopeDepth = usize;

#[derive(Clone, Debug)]
pub struct NameMap {
    modules: UniqueMap<Name, (Option<ScopeDepth>, ModuleIdent)>,
    members: UniqueMap<Name, (Option<ScopeDepth>, ResolvedDefinition)>,
    pub kind: NameMapKind, // tracks the kind of the last last alias map builder pushed
    // essentially a mapping from ScopeDepth => AliasSet, which are the unused aliases at that depth
    unused: Vec<NameSet>,
}

pub struct OldNameMap(NameMapKind, Option<NameMap>);

impl NameMap {
    pub fn new() -> Self {
        Self {
            modules: UniqueMap::new(),
            members: UniqueMap::new(),
            unused: vec![],
            kind: NameMapKind::LegacyTopLevel,
        }
    }

    fn current_depth(&self) -> usize {
        self.unused.len()
    }

    pub fn module_alias_get(&mut self, n: &Name) -> Option<ModuleIdent> {
        match self.modules.get_mut(n) {
            None => None,
            Some((depth_opt, ident)) => {
                if let Some(depth) = depth_opt {
                    self.unused[*depth].modules.remove(n);
                }
                *depth_opt = None;
                // We are preserving the name's original location, rather than referring to where
                // the alias was defined. The name represents JUST the module name, though, so we do
                // not change location of the address as we don't have this information.
                // TODO maybe we should also keep the alias reference (or its location)?
                let sp!(
                    _,
                    ModuleIdent_ {
                        address,
                        module: ModuleName(sp!(_, module))
                    }
                ) = ident;
                let address = *address;
                let module = ModuleName(sp(n.loc, *module));
                Some(sp(n.loc, ModuleIdent_ { address, module }))
            }
        }
    }

    pub fn member_alias_get(&mut self, n: &Name) -> Option<ResolvedDefinition> {
        match self.members.get_mut(n) {
            None => None,
            Some((depth_opt, member)) => {
                if let Some(depth) = depth_opt {
                    self.unused[*depth].members.remove(n);
                }
                *depth_opt = None;
                // We are preserving the name's original location, rather than referring to where
                // the alias was defined. The name represents JUST the member name, though, so we do
                // not change location of the module as we don't have this information.
                // TODO maybe we should also keep the alias reference (or its location)?
                Some(member)
            }
        }
    }

    /// Adds all of the new items in the new inner scope as shadowing the outer one.
    /// Gives back the outer scope
    pub fn add_and_shadow_all(
        &mut self,
        loc: Loc,
        shadowing: AliasMapBuilder,
    ) -> Result<OldNameMap, Box<Diagnostic>> {
        if shadowing.is_empty() {
            return Ok(OldNameMap(self.kind, None));
        }

        let outer_scope = OldNameMap(self.kind, Some(self.clone()));
        let AliasMapBuilder::Legacy {
            modules: new_modules,
            members: new_members,
            ..
        } = shadowing
        else {
            return Err(Box::new(ice!((
                loc,
                "ICE alias map builder should be legacy for legacy"
            ))));
        };

        let next_depth = self.current_depth();
        let mut current_scope = NameSet::new(NameMapKind::Use);
        for (alias, (ident, is_implicit)) in new_modules {
            if !is_implicit {
                current_scope.modules.add(alias).unwrap();
            }
            self.modules.remove(&alias);
            self.modules.add(alias, (Some(next_depth), ident)).unwrap();
        }
        for (alias, ((mident, name, _kind), is_implicit)) in new_members {
            if !is_implicit {
                current_scope.members.add(alias).unwrap();
            }
            self.members.remove(&alias);
            self.members
                .add(alias, (Some(next_depth), (mident, name)))
                .unwrap();
        }
        self.unused.push(current_scope);
        self.kind = NameMapKind::Use;
        Ok(outer_scope)
    }

    // /// Similar to add_and_shadow but just removes aliases now shadowed by a type parameter
    // pub fn shadow_for_type_parameters<'a, I: IntoIterator<Item = &'a Name>>(
    //     &mut self,
    //     tparams: I,
    // ) -> OldNameMap
    // where
    //     I::IntoIter: ExactSizeIterator,
    // {
    //     let tparams_iter = tparams.into_iter();
    //     if tparams_iter.len() == 0 {
    //         self.kind = NameMapKind::TypeParameters;
    //         return OldNameMap(self.kind, None);
    //     }

    //     let outer_scope = OldNameMap(self.kind, Some(self.clone()));
    //     self.kind = NameMapKind::TypeParameters;
    //     self.unused.push(NameSet::new(NameMapKind::TypeParameters));
    //     for tp_name in tparams_iter {
    //         self.members.remove(tp_name);
    //     }
    //     outer_scope
    // }

    /// Resets the alias map and gives the set of aliases that were unused
    pub fn set_to_outer_scope(&mut self, outer_scope: OldNameMap) -> NameSet {
        let outer_scope = match outer_scope {
            OldNameMap(old_kind, None) => {
                let cur_kind = self.kind;
                self.kind = old_kind;
                return NameSet::new(self.kind);
            }
            Some(outer) => outer,
        };
        let mut inner_scope = std::mem::replace(self, outer_scope);
        let outer_scope = self;
        assert!(outer_scope.current_depth() + 1 == inner_scope.current_depth());
        let unused = inner_scope.unused.pop().unwrap();
        outer_scope.unused = inner_scope.unused;
        unused
    }
}
