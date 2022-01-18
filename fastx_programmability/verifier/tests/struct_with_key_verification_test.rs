// Copyright (c) Facebook, Inc. and its affiliates.
// SPDX-License-Identifier: Apache-2.0

mod common;

pub use common::module_builder::ModuleBuilder;
use fastx_types::FASTX_FRAMEWORK_ADDRESS;
use fastx_verifier::struct_with_key_verifier::verify_module;
use move_binary_format::file_format::*;
use move_core_types::account_address::AccountAddress;

#[test]
fn key_struct_with_drop() {
    let (mut module, id_struct) = ModuleBuilder::default();
    module.add_struct(
        module.get_self_index(),
        "S",
        AbilitySet::EMPTY | Ability::Key | Ability::Drop,
        vec![("id", SignatureToken::Struct(id_struct.handle))],
    );
    assert!(verify_module(module.get_module())
        .unwrap_err()
        .to_string()
        .contains("Struct S cannot have both key and drop abilities"));
}

#[test]
fn non_key_struct_without_fields() {
    let (mut module, _) = ModuleBuilder::default();
    module.add_struct(module.get_self_index(), "S", AbilitySet::EMPTY, vec![]);
    assert!(verify_module(module.get_module()).is_ok());
}

#[test]
fn key_struct_without_fields() {
    let (mut module, _) = ModuleBuilder::default();
    module.add_struct(
        module.get_self_index(),
        "S",
        AbilitySet::EMPTY | Ability::Key,
        vec![],
    );
    assert!(verify_module(module.get_module())
        .unwrap_err()
        .to_string()
        .contains("First field of struct S must be 'id', no field found"));
}

#[test]
fn key_struct_first_field_not_id() {
    let (mut module, id_struct) = ModuleBuilder::default();
    module.add_struct(
        module.get_self_index(),
        "S",
        AbilitySet::EMPTY | Ability::Key,
        vec![("foo", SignatureToken::Struct(id_struct.handle))],
    );
    assert!(verify_module(module.get_module())
        .unwrap_err()
        .to_string()
        .contains("First field of struct S must be 'id', foo found"));
}

#[test]
fn key_struct_second_field_id() {
    let (mut module, id_struct) = ModuleBuilder::default();
    module.add_struct(
        module.get_self_index(),
        "S",
        AbilitySet::EMPTY | Ability::Key,
        vec![
            ("foo", SignatureToken::Struct(id_struct.handle)),
            ("id", SignatureToken::Struct(id_struct.handle)),
        ],
    );
    assert!(verify_module(module.get_module())
        .unwrap_err()
        .to_string()
        .contains("First field of struct S must be 'id', foo found"));
}

#[test]
fn key_struct_id_field_incorrect_type() {
    let (mut module, _) = ModuleBuilder::default();
    module.add_struct(
        module.get_self_index(),
        "S",
        AbilitySet::EMPTY | Ability::Key,
        vec![("id", SignatureToken::U64)],
    );
    assert!(verify_module(module.get_module())
        .unwrap_err()
        .to_string()
        .contains("First field of struct S must be of ID type, U64 type found"));
}

#[test]
fn key_struct_id_field_incorrect_struct_address() {
    let (mut module, _) = ModuleBuilder::default();
    let new_module_idx =
        module.add_module(AccountAddress::new([1u8; AccountAddress::LENGTH]), "ID");
    let fake_id_struct = module.add_struct(
        new_module_idx,
        "ID",
        AbilitySet::EMPTY | Ability::Store | Ability::Drop,
        vec![],
    );
    module.add_struct(
        new_module_idx,
        "S",
        AbilitySet::EMPTY | Ability::Key,
        vec![("id", SignatureToken::Struct(fake_id_struct.handle))],
    );
    assert!(verify_module(module.get_module()).unwrap_err().to_string().contains(&format!("First field of struct S must be of type {}::ID::ID, 01010101010101010101010101010101::ID::ID type found", FASTX_FRAMEWORK_ADDRESS)));
}

#[test]
fn key_struct_id_field_incorrect_struct_name() {
    let (mut module, _) = ModuleBuilder::default();
    let fake_id_struct = module.add_struct(
        module.get_self_index(),
        "FOO",
        AbilitySet::EMPTY | Ability::Store | Ability::Drop,
        vec![],
    );
    module.add_struct(
        module.get_self_index(),
        "S",
        AbilitySet::EMPTY | Ability::Key,
        vec![("id", SignatureToken::Struct(fake_id_struct.handle))],
    );
    assert!(verify_module(module.get_module())
        .unwrap_err()
        .to_string()
        .contains(&format!(
            "First field of struct S must be of type {0}::ID::ID, {0}::ID::FOO type found",
            FASTX_FRAMEWORK_ADDRESS
        )));
}

#[test]
fn key_struct_id_field_valid() {
    let (mut module, id_struct) = ModuleBuilder::default();
    module.add_struct(
        module.get_self_index(),
        "S",
        AbilitySet::EMPTY | Ability::Key,
        vec![("id", SignatureToken::Struct(id_struct.handle))],
    );
    assert!(verify_module(module.get_module()).is_ok());
}
