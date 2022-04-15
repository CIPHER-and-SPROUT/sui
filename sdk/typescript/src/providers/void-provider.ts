// Copyright (c) 2022, Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

import {
  Provider,
  ObjectRef,
  SignedTransaction,
  TransactionResponse,
} from './provider';

export class VoidProvider extends Provider {
  // Objects
  async getObjectRefs(_address: string): Promise<ObjectRef[]> {
    throw this.newError('getObjectRefs');
  }

  // Transactions
  async executeTransaction(
    _txn: SignedTransaction
  ): Promise<TransactionResponse> {
    throw this.newError('executeTransaction');
  }

  private newError(operation: string): Error {
    return new Error(`Please use a valid provider for ${operation}`);
  }
}
