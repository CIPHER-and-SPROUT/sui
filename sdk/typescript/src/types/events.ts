// Copyright (c) 2022, Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

import { SuiAddress, ObjectOwner, TransactionDigest } from "./common";
import { ObjectId, SequenceNumber } from "./objects";
import { SuiJsonValue } from "./transactions";


// event types mirror those in "sui-json-rpc-types/lib.rs"
export type MoveEvent = {
    packageId: ObjectId;
    transactionModule: string;
    sender: SuiAddress;
    type: string;
    fields: { [key: string]: any; }; // TODO - better type
    bcs: string;
};

export type PublishEvent = {
    sender: SuiAddress;
    packageId: ObjectId;
};

export type TransferObjectEvent = {
    packageId: ObjectId;
    transactionModule: string;
    sender: SuiAddress;
    recipient: ObjectOwner;
    objectId: ObjectId;
    version: SequenceNumber;
    type: string; // TODO - better type
};

export type DeleteObjectEvent = {
    packageId: ObjectId;
    transactionModule: string;
    sender: SuiAddress;
    objectId: ObjectId;
};

export type NewObjectEvent = {
    packageId: ObjectId;
    transactionModule: string;
    sender: SuiAddress;
    recipient: ObjectOwner;
    objectId: ObjectId;
};

export type SuiEvent =
    | { moveEvent: MoveEvent }
    | { publish: PublishEvent }
    | { transferObject: TransferObjectEvent }
    | { deleteObject: DeleteObjectEvent }
    | { newObject: NewObjectEvent }
    | { epochChange: bigint }
    | { checkpoint: bigint };

export type MoveEventField = {
    path: string,
    value: SuiJsonValue
}

// mirrors sui_framework::EventType
export type EventType =
    | "TransferToAddress"
    | "TransferToObject"
    | "FreezeObject"
    | "ShareObject"
    | "DeleteObjectID"
    | "User";

// mirrors sui_json_rpc_types::SuiEventFilter
export type SuiEventFilter =
    | { "package" : ObjectId }
    | { "module" : string }
    | { "moveEventType" : string }
    | { "moveEventField" : MoveEventField }
    | { "senderAddress" : SuiAddress }
    | { "eventType" : EventType }
    | { "All" : SuiEventFilter[] }
    | { "Any" : SuiEventFilter[] }
    | { "And" : [SuiEventFilter, SuiEventFilter] }
    | { "Or" : [SuiEventFilter, SuiEventFilter] };

export type SuiEventEnvelope = {
    timestamp: bigint,
    txDigest: TransactionDigest,
    event: SuiEvent
}

export type SubscriptionId = number;