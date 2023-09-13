// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

import { act, renderHook, waitFor } from '@testing-library/react';
import { createDAppKitProviderContextWrapper, registerMockWallet } from '../test-utils.js';
import {
	useConnectWallet,
	useConnectionStatus,
	useCurrentAccount,
	useCurrentWallet,
	useDisconnectWallet,
	useWallets,
} from 'dapp-kit/src';
import { createMockAccount } from '../mocks/mockAccount.js';

describe('DAppKitProvider', () => {
	test('the correct wallet and account information is returned on initial render', () => {
		const wrapper = createDAppKitProviderContextWrapper();
		const { result } = renderHook(
			() => ({
				connectionStatus: useConnectionStatus(),
				wallets: useWallets(),
				currentWallet: useCurrentWallet(),
				currentAccount: useCurrentAccount(),
			}),
			{ wrapper },
		);

		expect(result.current.currentWallet).toBeFalsy();
		expect(result.current.currentAccount).toBeFalsy();
		expect(result.current.wallets).toHaveLength(0);
		expect(result.current.connectionStatus).toBe('disconnected');
	});

	// test('the list of wallets is ordered correctly by preference', () => {
	// 	const { unregister: unregister1 } = registerMockWallet({ walletName: 'Mock Wallet 1' });
	// 	const { unregister: unregister2 } = registerMockWallet({ walletName: 'Mock Wallet 2' });
	// 	const { unregister: unregister3 } = registerMockWallet({ walletName: 'Mock Wallet 3' });

	// 	const wrapper = createDAppKitProviderContextWrapper({
	// 		preferredWallets: ['Mock Wallet 2', 'Mock Wallet 1'],
	// 	});
	// 	const { result } = renderHook(() => useWallet(), { wrapper });
	// 	const walletNames = result.current.wallets.map((wallet) => wallet.name);

	// 	expect(walletNames).toStrictEqual(['Mock Wallet 2', 'Mock Wallet 1', 'Mock Wallet 3']);

	// 	act(() => {
	// 		unregister1();
	// 		unregister2();
	// 		unregister3();
	// 	});
	// });

	// test('the unsafe burner wallet is registered when enableUnsafeBurner is set', async () => {
	// 	const wrapper = createDAppKitProviderContextWrapper({
	// 		enableUnsafeBurner: true,
	// 	});
	// 	const { result } = renderHook(() => useWallet(), { wrapper });
	// 	const walletNames = result.current.wallets.map((wallet) => wallet.name);

	// 	expect(walletNames).toStrictEqual(['Unsafe Burner Wallet']);
	// });

	// test('unregistered wallets are removed from the list of wallets', async () => {
	// 	const { unregister: unregister1 } = registerMockWallet({ walletName: 'Mock Wallet 1' });
	// 	const { unregister: unregister2 } = registerMockWallet({ walletName: 'Mock Wallet 2' });
	// 	const { unregister: unregister3 } = registerMockWallet({ walletName: 'Mock Wallet 3' });

	// 	const wrapper = createDAppKitProviderContextWrapper();
	// 	const { result } = renderHook(() => useWallet(), { wrapper });

	// 	act(() => unregister2());

	// 	const walletNames = result.current.wallets.map((wallet) => wallet.name);
	// 	expect(walletNames).toStrictEqual(['Mock Wallet 1', 'Mock Wallet 3']);

	// 	act(() => {
	// 		unregister1();
	// 		unregister3();
	// 	});
	// });

	// test('the list of wallets is correctly filtered by required features', () => {
	// 	const { unregister: unregister1 } = registerMockWallet({
	// 		walletName: 'Mock Wallet 1',
	// 		additionalFeatures: {
	// 			'my-dapp:super-cool-feature': {
	// 				version: '1.0.0',
	// 				superCoolFeature: () => {},
	// 			},
	// 		},
	// 	});
	// 	const { unregister: unregister2 } = registerMockWallet({ walletName: 'Mock Wallet 2' });

	// 	const wrapper = createDAppKitProviderContextWrapper({
	// 		requiredFeatures: ['my-dapp:super-cool-feature'],
	// 	});
	// 	const { result } = renderHook(() => useWallet(), { wrapper });
	// 	const walletNames = result.current.wallets.map((wallet) => wallet.name);

	// 	expect(walletNames).toStrictEqual(['Mock Wallet 1']);

	// 	act(() => {
	// 		unregister1();
	// 		unregister2();
	// 	});
	// });

	describe('wallet auto-connection', () => {
		test('auto-connecting to a wallet works successfully', async () => {
			const { unregister, mockWallet } = registerMockWallet({
				walletName: 'Mock Wallet 1',
				accounts: [createMockAccount(), createMockAccount()],
			});
			const wrapper = createDAppKitProviderContextWrapper({
				autoConnect: true,
			});

			const { result, unmount } = renderHook(
				() => ({
					connectWallet: useConnectWallet(),
					connectionStatus: useConnectionStatus(),
				}),
				{ wrapper },
			);

			// Manually connect a wallet so we have a wallet to auto-connect to later.

			result.current.connectWallet.mutate({
				wallet: mockWallet,
				accountAddress: mockWallet.accounts[1].address,
			});
			await waitFor(() => expect(result.current.connectionStatus).toBe('connected'));

			// Now unmount our component tree to simulate someone leaving the page.
			unmount();

			// Render our component tree again and auto-connect to our previously connected wallet account.
			const { result: updatedResult } = renderHook(
				() => ({
					currentWallet: useCurrentWallet(),
					currentAccount: useCurrentAccount(),
					connectionStatus: useConnectionStatus(),
				}),
				{ wrapper },
			);

			await waitFor(() => expect(updatedResult.current.currentWallet).toBeTruthy());
			expect(updatedResult.current.currentWallet!.name).toStrictEqual('Mock Wallet 1');

			await waitFor(() => expect(updatedResult.current.currentAccount).toBeTruthy());
			expect(updatedResult.current.currentAccount!.address).toStrictEqual(
				mockWallet.accounts[1].address,
			);

			act(() => unregister());
		});

		test('wallet connection info is removed upon disconnection', async () => {
			const { unregister, mockWallet } = registerMockWallet({
				walletName: 'Mock Wallet 1',
			});
			const wrapper = createDAppKitProviderContextWrapper({
				autoConnect: true,
			});

			const { result, unmount } = renderHook(
				() => ({
					connectWallet: useConnectWallet(),
					disconnectWallet: useDisconnectWallet(),
					currentWallet: useCurrentWallet(),
					currentAccount: useCurrentAccount(),
					connectionStatus: useConnectionStatus(),
				}),
				{ wrapper },
			);

			result.current.connectWallet.mutate({
				wallet: mockWallet,
			});
			await waitFor(() => expect(result.current.connectionStatus).toBe('connected'));

			// By disconnecting, we should remove any wallet connection info that we have stored.
			result.current.disconnectWallet.mutate();
			await waitFor(() => expect(result.current.connectionStatus).toBe('disconnected'));

			// Now unmount our component tree to simulate someone leaving the page.
			unmount();

			// Render our component tree again and assert that we weren't able to auto-connect.
			const { result: updatedResult } = renderHook(() => useConnectionStatus(), { wrapper });
			await waitFor(() => expect(updatedResult.current).toBe('disconnected'));

			act(() => unregister());
		});
	});
});
