// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

import {
  createContext,
  FC,
  ReactNode,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useState,
} from "react";
import type {
  SuiAddress,
  MoveCallTransaction,
  SuiTransactionResponse,
  SignableTransaction,
} from "@mysten/sui.js";
import { WalletAdapter, WalletAdapterList } from "@mysten/wallet-adapter-base";
import { useWalletAdapters } from "./useWalletAdapters";

const DEFAULT_STORAGE_KEY = "preferredSuiWallet";

export interface WalletContextState {
  adapters: WalletAdapterList;
  wallets: WalletAdapter[];

  // Wallet that we are currently connected to
  wallet: WalletAdapter | null;

  connecting: boolean;
  connected: boolean;
  // disconnecting: boolean;

  select(walletName: string): void;
  connect(): Promise<void>;
  disconnect(): Promise<void>;

  getAccounts: () => Promise<SuiAddress[]>;

  signAndExecuteTransaction(
    transaction: SignableTransaction
  ): Promise<SuiTransactionResponse>;

  /** @deprecated Prefer `signAndExecuteTransaction` when available. */
  executeMoveCall: (
    transaction: MoveCallTransaction
  ) => Promise<SuiTransactionResponse>;
  /** @deprecated Prefer `signAndExecuteTransaction` when available. */
  executeSerializedMoveCall: (
    transactionBytes: Uint8Array
  ) => Promise<SuiTransactionResponse>;
}

export const WalletContext = createContext<WalletContextState | null>(null);

// TODO: Add storage adapter interface
// TODO: Add storage key option
// TODO: Add autoConnect option
export interface WalletProviderProps {
  children: ReactNode;
  adapters: WalletAdapterList;
}

export const WalletProvider: FC<WalletProviderProps> = ({
  children,
  adapters,
}) => {
  const wallets = useWalletAdapters(adapters);

  const [wallet, setWallet] = useState<WalletAdapter | null>(null);
  const [connected, setConnected] = useState(false);
  const [connecting, setConnecting] = useState(false);

  const disconnect = useCallback(async () => {
    setConnected(false);
    setWalletAndUpdateStorage(null);
  }, []);

  const connect = useCallback(async () => {
    if (wallet == null) {
      return;
    }

    try {
      setConnecting(true);
      await wallet.connect();
      setConnected(true);
    } catch (e) {
      setConnected(false);
    }
    setConnecting(false);
  }, [wallet]);

  // Use this to update wallet so that the chosen wallet persists after reload.
  const setWalletAndUpdateStorage = useCallback(
    (selectedWallet: WalletAdapter | null) => {
      setWallet(selectedWallet);
      if (selectedWallet) {
        localStorage.setItem(DEFAULT_STORAGE_KEY, selectedWallet.name);
      } else {
        localStorage.removeItem(DEFAULT_STORAGE_KEY);
      }
    },
    []
  );

  const select = useCallback(
    (name: string) => {
      let newWallet = wallets.find((wallet) => wallet.name === name);
      if (newWallet) {
        setWalletAndUpdateStorage(newWallet);
      }
      connect();
    },
    [setWalletAndUpdateStorage, connect]
  );

  // If the wallet is null, check if there isn't anything in local storage
  // Note: Optimize this.
  useEffect(() => {
    if (!wallet && !connected && !connecting) {
      let preferredWallet = localStorage.getItem(DEFAULT_STORAGE_KEY);
      if (typeof preferredWallet === "string") {
        select(preferredWallet);
      }
    }
  }, [select, connected, connecting, wallet]);

  // Attempt to connect whenever user selects a new wallet
  useEffect(() => {
    if (wallet != null && connecting !== true && connected !== true) {
      connect();
    }
  }, [connect, wallet, connecting, connected]);

  const walletContext = useMemo<WalletContextState>(
    () => ({
      adapters,
      wallets,
      wallet,
      connecting,
      connected,
      select,
      connect,
      disconnect,

      async getAccounts() {
        if (wallet == null) throw Error("Wallet Not Connected");
        return wallet.getAccounts();
      },

      async executeMoveCall(transaction) {
        if (wallet == null) throw Error("Wallet Not Connected");
        if (!wallet.executeMoveCall) {
          throw new Error('Wallet does not support "executeMoveCall" method');
        }
        return wallet.executeMoveCall(transaction);
      },

      async executeSerializedMoveCall(transactionBytes) {
        if (wallet == null) throw Error("Wallet Not Connected");
        if (!wallet.executeSerializedMoveCall) {
          throw new Error(
            'Wallet does not support "executeSerializedMoveCall" method'
          );
        }
        return wallet.executeSerializedMoveCall(transactionBytes);
      },

      async signAndExecuteTransaction(transaction) {
        if (wallet == null) {
          throw new Error("Wallet Not Connected");
        }
        if (!wallet.signAndExecuteTransaction) {
          throw new Error(
            'Wallet does not support "signAndExecuteTransaction" method'
          );
        }
        return wallet.signAndExecuteTransaction(transaction);
      },
    }),
    [
      wallets,
      adapters,
      wallet,
      select,
      connect,
      disconnect,
      connecting,
      connected,
    ]
  );

  return (
    <WalletContext.Provider value={walletContext}>
      {children}
    </WalletContext.Provider>
  );
};

export function useWallet(): WalletContextState {
  const context = useContext(WalletContext);

  if (!context) {
    throw new Error(
      "You tried to access the `WalletContext` outside of the `WalletProvider`."
    );
  }

  return context;
}
