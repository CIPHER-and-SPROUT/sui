// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

import {
  Ed25519Keypair,
  getCertifiedTransaction,
  getTransactionEffects,
  JsonRpcProvider,
  LocalTxnDataSerializer,
  RawSigner,
  SignableTransaction,
} from "@mysten/sui.js";
import { WalletAdapter } from "@mysten/wallet-adapter-base";

export class UnsafeBurnerWalletAdapter implements WalletAdapter {
  name = "Unsafe Burner Wallet";

  connecting: boolean;
  connected: boolean;

  #provider: JsonRpcProvider;
  #keypair: Ed25519Keypair;
  #signer: RawSigner;

  constructor(network: string = "https://fullnode.devnet.sui.io/") {
    this.#keypair = new Ed25519Keypair();
    this.#provider = new JsonRpcProvider(network);
    this.#signer = new RawSigner(
      this.#keypair,
      this.#provider,
      new LocalTxnDataSerializer(this.#provider)
    );
    this.connecting = false;
    this.connected = false;

    console.warn(
      "Your application is presently configured to use the `UnsafeBurnerWalletAdapter`. Ensure that this adapter is removed for production."
    );
  }

  async getAccounts() {
    return [this.#keypair.getPublicKey().toSuiAddress()];
  }

  async signAndExecuteTransaction(transaction: SignableTransaction) {
    const response =
      await this.#signer.signAndExecuteTransactionWithRequestType(transaction);

    return {
      certificate: getCertifiedTransaction(response)!,
      effects: getTransactionEffects(response)!,
      timestamp_ms: null,
      parsed_data: null,
    };
  }

  async connect() {
    this.connecting = true;
    await Promise.resolve();
    this.connecting = false;
    this.connected = true;
  }

  async disconnect() {
    this.connecting = false;
    this.connected = false;
  }
}
