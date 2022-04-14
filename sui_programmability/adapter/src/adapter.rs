// Copyright (c) 2022, Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use anyhow::Result;

use crate::bytecode_rewriter::ModuleHandleRewriter;
use move_binary_format::{
    access::ModuleAccess,
    errors::PartialVMResult,
    file_format::{CompiledModule, LocalIndex, SignatureToken, StructHandleIndex},
};
use sui_framework::EventType;
use sui_types::{
    base_types::*,
    error::{SuiError, SuiResult},
    event::Event,
    fp_ensure,
    gas::SuiGasStatus,
    id::VersionedID,
    messages::CallResult,
    object::{Data, MoveObject, Object, Owner},
    storage::{DeleteKind, Storage},
};
use sui_verifier::{
    entry_points_verifier::{self, INIT_FN_NAME},
    verifier,
};

use move_core_types::{
    account_address::AccountAddress,
    identifier::Identifier,
    language_storage::{ModuleId, StructTag, TypeTag},
    resolver::{ModuleResolver, ResourceResolver},
    value::MoveTypeLayout,
};
use move_vm_runtime::{native_functions::NativeFunctionTable, session::SerializedReturnValues};
use std::{
    borrow::Borrow,
    collections::{BTreeMap, HashMap, HashSet},
    convert::TryFrom,
    fmt::Debug,
};

pub use move_vm_runtime::move_vm::MoveVM;

#[cfg(test)]
#[path = "unit_tests/adapter_tests.rs"]
mod adapter_tests;

pub fn new_move_vm(natives: NativeFunctionTable) -> Result<MoveVM, SuiError> {
    MoveVM::new(natives).map_err(|_| SuiError::ExecutionInvariantViolation)
}

/// Execute `module::function<type_args>(object_args ++ pure_args)` as a call from `sender` with the given `gas_budget`.
/// Execution will read from/write to the store in `state_view`.
/// IMPORTANT NOTES on the return value:
/// The return value is a two-layer SuiResult. The outer layer indicates whether a system error
/// has occurred (i.e. issues with the sui system, not with user transaction).
/// As long as there are no system issues we return Ok(SuiResult).
/// The inner SuiResult indicates the execution result. If execution failed, we return Ok(Err),
/// otherwise we return Ok(Ok).
/// TODO: Do we really need the two layers?
#[allow(clippy::too_many_arguments)]
pub fn execute<E: Debug, S: ResourceResolver<Error = E> + ModuleResolver<Error = E> + Storage>(
    vm: &MoveVM,
    state_view: &mut S,
    _natives: &NativeFunctionTable,
    module_id: ModuleId,
    function: &Identifier,
    type_args: Vec<TypeTag>,
    object_args: Vec<Object>,
    pure_args: Vec<Vec<u8>>,
    gas_status: &mut SuiGasStatus,
    ctx: &mut TxContext,
) -> SuiResult<Vec<CallResult>> {
    // object_owner_map maps from object ID to its exclusive object owner.
    // This map will be used for detecting circular ownership among
    // objects, which can only happen to objects exclusively owned
    // by objects.
    let mut object_owner_map = HashMap::new();
    for obj in &object_args {
        if let Owner::ObjectOwner(owner) = obj.owner {
            object_owner_map.insert(obj.id().into(), owner);
        }
    }

    let module = vm.load_module(&module_id, state_view)?;
    let TypeCheckSuccess {
        module_id,
        args,
        mutable_ref_objects,
        by_value_objects,
    } = resolve_and_type_check(&module, function, &type_args, object_args, pure_args)?;

    let mut args = args;
    args.push(ctx.to_vec());
    execute_internal(
        vm,
        state_view,
        &module_id,
        function,
        type_args,
        args,
        mutable_ref_objects,
        by_value_objects,
        object_owner_map,
        gas_status,
        ctx,
    )
}

/// This function calls into Move VM to execute a Move function
/// call.
#[allow(clippy::too_many_arguments)]
fn execute_internal<
    E: Debug,
    S: ResourceResolver<Error = E> + ModuleResolver<Error = E> + Storage,
