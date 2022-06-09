// Copyright (c) 2022, Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

import {
    getExecutionStatusType,
    getTotalGasUsed,
    getTransactions,
    getTransactionDigest,
    getTransactionKindName,
    getTransferCoinTransaction,
    JsonRpcProvider,
} from '@mysten/sui.js';

import { getEndpoint, Network } from './rpcSetting';

import type {
    CertifiedTransaction,
    TransactionEffectsResponse,
    GetTxnDigestsResponse,
} from '@mysten/sui.js';

// TODO: Remove these types with SDK types
export type AddressBytes = number[];
export type AddressOwner = { AddressOwner: AddressBytes };

export type AnyVec = { vec: any[] };
export type JsonBytes = { bytes: number[] };

export { Network, getEndpoint };

export const DefaultRpcClient = (network: Network | string) =>
    new JsonRpcProvider(getEndpoint(network));

const deduplicate = (results: [number, string][]) =>
    results
        .map((result) => result[1])
        .filter((value, index, self) => self.indexOf(value) === index);

export const getDataOnTxDigests = (
    network: Network | string,
    transactions: GetTxnDigestsResponse
) =>
    DefaultRpcClient(network)
        .getTransactionWithEffectsBatch(deduplicate(transactions))
        .then((txEffs: TransactionEffectsResponse[]) => {
            return (
                txEffs
                    .map((txEff, i) => {
                        const [seq, digest] = transactions.filter(
                            (transactionId) =>
                                transactionId[1] ===
                                getTransactionDigest(txEff.certificate)
                        )[0];
                        const res: CertifiedTransaction = txEff.certificate;
                        // TODO: handle multiple transactions
                        const txns = getTransactions(res);
                        if (txns.length > 1) {
                            console.error(
                                'Handling multiple transactions is not yet supported',
                                txEff
                            );
                            return null;
                        }
                        const txn = txns[0];
                        const txKind = getTransactionKindName(txn);
                        const recipient =
                            getTransferCoinTransaction(txn)?.recipient;

                        return {
                            seq,
                            txId: digest,
                            status: getExecutionStatusType(txEff),
                            txGas: getTotalGasUsed(txEff),
                            kind: txKind,
                            From: res.data.sender,
                            ...(recipient
                                ? {
                                      To: recipient,
                                  }
                                : {}),
                        };
                    })
                    // Remove failed transactions and sort by sequence number
                    .filter((itm) => itm)
                    .sort((a, b) => b!.seq - a!.seq)
            );
        });
