// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

import { useSuiClientQuery } from '@mysten/dapp-kit';
import { type BalanceChange } from '@mysten/sui/client';

import { PreviewCard } from '../PreviewCard';
import { onChainAmountToFloat } from '../utils';

export function BalanceChanges({ changes }: { changes: BalanceChange[] }) {
	return (
		<div className="grid grid-cols-2 gap-4 even:bg-gray-900">
			{changes.map((change, index) => (
				<ChangedBalance key={index} change={change} />
			))}
		</div>
	);
}

function ChangedBalance({ change }: { change: BalanceChange }) {
	const { data: coinMetadata } = useSuiClientQuery('getCoinMetadata', {
		coinType: change.coinType,
	});

	const amount = () => {
		if (!coinMetadata) return '-';
		const amt = onChainAmountToFloat(change.amount, coinMetadata.decimals);

		return `${amt && amt > 0.0 ? '+' : ''}${amt}`;
	};
	if (!coinMetadata) return <div>Loading...</div>;

	return (
		<PreviewCard.Root>
			<PreviewCard.Body>
				<>
					{coinMetadata.iconUrl && (
						<img src={coinMetadata.iconUrl as string} alt={coinMetadata.name} />
					)}
					<p>
						<span className={`${Number(amount()) > 0.0 ? 'text-green-300' : 'text-red-700'}`}>
							{amount()}{' '}
						</span>{' '}
						{coinMetadata.symbol} ({change.coinType})
					</p>
				</>
			</PreviewCard.Body>
			<PreviewCard.Footer owner={change.owner} />
		</PreviewCard.Root>
	);
}