>(
    vm: &MoveVM,
    state_view: &mut S,
    module_id: &ModuleId,
    function: &Identifier,
    type_args: Vec<TypeTag>,
    args: Vec<Vec<u8>>,
    mut mutable_ref_objects: BTreeMap<LocalIndex, Object>,
    by_value_objects: BTreeMap<ObjectID, Object>,
    object_owner_map: HashMap<SuiAddress, SuiAddress>,
    gas_status: &mut SuiGasStatus, // gas status for the current call operation
    ctx: &mut TxContext,
) -> SuiResult<Vec<CallResult>> {
    let mut session = vm.new_session(state_view);
    // script visibility checked manually for entry points
    let result = session
        .execute_function_bypass_visibility(
            module_id,
            function,
            type_args,
            args,
            gas_status.get_move_gas_status(),
        )
        .and_then(|ret| Ok((ret, session.finish()?)));

    match result {
        Ok((
            SerializedReturnValues {
                mut mutable_reference_outputs,
                return_values,
            },
            (change_set, events),
        )) => {
            // Sui Move programs should never touch global state, so ChangeSet should be empty
            debug_assert!(change_set.accounts().is_empty());
            // Input ref parameters we put in should be the same number we get out, plus one for the &mut TxContext
            debug_assert!(mutable_ref_objects.len() + 1 == mutable_reference_outputs.len());

            // When this function is used during publishing, it
            // may be executed several times, with objects being
            // created in the Move VM in each Move call. In such
            // case, we need to update TxContext value so that it
            // reflects what happened each time we call into the
            // Move VM (e.g. to account for the number of created
            // objects).
            let (_, ctx_bytes, _) = mutable_reference_outputs.pop().unwrap();
            let updated_ctx: TxContext = bcs::from_bytes(&ctx_bytes).unwrap();
            ctx.update_state(updated_ctx)?;

            let mutable_refs =
                mutable_reference_outputs
                    .into_iter()
                    .map(|(local_idx, bytes, _layout)| {
                        let object = mutable_ref_objects.remove(&local_idx).unwrap();
                        (object, bytes)
                    });
            process_successful_execution(
                state_view,
                by_value_objects,
                mutable_refs,
                events,
                ctx,
                object_owner_map,
            )?;
            // All mutable references should have been marked as updated
            debug_assert!(mutable_ref_objects.is_empty());
            Ok(process_return_values(&return_values))
        }
        // charge for all computations so far
        Err(error) => Err(SuiError::AbortedExecution {
            error: error.to_string(),
        }),
    }
}

fn process_return_values(return_values: &[(Vec<u8>, MoveTypeLayout)]) -> Vec<CallResult> {
    return_values
        .iter()
        .filter_map(|(bytes, ty_layout)| {
            Some(match ty_layout {
                // debug_assert-s for missing arms should be OK here as we
                // already checked in
                // MovePackage::check_and_get_entry_function that no other
                // types can exist in the signature

                // see CallResults struct comments for why this is
                // implemented the way it is
                MoveTypeLayout::Bool => CallResult::Bool(bcs::from_bytes(bytes).unwrap()),
                MoveTypeLayout::U8 => CallResult::U8(bcs::from_bytes(bytes).unwrap()),
                MoveTypeLayout::U64 => CallResult::U64(bcs::from_bytes(bytes).unwrap()),
                MoveTypeLayout::U128 => CallResult::U128(bcs::from_bytes(bytes).unwrap()),
                MoveTypeLayout::Address => CallResult::Address(bcs::from_bytes(bytes).unwrap()),
                MoveTypeLayout::Vector(t) => match &**t {
                    MoveTypeLayout::Bool => CallResult::BoolVec(bcs::from_bytes(bytes).unwrap()),
                    MoveTypeLayout::U8 => CallResult::U8Vec(bcs::from_bytes(bytes).unwrap()),
                    MoveTypeLayout::U64 => CallResult::U64Vec(bcs::from_bytes(bytes).unwrap()),
                    MoveTypeLayout::U128 => CallResult::U128Vec(bcs::from_bytes(bytes).unwrap()),
                    MoveTypeLayout::Address => CallResult::AddrVec(bcs::from_bytes(bytes).unwrap()),
                    MoveTypeLayout::Vector(inner_t) => match &**inner_t {
                        MoveTypeLayout::Bool => {
                            CallResult::BoolVecVec(bcs::from_bytes(bytes).unwrap())
                        }
                        MoveTypeLayout::U8 => CallResult::U8VecVec(bcs::from_bytes(bytes).unwrap()),
                        MoveTypeLayout::U64 => {
                            CallResult::U64VecVec(bcs::from_bytes(bytes).unwrap())
                        }
                        MoveTypeLayout::U128 => {
                            CallResult::U128VecVec(bcs::from_bytes(bytes).unwrap())
                        }
                        MoveTypeLayout::Address => {
                            CallResult::AddrVecVec(bcs::from_bytes(bytes).unwrap())
                        }
                        _ => {
                            debug_assert!(false);
                            return None;
                        }
                    },
                    _ => {
                        debug_assert!(false);
                        return None;
                    }
                },
                _ => {
                    debug_assert!(false);
                    return None;
                }
            })
        })
        .collect()
}

