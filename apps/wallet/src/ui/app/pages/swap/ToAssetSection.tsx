// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0
import { useActiveAccount } from '_app/hooks/useActiveAccount';
import {
	Coins,
	getUSDCurrency,
	MAX_FLOAT,
	SUI_CONVERSION_RATE,
	USDC_DECIMALS,
	useBalanceConversion,
	useDeepBookConfigs,
	useRecognizedCoins,
} from '_app/hooks/useDeepBook';
import { Text } from '_app/shared/text';
import { IconButton } from '_components/IconButton';
import { useCoinsReFetchingConfig } from '_hooks';
import { DescriptionItem } from '_pages/approval-request/transaction-request/DescriptionList';
import { AssetData } from '_pages/swap/AssetData';
import { MaxSlippage, MaxSlippageModal } from '_pages/swap/MaxSlippage';
import { ToAssets } from '_pages/swap/ToAssets';
import { useCoinMetadata, useFormatCoin } from '@mysten/core';
import { useSuiClientQuery } from '@mysten/dapp-kit';
import { Refresh16 } from '@mysten/icons';
import { type BalanceChange } from '@mysten/sui.js/client';
import { SUI_TYPE_ARG } from '@mysten/sui.js/utils';
import BigNumber from 'bignumber.js';
import clsx from 'classnames';
import { useEffect, useState } from 'react';
import { useFormContext } from 'react-hook-form';

import { type FormValues } from './utils';

export function ToAssetSection({
	activeCoinType,
	balanceChanges,
}: {
	activeCoinType: string | null;
	balanceChanges: BalanceChange[];
}) {
	const coinsMap = useDeepBookConfigs().coinsMap;
	const activeAccount = useActiveAccount();
	const activeAccountAddress = activeAccount?.address;
	const recognizedCoins = useRecognizedCoins();
	const [isToAssetOpen, setToAssetOpen] = useState(false);
	const [isSlippageModalOpen, setSlippageModalOpen] = useState(false);
	const isAsk = activeCoinType === SUI_TYPE_ARG;

	const {
		watch,
		setValue,
		formState: { isValid },
	} = useFormContext<FormValues>();
	const { data: activeCoinData } = useCoinMetadata(activeCoinType);
	const toAssetType = watch('toAssetType');

	const rawToAssetAmount = balanceChanges.find(
		(balanceChange) => balanceChange.coinType === toAssetType,
	)?.amount;

	const toAssetAmountAsNum = new BigNumber(rawToAssetAmount || '0')
		.shiftedBy(isAsk ? -SUI_CONVERSION_RATE : -USDC_DECIMALS)
		.toNumber();

	const { staleTime, refetchInterval } = useCoinsReFetchingConfig();

	useEffect(() => {
		const newToAsset = isAsk ? coinsMap[Coins.USDC] : SUI_TYPE_ARG;
		setValue('toAssetType', newToAsset);
	}, [coinsMap, isAsk, setValue]);

	const { data: coinBalanceData } = useSuiClientQuery(
		'getBalance',
		{ coinType: toAssetType, owner: activeAccountAddress! },
		{ enabled: !!activeAccountAddress, refetchInterval, staleTime },
	);

	const coinBalance = coinBalanceData?.totalBalance;

	const [toAssetBalance, _, toAssetMetaData] = useFormatCoin(coinBalance, toAssetType);

	const toAssetSymbol = toAssetMetaData.data?.symbol ?? '';
	const amount = watch('amount');

	const { rawValue, averagePrice, refetch, isRefetching } = useBalanceConversion(
		new BigNumber(amount),
		isAsk ? Coins.SUI : Coins.USDC,
		isAsk ? Coins.USDC : Coins.SUI,
		isAsk ? -SUI_CONVERSION_RATE : SUI_CONVERSION_RATE,
	);

	const averagePriceAsString = averagePrice.toFixed(MAX_FLOAT).toString();

	if (!toAssetMetaData.data) {
		return null;
	}

	return (
		<div
			className={clsx(
				'flex flex-col border border-hero-darkest/20 rounded-xl p-5 gap-4 border-solid',
				isValid && 'bg-sui-primaryBlue2023/10',
			)}
		>
			<ToAssets
				recognizedCoins={recognizedCoins}
				isOpen={isToAssetOpen}
				onClose={() => setToAssetOpen(false)}
				onRowClick={(coinType) => {
					setToAssetOpen(false);
				}}
			/>
			<AssetData
				disabled
				tokenBalance={toAssetBalance}
				coinType={toAssetType}
				symbol={toAssetSymbol}
				onClick={() => {
					setToAssetOpen(true);
				}}
			/>
			<div
				className={clsx(
					'py-2 pr-2 pl-3 rounded-lg bg-gray-40 flex gap-2',
					isValid && 'border-solid border-hero-darkest/10',
				)}
			>
				{toAssetAmountAsNum && !isRefetching ? (
					<>
						<Text variant="body" weight="semibold" color="steel-darker">
							{toAssetAmountAsNum}
						</Text>
						<Text variant="body" weight="semibold" color="steel">
							{toAssetSymbol}
						</Text>
					</>
				) : (
					<Text variant="body" weight="semibold" color="steel">
						--
					</Text>
				)}
			</div>
			{rawValue && (
				<div className="ml-3">
					<DescriptionItem
						title={
							<Text variant="bodySmall" color="steel-dark">
								{isRefetching ? '--' : getUSDCurrency(isAsk ? toAssetAmountAsNum : Number(amount))}
							</Text>
						}
					>
						<div className="flex gap-1 items-center">
							<Text variant="bodySmall" weight="medium" color="steel-dark">
								1 {activeCoinData?.symbol} = {isRefetching ? '--' : averagePriceAsString}{' '}
								{toAssetSymbol}
							</Text>
							<IconButton
								icon={<Refresh16 className="h-4 w-4 text-steel-dark hover:text-hero-dark" />}
								onClick={() => refetch()}
								loading={isRefetching}
							/>
						</div>
					</DescriptionItem>

					<div className="h-px w-full bg-hero-darkest/10 my-3" />

					<MaxSlippage onOpen={() => setSlippageModalOpen(true)} />
					<MaxSlippageModal
						isOpen={isSlippageModalOpen}
						onClose={() => setSlippageModalOpen(false)}
					/>
				</div>
			)}
		</div>
	);
}
