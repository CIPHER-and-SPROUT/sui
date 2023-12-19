// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use async_graphql::*;

use move_core_types::annotated_value::{MoveStruct, MoveValue};
use sui_types::collection_types::VecMap;

use crate::error::Error;
use sui_json_rpc_types::SuiMoveValue;

/// The set of named templates defined on-chain for the type of this object,
/// to be handled off-chain. The server substitutes data from the object
/// into these templates to generate a display string per template.
#[derive(Debug, SimpleObject)]
pub(crate) struct DisplayEntry {
    /// The identifier for a particular template string of the Display object.
    pub key: String,
    /// The template string for the key with placeholder values substituted.
    pub value: Option<String>,
    /// An error string describing why the template could not be rendered.
    pub error: Option<String>,
}

#[derive(thiserror::Error, Debug)]
pub(crate) enum DisplayRenderError {
    #[error("Display template value cannot be empty")]
    TemplateValueEmpty,
    #[error("Display template value of {0} exceeds maximum depth of {1}")]
    ExceedsLookupDepth(usize, u64),
    #[error("Vector of name {0} is not supported as a Display value")]
    Vector(String),
    #[error("Field '{0}' not found")]
    FieldNotFound(String),
    #[error("Unexpected MoveValue")]
    UnexpectedMoveValue,
}

impl DisplayEntry {
    pub(crate) fn create_value(key: String, value: String) -> Self {
        Self {
            key,
            value: Some(value),
            error: None,
        }
    }

    pub(crate) fn create_error(key: String, error: String) -> Self {
        Self {
            key,
            value: None,
            error: Some(error),
        }
    }
}

pub(crate) fn get_rendered_fields(
    fields: VecMap<String, String>,
    move_struct: &MoveStruct,
) -> Result<Vec<DisplayEntry>, Error> {
    let mut rendered_fields: Vec<DisplayEntry> = vec![];

    for entry in fields.contents.iter() {
        let rendered_value = match parse_template(&entry.value, move_struct) {
            Ok(value) => DisplayEntry::create_value(entry.key.clone(), value),
            Err(e) => DisplayEntry::create_error(entry.key.clone(), e.to_string()),
        };
        rendered_fields.push(rendered_value);
    }

    Ok(rendered_fields)
}

/// Handles the PART of the grammar, defined as:
/// PART   ::= '{' CHAIN '}'
///          | '\{' | '\}'
///          | [:utf8:]
/// Defers resolution down to the IDENT to get_value_from_move_struct,
/// and substitutes the result into the PART template.
fn parse_template(template: &str, move_struct: &MoveStruct) -> Result<String, DisplayRenderError> {
    let mut output = template.to_string();
    let mut var_name = String::new();
    let mut in_braces = false;
    let mut escaped = false;

    for ch in template.chars() {
        match ch {
            '\\' => {
                escaped = true;
                continue;
            }
            '{' if !escaped => {
                in_braces = true;
                var_name.clear();
            }
            '}' if !escaped => {
                in_braces = false;
                let value = get_value_from_move_struct(move_struct, &var_name)?;
                output = output.replace(&format!("{{{}}}", var_name), &value.to_string());
            }
            _ if !escaped => {
                if in_braces {
                    var_name.push(ch);
                }
            }
            _ => {}
        }
        escaped = false;
    }

    Ok(output.replace('\\', ""))
}

/// Handles the CHAIN and IDENT of the grammar, defined as:
/// CHAIN  ::= IDENT | CHAIN '.' IDENT
/// IDENT  ::= /* Move identifier */
pub(crate) fn get_value_from_move_struct(
    move_struct: &MoveStruct,
    var_name: &str,
) -> Result<String, DisplayRenderError> {
    let parts: Vec<&str> = var_name.split('.').collect();
    if parts.is_empty() {
        return Err(DisplayRenderError::TemplateValueEmpty);
    }
    // todo: 10 is a carry-over from the sui-json-rpc implementation
    // we should introduce this as a new limit on the config
    if parts.len() > 10 {
        return Err(DisplayRenderError::ExceedsLookupDepth(parts.len(), 10));
    }

    // update this as we iterate through the parts
    let start_value = &MoveValue::Struct(move_struct.clone());

    let result = parts
        .iter()
        .try_fold(start_value, |current_value, part| match current_value {
            MoveValue::Struct(s) => s
                .fields
                .iter()
                .find_map(|(id, value)| {
                    if id.to_string() == *part {
                        Some(value)
                    } else {
                        None
                    }
                })
                .ok_or_else(|| DisplayRenderError::FieldNotFound(part.to_string())),
            _ => Err(DisplayRenderError::UnexpectedMoveValue),
        })?;

    // TODO: move off dependency on SuiMoveValue
    let sui_move_value: SuiMoveValue = result.clone().into();

    match sui_move_value {
        SuiMoveValue::Option(move_option) => match move_option.as_ref() {
            Some(move_value) => Ok(move_value.to_string()),
            None => Ok("".to_string()),
        },
        SuiMoveValue::Vector(_) => Err(DisplayRenderError::Vector(var_name.to_string())),
        _ => Ok(sui_move_value.to_string()),
    }
}