pub fn publish<E: Debug, S: ResourceResolver<Error = E> + ModuleResolver<Error = E> + Storage>(
    state_view: &mut S,
    natives: NativeFunctionTable,
    module_bytes: Vec<Vec<u8>>,
    ctx: &mut TxContext,
    gas_status: &mut SuiGasStatus,
) -> SuiResult {
    gas_status.charge_publish_package(module_bytes.iter().map(|v| v.len()).sum())?;
    let mut modules = module_bytes
        .iter()
        .map(|b| CompiledModule::deserialize(b))
        .collect::<PartialVMResult<Vec<CompiledModule>>>()
        .map_err(|err| SuiError::ModuleDeserializationFailure {
            error: err.to_string(),
        })?;

    fp_ensure!(
        !modules.is_empty(),
        SuiError::ModulePublishFailure {
            error: "Publishing empty list of modules".to_string(),
        }
    );

    let package_id = generate_package_id(&mut modules, ctx)?;
    let vm = verify_and_link(state_view, &modules, package_id, natives, gas_status)?;
    store_package_and_init_modules(state_view, &vm, modules, ctx, gas_status)
}

/// Store package in state_view and call module initializers
pub fn store_package_and_init_modules<
    E: Debug,
    S: ResourceResolver<Error = E> + ModuleResolver<Error = E> + Storage,
>(
    state_view: &mut S,
    vm: &MoveVM,
    modules: Vec<CompiledModule>,
    ctx: &mut TxContext,
    gas_status: &mut SuiGasStatus,
) -> SuiResult {
    let mut modules_to_init = Vec::new();
    for module in modules.iter() {
        if entry_points_verifier::module_has_init(module) {
            modules_to_init.push(module.self_id());
        }
    }

    // wrap the modules in an object, write it to the store
    // The call to unwrap() will go away once we remove address owner from Immutable objects.
    let package_object = Object::new_package(modules, ctx.digest());
    state_view.set_create_object_ids(HashSet::from([package_object.id()]));
    state_view.write_object(package_object);

    init_modules(state_view, vm, modules_to_init, ctx, gas_status)
}

/// Modules in module_ids_to_init must have the init method defined
fn init_modules<E: Debug, S: ResourceResolver<Error = E> + ModuleResolver<Error = E> + Storage>(
    state_view: &mut S,
    vm: &MoveVM,
    module_ids_to_init: Vec<ModuleId>,
    ctx: &mut TxContext,
    gas_status: &mut SuiGasStatus,
) -> SuiResult {
    let init_ident = Identifier::new(INIT_FN_NAME.as_str()).unwrap();
    for module_id in module_ids_to_init {
        let args = vec![ctx.to_vec()];

        execute_internal(
            vm,
            state_view,
            &module_id,
            &init_ident,
            Vec::new(),
            args,
            BTreeMap::new(),
            BTreeMap::new(),
            HashMap::new(),
            gas_status,
            ctx,
        )?;
    }
    Ok(())
}

/// Given a list of `modules`, links each module against its
/// dependencies and runs each module with both the Move VM verifier
/// and the Sui verifier.
pub fn verify_and_link<
    E: Debug,
    S: ResourceResolver<Error = E> + ModuleResolver<Error = E> + Storage,
