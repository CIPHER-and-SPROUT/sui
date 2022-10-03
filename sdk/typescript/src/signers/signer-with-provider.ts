// Copyright (c) 2022, Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

import { JsonRpcProvider } from '../providers/json-rpc-provider';
import { Provider } from '../providers/provider';
import { VoidProvider } from '../providers/void-provider';
import { Base64DataBuffer } from '../serialization/base64';
import {
  ExecuteTransactionRequestType,
  SuiAddress,
  SuiExecuteTransactionResponse,
  SuiTransactionResponse,
} from '../types';
import { SignaturePubkeyPair, Signer } from './signer';
import { RpcTxnDataSerializer } from './txn-data-serializers/rpc-txn-data-serializer';
import {
  MoveCallTransaction,
  MergeCoinTransaction,
  PayTransaction,
  SplitCoinTransaction,
  TransferObjectTransaction,
  TransferSuiTransaction,
  TxnDataSerializer,
  PublishTransaction,
} from './txn-data-serializers/txn-data-serializer';

///////////////////////////////
// Exported Abstracts
export abstract class SignerWithProvider implements Signer {
  readonly provider: Provider;
  readonly serializer: TxnDataSerializer;

  ///////////////////
  // Sub-classes MUST implement these

  // Returns the checksum address
  abstract getAddress(): Promise<SuiAddress>;

  /**
   * Returns the signature for the data and the public key of the signer
   */
  abstract signData(data: Base64DataBuffer): Promise<SignaturePubkeyPair>;

  // Returns a new instance of the Signer, connected to provider.
  // This MAY throw if changing providers is not supported.
  abstract connect(provider: Provider): SignerWithProvider;

  ///////////////////
  // Sub-classes MAY override these

  constructor(provider?: Provider, serializer?: TxnDataSerializer) {
    this.provider = provider || new VoidProvider();
    let endpoint = '';
    let skipDataValidation = false;
    if (this.provider instanceof JsonRpcProvider) {
      endpoint = this.provider.endpoint;
      skipDataValidation = this.provider.skipDataValidation;
    }
    this.serializer =
      serializer || new RpcTxnDataSerializer(endpoint, skipDataValidation);
  }

  /**
   * Sign a transaction and submit to the Gateway for execution
   * @experimental
   * @param txBytes BCS serialised TransactionData bytes
   */
  async signAndExecuteTransaction(
    txBytes: Base64DataBuffer
  ): Promise<SuiTransactionResponse> {
    const sig = await this.signData(txBytes);
    return await this.provider.executeTransaction(
      txBytes.toString(),
      sig.signatureScheme,
      sig.signature.toString(),
      sig.pubKey.toString()
    );
  }

  /**
   * @experimental Sign a transaction and submit to the Fullnode for execution
   *
   * @param txBytes BCS serialised TransactionData bytes
   */
  async signAndExecuteTransactionWithRequestType(
    txBytes: Base64DataBuffer,
    requestType: ExecuteTransactionRequestType
  ): Promise<SuiExecuteTransactionResponse> {
    const sig = await this.signData(txBytes);
    return await this.provider.executeTransactionWithRequestType(
      txBytes.toString(),
      sig.signatureScheme,
      sig.signature.toString(),
      sig.pubKey.toString(),
      requestType
    );
  }

  /**
   * Trigger gateway to sync account state related to the address,
   * based on the account state on validators.
   */
  async syncAccountState(): Promise<any> {
    const address = await this.getAddress();
    return await this.provider.syncAccountState(address);
  }

  /**
   * Serialize and Sign a `TransferObject` transaction and submit to the Gateway for execution
   */
  async transferObject(
    transaction: TransferObjectTransaction
  ): Promise<SuiTransactionResponse> {
    const signerAddress = await this.getAddress();
    const txBytes = await this.serializer.newTransferObject(
      signerAddress,
      transaction
    );
    return await this.signAndExecuteTransaction(txBytes);
  }

  /**
   * Serialize and Sign a `TransferSui` transaction and submit to the Gateway for execution
   */
  async transferSui(
    transaction: TransferSuiTransaction
  ): Promise<SuiTransactionResponse> {
    const signerAddress = await this.getAddress();
    const txBytes = await this.serializer.newTransferSui(
      signerAddress,
      transaction
    );
    return await this.signAndExecuteTransaction(txBytes);
  }

  /**
   * Serialize and Sign a `Pay` transaction and submit to the Gateway for execution
   */
  async pay(transaction: PayTransaction): Promise<SuiTransactionResponse> {
    const signerAddress = await this.getAddress();
    const txBytes = await this.serializer.newPay(signerAddress, transaction);
    return await this.signAndExecuteTransaction(txBytes);
  }

  /**
   * Serialize and Sign a `MergeCoin` transaction and submit to the Gateway for execution
   */
  async mergeCoin(
    transaction: MergeCoinTransaction
  ): Promise<SuiTransactionResponse> {
    const signerAddress = await this.getAddress();
    const txBytes = await this.serializer.newMergeCoin(
      signerAddress,
      transaction
    );
    return await this.signAndExecuteTransaction(txBytes);
  }

