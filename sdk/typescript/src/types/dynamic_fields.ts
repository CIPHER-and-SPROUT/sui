// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

import {
  any,
  array,
  boolean,
  Infer,
  literal,
  number,
  object,
  string,
  union,
} from 'superstruct';
import { ObjectId } from './common';

export const DynamicFieldType = union([
  literal('DynamicField'),
  literal('DynamicObject'),
]);
export type DynamicFieldType = Infer<typeof DynamicFieldType>;

export const DynamicFieldName = object({
  type: string(),
  value: any(),
});
export type DynamicFieldName = Infer<typeof DynamicFieldName>;

export const DynamicFieldInfo = object({
  name: union([DynamicFieldName, string()]),
  type: DynamicFieldType,
  objectType: string(),
  objectId: ObjectId,
  version: number(),
  digest: string(),
});
export type DynamicFieldInfo = Infer<typeof DynamicFieldInfo>;

export const DynamicFieldPage = object({
  data: array(DynamicFieldInfo),
  nextCursor: union([ObjectId, literal(null)]),
  hasNextPage: boolean(),
});
export type DynamicFieldPage = Infer<typeof DynamicFieldPage>;
