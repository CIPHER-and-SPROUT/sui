// Copyright (c) Mysten Labs
// SPDX-License-Identifier: Apache-2.0

use move_binary_format::CompiledModule;
use move_core_types::{account_address::AccountAddress, ident_str};
use move_package::BuildConfig;
use num_enum::TryFromPrimitive;
use std::collections::HashSet;
use std::path::Path;
use sui_types::error::{SuiError, SuiResult};
use sui_verifier::verifier as sui_bytecode_verifier;

#[cfg(test)]
use std::path::PathBuf;

pub mod natives;

// Move unit tests will halt after executing this many steps. This is a protection to avoid divergence
const MAX_UNIT_TEST_INSTRUCTIONS: u64 = 100_000;

pub const DEFAULT_FRAMEWORK_PATH: &str = env!("CARGO_MANIFEST_DIR");

#[derive(TryFromPrimitive)]
#[repr(u8)]
pub enum EventType {
    /// System event: transfer between addresses
    TransferToAddress,
    /// System event: freeze, then transfer between addresses
    TransferToAddressAndFreeze,
    /// System event: transfer object to another object
    TransferToObject,
    /// System event: an object ID is deleted. This does not necessarily
    /// mean an object is being deleted. However whenever an object is being
    /// deleted, the object ID must be deleted and this event will be
    /// emitted.
    DeleteObjectID,
    /// User-defined event
    User,
}

pub fn get_sui_framework_modules(lib_dir: &Path) -> SuiResult<Vec<CompiledModule>> {
    let modules = build_framework(lib_dir)?;
    verify_modules(&modules)?;
    Ok(modules)
}

pub fn get_move_stdlib_modules(lib_dir: &Path) -> SuiResult<Vec<CompiledModule>> {
    let denylist = vec![
        ident_str!("Capability").to_owned(),
        ident_str!("Event").to_owned(),
        ident_str!("GUID").to_owned(),
    ];
    let modules: Vec<CompiledModule> = build_framework(lib_dir)?
        .into_iter()
        .filter(|m| !denylist.contains(&m.self_id().name().to_owned()))
        .collect();
    verify_modules(&modules)?;
    Ok(modules)
}

/// Given a `path` and a `build_config`, build the package in that path and return the compiled modules as Vec<Vec<u8>>.
/// This is useful for when publishing
/// If we are building the FastX framework, `is_framework` will be true;
/// Otherwise `is_framework` should be false (e.g. calling from client).
pub fn build_move_package_to_bytes(path: &Path) -> Result<Vec<Vec<u8>>, SuiError> {
    build_move_package(
        path,
        BuildConfig {
            ..Default::default()
        },
        false,
    )
    .map(|mods| {
        mods.iter()
            .map(|m| {
                let mut bytes = Vec::new();
                m.serialize(&mut bytes).unwrap();
                bytes
            })
            .collect::<Vec<_>>()
    })
}

/// Given a `path` and a `build_config`, build the package in that path.
/// If we are building the FastX framework, `is_framework` will be true;
/// Otherwise `is_framework` should be false (e.g. calling from client).
pub fn build_move_package(
    path: &Path,
    build_config: BuildConfig,
    is_framework: bool,
) -> SuiResult<Vec<CompiledModule>> {
    match build_config.compile_package(path, &mut Vec::new()) {
        Err(error) => Err(SuiError::ModuleBuildFailure {
            error: error.to_string(),
        }),
        Ok(package) => {
            let compiled_modules = package.compiled_modules();
            if !is_framework {
                if let Some(m) = compiled_modules
                    .iter_modules()
                    .iter()
                    .find(|m| m.self_id().address() != &AccountAddress::ZERO)
                {
                    return Err(SuiError::ModulePublishFailure {
                        error: format!(
                            "Modules must all have 0x0 as their addresses. Violated by module {:?}",
                            m.self_id()
                        ),
                    });
                }
            }
            // Collect all module names from the current package to be published.
            // For each transitive dependent module, if they are not to be published,
            // they must have a non-zero address (meaning they are already published on-chain).
            // TODO: Shall we also check if they are really on-chain in the future?
            let self_modules: HashSet<String> = compiled_modules
                .iter_modules()
                .iter()
                .map(|m| m.self_id().name().to_string())
                .collect();
            if let Some(m) = package
                .transitive_compiled_modules()
                .iter_modules()
                .iter()
                .find(|m| {
                    !self_modules.contains(m.self_id().name().as_str())
                        && m.self_id().address() == &AccountAddress::ZERO
                })
            {
                return Err(SuiError::ModulePublishFailure { error: format!("Denpendent modules must have been published on-chain with non-0 addresses, unlike module {:?}", m.self_id()) });
            }
            Ok(package
                .transitive_compiled_modules()
                .compute_dependency_graph()
                .compute_topological_order()
                .unwrap()
                .filter(|m| self_modules.contains(m.self_id().name().as_str()))
                .cloned()
                .collect())
        }
    }
}

pub fn build_and_verify_user_package(path: &Path, dev_mode: bool) -> SuiResult {
    let build_config = BuildConfig {
        dev_mode,
        ..Default::default()
    };
    let modules = build_move_package(path, build_config, false)?;
    verify_modules(&modules)
}

fn verify_modules(modules: &[CompiledModule]) -> SuiResult {
    for m in modules {
        move_bytecode_verifier::verify_module(m).map_err(|err| {
            SuiError::ModuleVerificationFailure {
                error: err.to_string(),
            }
        })?;
        sui_bytecode_verifier::verify_module(m)?;
    }
    Ok(())
    // TODO(https://github.com/MystenLabs/fastnft/issues/69): Run Move linker
}

fn build_framework(framework_dir: &Path) -> SuiResult<Vec<CompiledModule>> {
    let build_config = BuildConfig {
        dev_mode: false,
        ..Default::default()
    };
    build_move_package(framework_dir, build_config, true)
}

pub fn run_move_unit_tests(path: &Path) -> SuiResult {
    use move_cli::package::cli::{self, UnitTestResult};
    use sui_types::{MOVE_STDLIB_ADDRESS, SUI_FRAMEWORK_ADDRESS};

    use move_unit_test::UnitTestingConfig;

    let result = cli::run_move_unit_tests(
        path,
        BuildConfig::default(),
        UnitTestingConfig::default_with_bound(Some(MAX_UNIT_TEST_INSTRUCTIONS)),
        natives::all_natives(MOVE_STDLIB_ADDRESS, SUI_FRAMEWORK_ADDRESS),
        /* compute_coverage */ false,
    )
    .map_err(|err| SuiError::MoveUnitTestFailure {
        error: err.to_string(),
    })?;
    if result == UnitTestResult::Failure {
        Err(SuiError::MoveUnitTestFailure {
            error: "Test failed".to_string(),
        })
    } else {
        Ok(())
    }
}

#[test]
fn run_framework_move_unit_tests() {
    get_sui_framework_modules(&PathBuf::from(DEFAULT_FRAMEWORK_PATH));
    run_move_unit_tests(Path::new(env!("CARGO_MANIFEST_DIR"))).unwrap();
}

#[test]
fn run_examples_move_unit_tests() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../examples");
    build_and_verify_user_package(&path, true).unwrap();
    run_move_unit_tests(&path).unwrap();
}
