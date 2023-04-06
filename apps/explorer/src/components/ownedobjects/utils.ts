// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

import {type SuiMoveNormalizedType} from '@mysten/sui.js';

export enum FieldTypeValue {
    ADDRESS = 'address',
    MODULE = 'module',
    NAME = 'name',
}

type TypeReference = {
    address: string;
    module: string;
    name: string;
    typeArguments?: SuiMoveNormalizedType[];
} | string | number;

export function extractSerializationType(type: SuiMoveNormalizedType): TypeReference {
    if (typeof type === 'string') {
        return type;
    }

    if ('TypeParameter' in type) {
        return type.TypeParameter;
    }

    if ('Reference' in type) {
        return extractSerializationType(type.Reference);
    }

    if ('MutableReference' in type) {
        return extractSerializationType(type.MutableReference);
    }

    if ('Vector' in type) {
        return extractSerializationType(type.Vector);
    }

    if ('Struct' in type) {
        const theType = type.Struct;
        const theTypeArgs = theType.typeArguments;
        
        if (theTypeArgs && theTypeArgs.length > 0) {
            return extractSerializationType(theTypeArgs[0]);
        }

        return type.Struct;
    }
    
    return type;
}


export function getFieldTypeValue(normalizedType: TypeReference , structFieldName = FieldTypeValue.NAME)  {
    if(typeof normalizedType === 'string' || typeof normalizedType === 'number') {
        return normalizedType;
    }
    return normalizedType[structFieldName]
}