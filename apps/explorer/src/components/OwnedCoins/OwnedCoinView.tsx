// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

import { useFormatCoin } from '@mysten/core';
import { ArrowShowAndHideRight12, Warning16 } from '@mysten/icons';
import { type CoinBalance } from '@mysten/sui.js/client';
import { Text } from '@mysten/ui';
import * as Collapsible from '@radix-ui/react-collapsible';
import clsx from 'clsx';
import { useState } from 'react';

import { CoinIcon } from './CoinIcon';
import CoinsPanel from './OwnedCoinsPanel';
import { Banner } from '~/ui/Banner';
import { Tooltip } from '~/ui/Tooltip';
import { ampli } from '~/utils/analytics/ampli';
import { CoinBalanceVerified } from '.';

type OwnedCoinViewProps = {
	coin: CoinBalanceVerified;
	id: string;
	isRecognized?: boolean;
};

export default function OwnedCoinView({ coin, id, isRecognized }: OwnedCoinViewProps) {
	const [open, setOpen] = useState(false);
	const [formattedTotalBalance, symbol] = useFormatCoin(coin.totalBalance, coin.coinType);

	return (
		<Collapsible.Root open={open} onOpenChange={setOpen}>
			<Collapsible.Trigger
				data-testid="ownedcoinlabel"
				className={clsx(
					'mt-1 flex w-full items-center rounded-lg bg-opacity-5 p-2 text-left hover:bg-hero-darkest hover:bg-opacity-5',
					open && 'bg-hero-darkest pt-3',
				)}
				style={{
					borderBottomLeftRadius: open ? '0' : '8px',
					borderBottomRightRadius: open ? '0' : '8px',
				}}
			>
				<div className="flex w-[45%] items-center gap-1 truncate">
					<ArrowShowAndHideRight12
						className={clsx('text-gray-60', open && 'rotate-90 transform')}
					/>
					{/* fade in 300ms for pills */}
					<div className="flex items-center gap-3">
						<CoinIcon coinType={coin.coinType} size="sm" />
						<Text color="steel-darker" variant="body/medium">
							{symbol}
						</Text>
					</div>

					{!isRecognized && (
						<Tooltip
							tip="This coin has not been recognized by Sui Foundation."
							onOpen={() =>
								ampli.activatedTooltip({
									tooltipLabel: 'unrecognizedCoinWarning',
								})
							}
						>
							<Banner variant="warning" icon={null} border spacing="sm">
								<Warning16 />
							</Banner>
						</Tooltip>
					)}
				</div>

				<div className="flex w-[25%] px-2">
					<Text color={isRecognized ? 'steel-darker' : 'gray-60'} variant="body/medium">
						{coin.coinObjectCount}
					</Text>
				</div>

				<div className="flex w-[30%] items-center gap-1">
					<Text color={isRecognized ? 'steel-darker' : 'gray-60'} variant="bodySmall/medium">
						{formattedTotalBalance}
					</Text>
					<Text color="steel" variant="subtitleSmallExtra/normal">
						{symbol}
					</Text>
				</div>
			</Collapsible.Trigger>

			<Collapsible.Content>
				<div
					className="flex flex-col gap-1 bg-gray-40 p-3"
					style={{
						borderBottomLeftRadius: '8px',
						borderBottomRightRadius: '8px',
					}}
				>
					<CoinsPanel id={id} coinType={coin.coinType} />
				</div>
			</Collapsible.Content>
		</Collapsible.Root>
	);
}