>(
    state_view: &S,
    modules: &[CompiledModule],
    package_id: ObjectID,
    natives: NativeFunctionTable,
    gas_status: &mut SuiGasStatus,
) -> Result<MoveVM, SuiError> {
    // Run the Move bytecode verifier and linker.
    // It is important to do this before running the Sui verifier, since the sui
    // verifier may assume well-formedness conditions enforced by the Move verifier hold
    let vm = MoveVM::new(natives)
        .expect("VM creation only fails if natives are invalid, and we created the natives");
    let mut session = vm.new_session(state_view);
    // TODO(https://github.com/MystenLabs/sui/issues/69): avoid this redundant serialization by exposing VM API that allows us to run the linker directly on `Vec<CompiledModule>`
    let new_module_bytes: Vec<_> = modules
        .iter()
        .map(|m| {
            let mut bytes = Vec::new();
            m.serialize(&mut bytes).unwrap();
            bytes
        })
        .collect();
    session
        .publish_module_bundle(
            new_module_bytes,
            AccountAddress::from(package_id),
            // TODO: publish_module_bundle() currently doesn't charge gas.
            // Do we want to charge there?
            gas_status.get_move_gas_status(),
        )
        .map_err(|e| SuiError::ModulePublishFailure {
            error: e.to_string(),
        })?;

    // run the Sui verifier
    for module in modules.iter() {
        // Run Sui bytecode verifier, which runs some additional checks that assume the Move bytecode verifier has passed.
        verifier::verify_module(module)?;
    }
    Ok(vm)
}

/// Given a list of `modules`, use `ctx` to generate a fresh ID for the new packages.
/// If `is_framework` is true, then the modules can have arbitrary user-defined address,
/// otherwise their addresses must be 0.
/// Mutate each module's self ID to the appropriate fresh ID and update its module handle tables
/// to reflect the new ID's of its dependencies.
/// Returns the newly created package ID.
pub fn generate_package_id(
    modules: &mut [CompiledModule],
    ctx: &mut TxContext,
) -> Result<ObjectID, SuiError> {
    let mut sub_map = BTreeMap::new();
    let package_id = ctx.fresh_id();
    for module in modules.iter() {
        let old_module_id = module.self_id();
        let old_address = *old_module_id.address();
        if old_address != AccountAddress::ZERO {
            let handle = module.module_handle_at(module.self_module_handle_idx);
            let name = module.identifier_at(handle.name);
            return Err(SuiError::ModulePublishFailure {
                error: format!("Publishing module {name} with non-zero address is not allowed"),
            });
        }
        let new_module_id = ModuleId::new(
            AccountAddress::from(package_id),
            old_module_id.name().to_owned(),
        );
        if sub_map.insert(old_module_id, new_module_id).is_some() {
            return Err(SuiError::ModulePublishFailure {
                error: "Publishing two modules with the same ID".to_string(),
            });
        }
    }

    // Safe to unwrap because we checked for duplicate domain entries above, and range entries are fresh ID's
    let rewriter = ModuleHandleRewriter::new(sub_map).unwrap();
    for module in modules.iter_mut() {
        // rewrite module handles to reflect freshly generated ID's
        rewriter.sub_module_ids(module);
    }
    Ok(package_id)
}

type MoveEvent = (Vec<u8>, u64, TypeTag, Vec<u8>);

/// Update `state_view` with the effects of successfully executing a transaction:
/// - Look for each input in `by_value_objects` to determine whether the object was transferred, frozen, or deleted
/// - Update objects passed via a mutable reference in `mutable_refs` to their new values
/// - Process creation of new objects and user-emittd events in `events`
#[allow(clippy::too_many_arguments)]
fn process_successful_execution<
    E: Debug,
    S: ResourceResolver<Error = E> + ModuleResolver<Error = E> + Storage,
