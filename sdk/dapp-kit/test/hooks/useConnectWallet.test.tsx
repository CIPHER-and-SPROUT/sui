// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

import { renderHook, waitFor, act } from '@testing-library/react';
import {
	useConnectWallet,
	useConnectionStatus,
	useCurrentWallet,
	useCurrentAccount,
} from 'dapp-kit/src';
import { createWalletProviderContextWrappe, registerMockWallet } from '../test-utils.js';
import { WalletAlreadyConnectedError } from 'dapp-kit/src/errors/walletErrors.js';
import type { Mock } from 'vitest';

describe('useConnectWallet', () => {
	test('throws an error when connecting to a wallet when a connection is already active', async () => {
		const { unregister, mockWallet } = registerMockWallet({ walletName: 'Mock Wallet 1' });

		const wrapper = createWalletProviderContextWrappe();
		const { result } = renderHook(() => useConnectWallet(), { wrapper });

		result.current.mutate({ wallet: mockWallet });
		await waitFor(() => expect(result.current.isSuccess).toBe(true));

		result.current.mutate({ wallet: mockWallet });
		await waitFor(() => expect(result.current.error).toBeInstanceOf(WalletAlreadyConnectedError));

		act(() => {
			unregister();
		});
	});

	test('throws an error when a user fails to connect their wallet', async () => {
		const { unregister, mockWallet } = registerMockWallet({ walletName: 'Mock Wallet 1' });

		const wrapper = createWalletProviderContextWrappe();
		const { result } = renderHook(
			() => ({
				connectWallet: useConnectWallet(),
				connectionStatus: useConnectionStatus(),
			}),
			{ wrapper },
		);

		const connectFeature = mockWallet.features['standard:connect'];
		const mockConnect = connectFeature.connect as Mock;

		mockConnect.mockRejectedValueOnce(() => {
			throw new Error('User rejected request');
		});

		result.current.connectWallet.mutate({ wallet: mockWallet });

		await waitFor(() => expect(result.current.connectWallet.isError).toBe(true));
		expect(result.current.connectionStatus).toBe('disconnected');

		act(() => {
			unregister();
		});
	});

	test('connecting to a wallet works successfully', async () => {
		const { unregister, mockWallet } = registerMockWallet({ walletName: 'Mock Wallet 1' });

		const wrapper = createWalletProviderContextWrappe();
		const { result } = renderHook(
			() => ({
				connectWallet: useConnectWallet(),
				connectionStatus: useConnectionStatus(),
				currentWallet: useCurrentWallet(),
				currentAccount: useCurrentAccount(),
			}),
			{ wrapper },
		);

		result.current.connectWallet.mutate({ wallet: mockWallet });

		await waitFor(() => expect(result.current.connectWallet.isSuccess).toBe(true));
		expect(result.current.currentWallet?.name).toBe('Mock Wallet 1');
		expect(result.current.currentWallet?.accounts).toHaveLength(1);
		expect(result.current.currentAccount).toBeTruthy();
		expect(result.current.connectionStatus).toBe('connected');

		act(() => {
			unregister();
		});
	});
});
