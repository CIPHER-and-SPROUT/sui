// Copyright (c) 2022, Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use crate::EventType;
use move_binary_format::errors::PartialVMResult;
use move_core_types::account_address::AccountAddress;
use move_vm_runtime::native_functions::NativeContext;
use move_vm_types::{
    gas_schedule::NativeCostIndex,
    loaded_data::runtime_types::Type,
    natives::function::{native_gas, NativeResult},
    pop_arg,
    values::{Struct, StructRef, Value},
};
use smallvec::smallvec;
use std::collections::VecDeque;

pub fn bytes_to_address(
    context: &mut NativeContext,
    ty_args: Vec<Type>,
    mut args: VecDeque<Value>,
) -> PartialVMResult<NativeResult> {
    debug_assert!(ty_args.is_empty());
    debug_assert!(args.len() == 1);

    let addr_bytes = pop_arg!(args, Vec<u8>);
    // unwrap safe because this native function is only called from new_from_bytes,
    // which already asserts the size of bytes to be equal of account address.
    let addr = AccountAddress::from_bytes(addr_bytes).unwrap();

    // TODO: what should the cost of this be?
    let cost = native_gas(context.cost_table(), NativeCostIndex::CREATE_SIGNER, 0);

    Ok(NativeResult::ok(cost, smallvec![Value::address(addr)]))
}

pub fn get_versioned_id(
    context: &mut NativeContext,
    ty_args: Vec<Type>,
    mut args: VecDeque<Value>,
) -> PartialVMResult<NativeResult> {
    debug_assert!(ty_args.len() == 1);
    debug_assert!(args.len() == 1);

    let obj = pop_arg!(args, StructRef);
    let id_field = obj.borrow_field(0)?;

    // TODO: what should the cost of this be?
    let cost = native_gas(context.cost_table(), NativeCostIndex::SIGNER_BORROW, 0);

    Ok(NativeResult::ok(cost, smallvec![id_field]))
}

pub fn delete_id(
    context: &mut NativeContext,
    ty_args: Vec<Type>,
    mut args: VecDeque<Value>,
) -> PartialVMResult<NativeResult> {
    debug_assert!(ty_args.is_empty());
    debug_assert!(args.len() == 1);

    let obj = pop_arg!(args, Struct);
    // All of the following unwraps are safe by construction because a properly verified
    // bytecode ensures that the parameter must be of type UniqueID with the correct fields:
    // ```
    // UniqueID { id: ID { bytes: address } }
    // ```
    // Get the `id` field of the UniqueID struct, which is of type ID
    let id_field = obj
        .unpack()
        .unwrap()
        .next()
        .unwrap()
        .value_as::<Struct>()
        .unwrap();
    // Get the inner address of the ID type
    let id = id_field.unpack().unwrap().next().unwrap();

    // TODO: what should the cost of this be?
    let cost = native_gas(context.cost_table(), NativeCostIndex::EMIT_EVENT, 0);

    if !context.save_event(vec![], EventType::DeleteObjectID as u64, Type::Address, id)? {
        return Ok(NativeResult::err(cost, 0));
    }

    Ok(NativeResult::ok(cost, smallvec![]))
}