>(
    state_view: &mut S,
    mut by_value_objects: BTreeMap<ObjectID, Object>,
    mutable_refs: impl Iterator<Item = (Object, Vec<u8>)>,
    events: Vec<MoveEvent>,
    ctx: &TxContext,
    mut object_owner_map: HashMap<SuiAddress, SuiAddress>,
) -> SuiResult {
    for (mut obj, new_contents) in mutable_refs {
        // update contents and increment sequence number
        obj.data
            .try_as_move_mut()
            .expect("We previously checked that mutable ref inputs are Move objects")
            .update_contents(new_contents);
        state_view.write_object(obj);
    }
    let tx_digest = ctx.digest();
    // newly_generated_ids contains all object IDs generated in this transaction.
    let newly_generated_ids = ctx.recreate_all_ids();
    state_view.set_create_object_ids(newly_generated_ids.clone());
    // process events to identify transfers, freezes
    for e in events {
        let (recipient, event_type, type_, event_bytes) = e;
        let event_type = EventType::try_from(event_type as u8)
            .expect("Safe because event_type is derived from an EventType enum");
        match event_type {
            EventType::TransferToAddress
            | EventType::FreezeObject
            | EventType::TransferToObject
            | EventType::ShareObject => {
                let new_owner = match event_type {
                    EventType::TransferToAddress => {
                        Owner::AddressOwner(SuiAddress::try_from(recipient.as_slice()).unwrap())
                    }
                    EventType::FreezeObject => Owner::SharedImmutable,
                    EventType::TransferToObject => {
                        Owner::ObjectOwner(ObjectID::try_from(recipient.borrow()).unwrap().into())
                    }
                    EventType::ShareObject => Owner::SharedMutable,
                    _ => unreachable!(),
                };
                handle_transfer(
                    new_owner,
                    type_,
                    event_bytes,
                    tx_digest,
                    &mut by_value_objects,
                    state_view,
                    &mut object_owner_map,
                    &newly_generated_ids,
                )
            }
            EventType::DeleteObjectID => {
                // unwrap safe because this event can only be emitted from processing
                // native call delete_id, which guarantees the type of the id.
                let id: VersionedID = bcs::from_bytes(&event_bytes).unwrap();
                let obj_id = id.object_id();
                // We don't care about IDs that are generated in this same transaction
                // but only to be deleted.
                if !newly_generated_ids.contains(obj_id) {
                    if let Some(object) = by_value_objects.remove(id.object_id()) {
                        // This object was in the input, and is being deleted. A normal deletion.
                        debug_assert_eq!(object.version(), id.version());
                        if matches!(object.owner, Owner::ObjectOwner { .. }) {
                            // If an object is owned by another object, we are not allowed to directly delete the child
                            // object because this could lead to a dangling reference of the ownership. Such
                            // dangling reference can never be dropped. To delete this object, one must either first transfer
                            // the child object to an account address, or call through Transfer::delete_child_object(),
                            // which would consume both the child object and the ChildRef ownership reference,
                            // and emit the DeleteChildObject event. These child objects can be safely deleted.
                            return Err(SuiError::DeleteObjectOwnedObject);
                        }
                        state_view.delete_object(obj_id, id.version(), DeleteKind::Normal);
                    } else {
                        // This object wasn't in the input, and is being deleted. It must
                        // be unwrapped in this transaction and then get deleted.
                        // When an object was wrapped at version `v`, we added an record into `parent_sync`
                        // with version `v+1` along with OBJECT_DIGEST_WRAPPED. Now when the object is unwrapped,
                        // it will also have version `v+1`, leading to a violation of the invariant that any
                        // object_id and version pair must be unique. Hence for any object that's just unwrapped,
                        // we force incrementing its version number again to make it `v+2` before writing to the store.
                        state_view.delete_object(
                            obj_id,
                            id.version().increment(),
                            DeleteKind::UnwrapThenDelete,
                        );
                    }
                }
                Ok(())
            }
            EventType::DeleteChildObject => {
                let id_bytes: AccountAddress = bcs::from_bytes(&event_bytes).unwrap();
                let obj_id: ObjectID = id_bytes.into();
                // unwrap safe since to delete a child object, this child object
                // must be passed by value in the input.
                let object = by_value_objects.remove(&obj_id).unwrap();
                state_view.delete_object(&obj_id, object.version(), DeleteKind::Normal);
                Ok(())
            }
            EventType::User => {
                match type_ {
                    TypeTag::Struct(s) => state_view.log_event(Event::new(s, event_bytes)),
                    _ => unreachable!(
                        "Native function emit_event<T> ensures that T is always bound to structs"
                    ),
                };
                Ok(())
            }
        }?;
    }

    // any object left in `by_value_objects` is an input passed by value that was not transferred or frozen.
    // this means that either the object was (1) deleted from the Sui system altogether, or
    // (2) wrapped inside another object that is in the Sui object pool
    for (id, object) in by_value_objects.iter() {
        state_view.delete_object(id, object.version(), DeleteKind::Wrap);
    }

    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn handle_transfer<
    E: Debug,
    S: ResourceResolver<Error = E> + ModuleResolver<Error = E> + Storage,
>(
    recipient: Owner,
    type_: TypeTag,
    contents: Vec<u8>,
    tx_digest: TransactionDigest,
    by_value_objects: &mut BTreeMap<ObjectID, Object>,
    state_view: &mut S,
    object_owner_map: &mut HashMap<SuiAddress, SuiAddress>,
    newly_generated_ids: &HashSet<ObjectID>,
) -> SuiResult {
    match type_ {
        TypeTag::Struct(s_type) => {
            let mut move_obj = MoveObject::new(s_type, contents);
            let old_object = by_value_objects.remove(&move_obj.id());

            #[cfg(debug_assertions)]
            {
                check_transferred_object_invariants(&move_obj, &old_object)
            }

            // increment the object version. note that if the transferred object was
            // freshly created, this means that its version will now be 1.
            // thus, all objects in the global object pool have version > 0
            move_obj.increment_version();
            let obj_id = move_obj.id();
            // A to-be-transferred object can come from 3 sources:
            //   1. Passed in by-value (in `by_value_objects`, i.e. old_object is not none)
            //   2. Created in this transaction (in `newly_generated_ids`)
            //   3. Unwrapped in this transaction
            // The following condition checks if this object was unwrapped in this transaction.
            if old_object.is_none() && !newly_generated_ids.contains(&obj_id) {
                // When an object was wrapped at version `v`, we added an record into `parent_sync`
                // with version `v+1` along with OBJECT_DIGEST_WRAPPED. Now when the object is unwrapped,
                // it will also have version `v+1`, leading to a violation of the invariant that any
                // object_id and version pair must be unique. Hence for any object that's just unwrapped,
                // we force incrementing its version number again to make it `v+2` before writing to the store.
                move_obj.increment_version();
            }
            let obj = Object::new_move(move_obj, recipient, tx_digest);
            if old_object.is_none() {
                // Charge extra gas based on object size if we are creating a new object.
                // TODO: Do we charge extra gas when creating new objects (on top of storage write cost)?
            }
            let obj_address: SuiAddress = obj_id.into();
            object_owner_map.remove(&obj_address);
            if let Owner::ObjectOwner(new_owner) = recipient {
                // Below we check whether the transfer introduced any circular ownership.
                // We know that for any mutable object, all its ancenstors (if it was owned by another object)
                // must be in the input as well. Prior to this we have recorded the original ownership mapping
                // in object_owner_map. For any new transfer, we trace the new owner through the ownership
                // chain to see if a cycle is detected.
                // TODO: Set a constant upper bound to the depth of the new ownership chain.
                let mut parent = new_owner;
                while parent != obj_address && object_owner_map.contains_key(&parent) {
                    parent = *object_owner_map.get(&parent).unwrap();
                }
                if parent == obj_address {
                    return Err(SuiError::CircularObjectOwnership);
                }
                object_owner_map.insert(obj_address, new_owner);
            }

            state_view.write_object(obj);
        }
        _ => unreachable!("Only structs can be transferred"),
    }
    Ok(())
}

#[cfg(debug_assertions)]
fn check_transferred_object_invariants(new_object: &MoveObject, old_object: &Option<Object>) {
    if let Some(o) = old_object {
        // check consistency between the transferred object `new_object` and the tx input `o`
        // specifically, the object id, type, and version should be unchanged
        let m = o.data.try_as_move().unwrap();
        debug_assert_eq!(m.id(), new_object.id());
        debug_assert_eq!(m.version(), new_object.version());
        debug_assert_eq!(m.type_, new_object.type_);
    }
}

pub struct TypeCheckSuccess {
    pub module_id: ModuleId,
    pub args: Vec<Vec<u8>>,
    pub by_value_objects: BTreeMap<ObjectID, Object>,
    pub mutable_ref_objects: BTreeMap<LocalIndex, Object>,
}

/// - Check that `package_object`, `module` and `function` are valid
/// - Check that the the signature of `function` is well-typed w.r.t `type_args`, `object_args`, and `pure_args`
/// - Return the ID of the resolved module, a vector of BCS encoded arguments to pass to the VM, and a partitioning
/// of the input objects into objects passed by value vs by mutable reference
pub fn resolve_and_type_check(
    module: &CompiledModule,
    function: &Identifier,
    type_args: &[TypeTag],
    object_args: Vec<Object>,
    mut pure_args: Vec<Vec<u8>>,
) -> Result<TypeCheckSuccess, SuiError> {
    // Resolve the function we are calling
    let function_str = function.as_ident_str();
    let module_id = module.self_id();
    let fdef_opt = module.function_defs.iter().find(|fdef| {
        module.identifier_at(module.function_handle_at(fdef.function).name) == function_str
    });
    let fdef = match fdef_opt {
        Some(fdef) => fdef,
        None => {
            return Err(SuiError::FunctionNotFound {
                error: format!(
                    "Could not resolve function '{}' in module {}",
                    function, &module_id,
                ),
            })
        }
    };
    let fhandle = module.function_handle_at(fdef.function);

    // check arity of type and value arguments
    if fhandle.type_parameters.len() != type_args.len() {
        return Err(SuiError::InvalidFunctionSignature {
            error: format!(
                "Expected {:?} type arguments, but found {:?}",
                fhandle.type_parameters.len(),
                type_args.len()
            ),
        });
    }

    // total number of args is |objects| + |pure_args| + 1 for the the `TxContext` object
    let num_args = object_args.len() + pure_args.len() + 1;
    let parameters = &module.signature_at(fhandle.parameters).0;
    if parameters.len() != num_args {
        return Err(SuiError::InvalidFunctionSignature {
            error: format!(
                "Expected {:?} arguments calling function '{}', but found {:?}",
                parameters.len(),
                function,
                num_args
            ),
        });
    }

    entry_points_verifier::verify_entry_function(module, fdef, type_args)?;

    // type check object arguments passed in by value and by reference
    let mut args = Vec::new();
    let mut mutable_ref_objects = BTreeMap::new();
    let mut by_value_objects = BTreeMap::new();
    #[cfg(debug_assertions)]
    let mut num_immutable_objects = 0;
    #[cfg(debug_assertions)]
    let num_objects = object_args.len();

    for (idx, object) in object_args.into_iter().enumerate() {
        let param_type = &parameters[idx];
        let move_object = match &object.data {
            Data::Move(m) => m,
            Data::Package(_) => {
                let error = format!(
                    "Found module argument, but function expects {:?}",
                    param_type
                );
                return Err(SuiError::TypeError { error });
            }
        };
        args.push(move_object.contents().to_vec());
        // check that m.type_ matches the parameter types of the function
        let inner_param_type = match &param_type {
            SignatureToken::MutableReference(inner_t) => {
                if object.is_read_only() {
                    let error = format!(
                        "Argument {} is expected to be mutable, immutable object found",
                        idx
                    );
                    return Err(SuiError::TypeError { error });
                }
                &**inner_t
            }
            SignatureToken::Reference(inner_t) => {
                #[cfg(debug_assertions)]
                {
                    num_immutable_objects += 1;
                }
                &**inner_t
            }
            t @ SignatureToken::Struct(_)
            | t @ SignatureToken::StructInstantiation(_, _)
            | t @ SignatureToken::TypeParameter(_) => {
                if object.is_shared() {
                    // Forbid passing shared (both mutable and immutable) object by value.
                    // This ensures that shared object cannot be transferred, deleted or wrapped.
                    return Err(SuiError::TypeError {
                        error: format!(
                            "Shared object cannot be passed by-value, found in argument {}",
                            idx
                        ),
                    });
                }
                t
            }
            t => {
                return Err(SuiError::TypeError {
                    error: format!(
                        "Found object argument {}, but function expects {:?}",
                        move_object.type_, t
                    ),
                })
            }
        };
        type_check_struct(module, type_args, &move_object.type_, inner_param_type)?;
        match &param_type {
            SignatureToken::MutableReference(_) => {
                let _prev = mutable_ref_objects.insert(idx as LocalIndex, object);
                debug_assert!(_prev.is_none());
            }
            SignatureToken::Reference(_) => (),
            _ => {
                let _prev = by_value_objects.insert(object.id(), object);
                // should always pass due to earlier "no duplicate ID's" check
                debug_assert!(_prev.is_none());
            }
        }
    }

    debug_assert!(
        by_value_objects.len() + mutable_ref_objects.len() + num_immutable_objects == num_objects
    );
    // verify_entry_function ensures that pure_args are all primitives
    args.append(&mut pure_args);

    Ok(TypeCheckSuccess {
        module_id,
        args,
        by_value_objects,
        mutable_ref_objects,
    })
}

fn type_check_struct(
    module: &CompiledModule,
    function_type_arguments: &[TypeTag],
    arg_type: &StructTag,
    param_type: &SignatureToken,
) -> Result<(), SuiError> {
    if !struct_tag_equals_sig_token(module, function_type_arguments, arg_type, param_type) {
        Err(SuiError::TypeError {
            error: format!(
                "Expected argument of type {}, but found type {}",
                sui_verifier::format_signature_token(module, param_type),
                arg_type
            ),
        })
    } else {
        Ok(())
    }
}

fn type_tag_equals_sig_token(
    module: &CompiledModule,
    function_type_arguments: &[TypeTag],
    arg_type: &TypeTag,
    param_type: &SignatureToken,
) -> bool {
    match (arg_type, param_type) {
        (TypeTag::Bool, SignatureToken::Bool)
        | (TypeTag::U8, SignatureToken::U8)
        | (TypeTag::U64, SignatureToken::U64)
        | (TypeTag::U128, SignatureToken::U128)
        | (TypeTag::Address, SignatureToken::Address)
        | (TypeTag::Signer, SignatureToken::Signer) => true,

        (TypeTag::Vector(inner_arg_type), SignatureToken::Vector(inner_param_type)) => {
            type_tag_equals_sig_token(
                module,
                function_type_arguments,
                inner_arg_type,
                inner_param_type,
            )
        }

        (TypeTag::Struct(arg_struct), SignatureToken::Struct(_))
        | (TypeTag::Struct(arg_struct), SignatureToken::StructInstantiation(_, _)) => {
            struct_tag_equals_sig_token(module, function_type_arguments, arg_struct, param_type)
        }

        (_, SignatureToken::TypeParameter(idx)) => {
            arg_type == &function_type_arguments[*idx as usize]
        }
        _ => false,
    }
}

fn struct_tag_equals_sig_token(
    module: &CompiledModule,
    function_type_arguments: &[TypeTag],
    arg_type: &StructTag,
    param_type: &SignatureToken,
) -> bool {
    match param_type {
        SignatureToken::Struct(idx) => {
            struct_tag_equals_struct_inst(module, function_type_arguments, arg_type, *idx, &[])
        }
        SignatureToken::StructInstantiation(idx, args) => {
            struct_tag_equals_struct_inst(module, function_type_arguments, arg_type, *idx, args)
        }
        _ => false,
    }
}

fn struct_tag_equals_struct_inst(
    module: &CompiledModule,
    function_type_arguments: &[TypeTag],
    arg_type: &StructTag,
    param_type: StructHandleIndex,
    param_type_arguments: &[SignatureToken],
) -> bool {
    let (address, module_name, struct_name) = sui_verifier::resolve_struct(module, param_type);

    // same address
    &arg_type.address == address
    // same module
        && arg_type.module.as_ident_str() == module_name
        // same struct name
        && arg_type.name.as_ident_str() == struct_name
        // same type parameters
        && arg_type.type_params.len() == param_type_arguments.len()
        && arg_type.type_params.iter().zip(param_type_arguments).all(
            |(arg_type_arg, param_type_arg)| {
                type_tag_equals_sig_token(
                    module,
                    function_type_arguments,
                    arg_type_arg,
                    param_type_arg,
                )
            },
        )
}