  /**
   * Serialize and Sign a `SplitCoin` transaction and submit to the Gateway for execution
   */
  async splitCoin(
    transaction: SplitCoinTransaction
  ): Promise<SuiTransactionResponse> {
    const signerAddress = await this.getAddress();
    const txBytes = await this.serializer.newSplitCoin(
      signerAddress,
      transaction
    );
    return await this.signAndExecuteTransaction(txBytes);
  }

  /**
   * Serialize and Sign a `MoveCall` transaction and submit to the Gateway for execution
   */
  async executeMoveCall(
    transaction: MoveCallTransaction
  ): Promise<SuiTransactionResponse> {
    const signerAddress = await this.getAddress();
    const txBytes = await this.serializer.newMoveCall(
      signerAddress,
      transaction
    );
    return await this.signAndExecuteTransaction(txBytes);
  }

  /**
   * Publish a Move package on chain
   * @param transaction See {@link PublishTransaction}
   */
  async publish(
    transaction: PublishTransaction
  ): Promise<SuiTransactionResponse> {
    const signerAddress = await this.getAddress();
    const txBytes = await this.serializer.newPublish(
      signerAddress,
      transaction
    );
    return await this.signAndExecuteTransaction(txBytes);
  }

  /* ---------------------------- Experimental API ---------------------------- */
  /**
   * @experimental Serialize and sign a `TransferObject` transaction and submit to the Fullnode
   * for execution
   */
  async transferObjectWithRequestType(
    transaction: TransferObjectTransaction,
    requestType: ExecuteTransactionRequestType = 'WaitForEffectsCert'
  ): Promise<SuiExecuteTransactionResponse> {
    const signerAddress = await this.getAddress();
    const txBytes = await this.serializer.newTransferObject(
      signerAddress,
      transaction
    );
    return await this.signAndExecuteTransactionWithRequestType(
      txBytes,
      requestType
    );
  }

  /**
   * @experimental Serialize and sign a `TransferSui` transaction and submit to the Fullnode
   * for execution
   */
  async transferSuiWithRequestType(
    transaction: TransferSuiTransaction,
    requestType: ExecuteTransactionRequestType = 'WaitForEffectsCert'
  ): Promise<SuiExecuteTransactionResponse> {
    const signerAddress = await this.getAddress();
    const txBytes = await this.serializer.newTransferSui(
      signerAddress,
      transaction
    );
    return await this.signAndExecuteTransactionWithRequestType(
      txBytes,
      requestType
    );
  }

  /**
   * @experimental Serialize and Sign a `Pay` transaction and submit to the fullnode for execution
   */
  async payWithRequestType(
    transaction: PayTransaction,
    requestType: ExecuteTransactionRequestType = 'WaitForEffectsCert'
  ): Promise<SuiExecuteTransactionResponse> {
    const signerAddress = await this.getAddress();
    const txBytes = await this.serializer.newPay(signerAddress, transaction);
    return await this.signAndExecuteTransactionWithRequestType(
      txBytes,
      requestType
    );
  }

  /**
   * @experimental Serialize and sign a `MergeCoin` transaction and submit to the Fullnode
   * for execution
   */
  async mergeCoinWithRequestType(
    transaction: MergeCoinTransaction,
    requestType: ExecuteTransactionRequestType = 'WaitForEffectsCert'
  ): Promise<SuiExecuteTransactionResponse> {
    const signerAddress = await this.getAddress();
    const txBytes = await this.serializer.newMergeCoin(
      signerAddress,
      transaction
    );
    return await this.signAndExecuteTransactionWithRequestType(
      txBytes,
      requestType
    );
  }

  /**
   * @experimental Serialize and sign a `SplitCoin` transaction and submit to the Fullnode
   * for execution
   */
  async splitCoinWithRequestType(
    transaction: SplitCoinTransaction,
    requestType: ExecuteTransactionRequestType = 'WaitForEffectsCert'
  ): Promise<SuiExecuteTransactionResponse> {
    const signerAddress = await this.getAddress();
    const txBytes = await this.serializer.newSplitCoin(
      signerAddress,
      transaction
    );
    return await this.signAndExecuteTransactionWithRequestType(
      txBytes,
      requestType
    );
  }

  /**
   * @experimental Serialize and sign a `MoveCall` transaction and submit to the Fullnode
   * for execution
   */
  async executeMoveCallWithRequestType(
    transaction: MoveCallTransaction,
    requestType: ExecuteTransactionRequestType = 'WaitForEffectsCert'
  ): Promise<SuiExecuteTransactionResponse> {
    const signerAddress = await this.getAddress();
    const txBytes = await this.serializer.newMoveCall(
      signerAddress,
      transaction
    );
    return await this.signAndExecuteTransactionWithRequestType(
      txBytes,
      requestType
    );
  }

  /**
   * @experimental Serialize and sign a `Publish` transaction and submit to the Fullnode
   * for execution
   */
  async publishWithRequestType(
    transaction: PublishTransaction,
    requestType: ExecuteTransactionRequestType = 'WaitForEffectsCert'
  ): Promise<SuiExecuteTransactionResponse> {
    const signerAddress = await this.getAddress();
    const txBytes = await this.serializer.newPublish(
      signerAddress,
      transaction
    );
    return await this.signAndExecuteTransactionWithRequestType(
      txBytes,
      requestType
    );
  }
}
