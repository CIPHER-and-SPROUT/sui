// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

import {
  array,
  Infer,
  object,
  string,
  union,
  boolean,
  define,
  number,
  literal,
  record,
  is,
} from 'superstruct';

export type SuiMoveFunctionArgTypesResponse = Infer<
  typeof SuiMoveFunctionArgType
>[];

export const SuiMoveFunctionArgType = union([
  string(),
  object({ Object: string() }),
]);

export const SuiMoveFunctionArgTypes = array(SuiMoveFunctionArgType);
export type SuiMoveFunctionArgTypes = Infer<typeof SuiMoveFunctionArgTypes>;

export const SuiMoveModuleId = object({
  address: string(),
  name: string(),
});
export type SuiMoveModuleId = Infer<typeof SuiMoveModuleId>;

export const SuiMoveVisibility = union([
  literal('Private'),
  literal('Public'),
  literal('Friend'),
]);
export type SuiMoveVisibility = Infer<typeof SuiMoveVisibility>;

export const SuiMoveAbilitySet = object({
  abilities: array(string()),
});
export type SuiMoveAbilitySet = Infer<typeof SuiMoveAbilitySet>;

export const SuiMoveStructTypeParameter = object({
  constraints: SuiMoveAbilitySet,
  is_phantom: boolean(),
});
export type SuiMoveStructTypeParameter = Infer<
  typeof SuiMoveStructTypeParameter
>;

export const SuiMoveNormalizedTypeParameterType = object({
  TypeParameter: number(),
});
export type SuiMoveNormalizedTypeParameterType = Infer<
  typeof SuiMoveNormalizedTypeParameterType
>;

export type SuiMoveNormalizedType =
  | string
  | SuiMoveNormalizedTypeParameterType
  | { Reference: SuiMoveNormalizedType }
  | { MutableReference: SuiMoveNormalizedType }
  | { Vector: SuiMoveNormalizedType }
  | SuiMoveNormalizedStructType;

function isSuiMoveNormalizedType(
  value: unknown
): value is SuiMoveNormalizedType {
  if (!value) return false;
  if (typeof value === 'string') return true;
  if (is(value, SuiMoveNormalizedTypeParameterType)) return true;
  if (isSuiMoveNormalizedStructType(value)) return true;
  if (typeof value !== 'object') return false;
  if ('Reference' in value && is(value.Reference, SuiMoveNormalizedType))
    return true;
  if (
    'MutableReference' in value &&
    is(value.MutableReference, SuiMoveNormalizedType)
  )
    return true;
  if ('Vector' in value && is(value.Vector, SuiMoveNormalizedType)) return true;
  return false;
}

export const SuiMoveNormalizedType = define(
  'SuiMoveNormalizedType',
  isSuiMoveNormalizedType
);

export type SuiMoveNormalizedStructType = {
  Struct: {
    address: string;
    module: string;
    name: string;
    type_arguments: SuiMoveNormalizedType[];
  };
};

function isSuiMoveNormalizedStructType(
  value: unknown
): value is SuiMoveNormalizedStructType {
  if (!value || typeof value !== 'object') return false;
  if (!('Struct' in value) || !value.Struct || typeof value.Struct !== 'object')
    return false;

  const structProperties = value.Struct as Record<string, unknown>;
  if (
    typeof structProperties.address !== 'string' ||
    typeof structProperties.module !== 'string' ||
    typeof structProperties.name !== 'string' ||
    !Array.isArray(structProperties.type_arguments) ||
    !structProperties.type_arguments.every((value) =>
      isSuiMoveNormalizedType(value)
    )
  ) {
    return false;
  }

  return true;
}

// NOTE: This type is recursive, so we need to manually implement it:
export const SuiMoveNormalizedStructType = define(
  'SuiMoveNormalizedStructType',
  isSuiMoveNormalizedStructType
);

export const SuiMoveNormalizedFunction = object({
  visibility: SuiMoveVisibility,
  is_entry: boolean(),
  type_parameters: array(SuiMoveAbilitySet),
  parameters: array(SuiMoveNormalizedType),
  return_: array(SuiMoveNormalizedType),
});
export type SuiMoveNormalizedFunction = Infer<typeof SuiMoveNormalizedFunction>;

export const SuiMoveNormalizedField = object({
  name: string(),
  type_: SuiMoveNormalizedType,
});
export type SuiMoveNormalizedField = Infer<typeof SuiMoveNormalizedField>;

export const SuiMoveNormalizedStruct = object({
  abilities: SuiMoveAbilitySet,
  type_parameters: array(SuiMoveStructTypeParameter),
  fields: array(SuiMoveNormalizedField),
});
export type SuiMoveNormalizedStruct = Infer<typeof SuiMoveNormalizedStruct>;

export const SuiMoveNormalizedModule = object({
  file_format_version: number(),
  address: string(),
  name: string(),
  friends: array(SuiMoveModuleId),
  structs: record(string(), SuiMoveNormalizedStruct),
  exposed_functions: record(string(), SuiMoveNormalizedFunction),
});
export type SuiMoveNormalizedModule = Infer<typeof SuiMoveNormalizedModule>;

export const SuiMoveNormalizedModules = record(
  string(),
  SuiMoveNormalizedModule
);
export type SuiMoveNormalizedModules = Infer<typeof SuiMoveNormalizedModules>;

export function extractMutableReference(
  normalizedType: SuiMoveNormalizedType
): SuiMoveNormalizedType | undefined {
  return typeof normalizedType === 'object' &&
    'MutableReference' in normalizedType
    ? normalizedType.MutableReference
    : undefined;
}

export function extractReference(
  normalizedType: SuiMoveNormalizedType
): SuiMoveNormalizedType | undefined {
  return typeof normalizedType === 'object' && 'Reference' in normalizedType
    ? normalizedType.Reference
    : undefined;
}

export function extractStructTag(
  normalizedType: SuiMoveNormalizedType
): SuiMoveNormalizedStructType | undefined {
  if (typeof normalizedType === 'object' && 'Struct' in normalizedType) {
    return normalizedType;
  }

  const ref = extractReference(normalizedType);
  const mutRef = extractMutableReference(normalizedType);

  if (typeof ref === 'object' && 'Struct' in ref) {
    return ref;
  }

  if (typeof mutRef === 'object' && 'Struct' in mutRef) {
    return mutRef;
  }
  return undefined;
}
