// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

import { useActiveAccount } from '_app/hooks/useActiveAccount';
import { useRecognizedPackages } from '_app/hooks/useRecognizedPackages';
import { useSigner } from '_app/hooks/useSigner';
import BottomMenuLayout, { Content, Menu } from '_app/shared/bottom-menu-layout';
import { Button } from '_app/shared/ButtonUI';
import { Form } from '_app/shared/forms/Form';
import { InputWithActionButton } from '_app/shared/InputWithAction';
import { ButtonOrLink } from '_app/shared/utils/ButtonOrLink';
import Loading from '_components/loading';
import Overlay from '_components/overlay';
import { filterAndSortTokenBalances } from '_helpers';
import {
	allowedSwapCoinsList,
	Coins,
	getUSDCurrency,
	isExceedingSlippageTolerance,
	SUI_CONVERSION_RATE,
	USDC_DECIMALS,
	useBalanceConversion,
	useCoinsReFetchingConfig,
	useDeepBookConfigs,
	useGetEstimate,
	useSortedCoinsByCategories,
} from '_hooks';
import { DeepBookContextProvider, useDeepBookContext } from '_shared/deepBook/context';
import { useFormatCoin, useTransactionSummary, useZodForm } from '@mysten/core';
import { useSuiClientQuery } from '@mysten/dapp-kit';
import { ArrowDown12, ArrowRight16 } from '@mysten/icons';
import { type DryRunTransactionBlockResponse } from '@mysten/sui.js/client';
import { SUI_DECIMALS, SUI_TYPE_ARG } from '@mysten/sui.js/utils';
import { useMutation, useQueryClient } from '@tanstack/react-query';
import BigNumber from 'bignumber.js';
import clsx from 'classnames';
import { useMemo } from 'react';
import { useWatch, type SubmitHandler } from 'react-hook-form';
import { useNavigate, useSearchParams } from 'react-router-dom';
import { z } from 'zod';

import { AssetData } from './AssetData';
import { GasFeeSection } from './GasFeeSection';
import { ToAssetSection } from './ToAssetSection';
import { initialValues, type FormValues } from './utils';

function getSwapPageAtcText(
	fromSymbol: string,
	toAssetType: string,
	coinsMap: Record<string, string>,
) {
	const toSymbol =
		toAssetType === SUI_TYPE_ARG
			? Coins.SUI
			: Object.entries(coinsMap).find(([key, value]) => value === toAssetType)?.[0] || '';

	return `Swap ${fromSymbol} to ${toSymbol}`;
}

export function SwapPageContent() {
	const queryClient = useQueryClient();
	const mainnetPools = useDeepBookConfigs().pools;
	const navigate = useNavigate();
	const [searchParams] = useSearchParams();
	const activeAccount = useActiveAccount();
	const signer = useSigner(activeAccount);
	const activeAccountAddress = activeAccount?.address;
	const { staleTime, refetchInterval } = useCoinsReFetchingConfig();
	const coinsMap = useDeepBookConfigs().coinsMap;
	const deepBookClient = useDeepBookContext().client;

	const accountCapId = useDeepBookContext().accountCapId;

	const activeCoinType = searchParams.get('type');
	const isAsk = activeCoinType === SUI_TYPE_ARG;

	const baseCoinType = SUI_TYPE_ARG;
	const quoteCoinType = coinsMap.USDC;

	const poolId = mainnetPools.SUI_USDC[0];

	const { data: baseCoinBalanceData, isLoading: baseCoinBalanceDataLoading } = useSuiClientQuery(
		'getBalance',
		{ coinType: baseCoinType, owner: activeAccountAddress! },
		{ enabled: !!activeAccountAddress, refetchInterval, staleTime },
	);

	const { data: quoteCoinBalanceData, isLoading: quoteCoinBalanceDataLoading } = useSuiClientQuery(
		'getBalance',
		{ coinType: quoteCoinType, owner: activeAccountAddress! },
		{ enabled: !!activeAccountAddress, refetchInterval, staleTime },
	);

	const rawBaseBalance = baseCoinBalanceData?.totalBalance;
	const rawQuoteBalance = quoteCoinBalanceData?.totalBalance;

	const [formattedBaseBalance, baseCoinSymbol, baseCoinMetadata] = useFormatCoin(
		rawBaseBalance,
		activeCoinType,
	);
	const [formattedQuoteBalance, quoteCoinSymbol, quoteCoinMetadata] = useFormatCoin(
		rawQuoteBalance,
		activeCoinType,
	);

	const { data: coinBalances } = useSuiClientQuery(
		'getAllBalances',
		{ owner: activeAccountAddress! },
		{
			enabled: !!activeAccountAddress,
			staleTime,
			refetchInterval,
			select: filterAndSortTokenBalances,
		},
	);

	const { recognized } = useSortedCoinsByCategories(coinBalances ?? []);

	const formattedBaseTokenBalance = formattedBaseBalance.replace(/,/g, '');

	const formattedQuoteTokenBalance = formattedQuoteBalance.replace(/,/g, '');

	const baseCoinDecimals = baseCoinMetadata.data?.decimals ?? 0;
	const maxBaseBalance = rawBaseBalance || '0';

	const quoteCoinDecimals = quoteCoinMetadata.data?.decimals ?? 0;
	const maxQuoteBalance = rawQuoteBalance || '0';

	const validationSchema = useMemo(() => {
		return z.object({
			amount: z.string().transform((value, context) => {
				const bigNumberValue = new BigNumber(value);

				if (!value.length) {
					context.addIssue({
						code: 'custom',
						message: 'Amount is required.',
					});
					return z.NEVER;
				}

				if (bigNumberValue.lt(0)) {
					context.addIssue({
						code: 'custom',
						message: 'Amount must be greater than 0.',
					});
					return z.NEVER;
				}

				const shiftedValue = isAsk ? baseCoinDecimals : quoteCoinDecimals;
				const maxBalance = isAsk ? maxBaseBalance : maxQuoteBalance;

				if (bigNumberValue.shiftedBy(shiftedValue).gt(BigInt(maxBalance).toString())) {
					context.addIssue({
						code: 'custom',
						message: 'Not available in account',
					});
					return z.NEVER;
				}

				return value;
			}),
			toAssetType: z.string(),
			allowedMaxSlippagePercentage: z.string().transform((percent, context) => {
				const numberPercent = Number(percent);

				if (numberPercent < 0 || numberPercent > 100) {
					context.addIssue({
						code: 'custom',
						message: 'Value must be between 0 and 100.',
					});
					return z.NEVER;
				}

				return percent;
			}),
		});
	}, [isAsk, baseCoinDecimals, quoteCoinDecimals, maxBaseBalance, maxQuoteBalance]);

	const form = useZodForm({
		mode: 'all',
		schema: validationSchema,
		defaultValues: {
			...initialValues,
			toAssetType: coinsMap.USDC,
		},
	});

	const {
		register,
		setValue,
		control,
		handleSubmit,
		trigger,
		reset,
		formState: { isValid, isSubmitting, errors },
	} = form;

	const renderButtonToCoinsList = useMemo(() => {
		return (
			recognized.length > 1 &&
			recognized.some((coin) => allowedSwapCoinsList.includes(coin.coinType))
		);
	}, [recognized]);

	const amount = useWatch({
		name: 'amount',
		control,
	});

	const isPayAll = amount === (isAsk ? formattedBaseTokenBalance : formattedQuoteTokenBalance);

	const { rawValue: rawInputSuiUsdc } = useBalanceConversion(
		new BigNumber(amount),
		Coins.SUI,
		Coins.USDC,
		-SUI_CONVERSION_RATE,
	);

	const { rawValue: rawInputUsdcSui } = useBalanceConversion(
		new BigNumber(amount),
		Coins.USDC,
		Coins.SUI,
		SUI_CONVERSION_RATE,
	);

	const atcText = useMemo(() => {
		if (isAsk) {
			return getSwapPageAtcText(baseCoinSymbol, quoteCoinType, coinsMap);
		}
		return getSwapPageAtcText(quoteCoinSymbol, baseCoinType, coinsMap);
	}, [isAsk, baseCoinSymbol, baseCoinType, coinsMap, quoteCoinSymbol, quoteCoinType]);

	const baseBalance = new BigNumber(isAsk ? amount || 0 : rawInputUsdcSui || 0)
		.shiftedBy(SUI_DECIMALS)
		.toString();
	const quoteBalance = new BigNumber(isAsk ? rawInputSuiUsdc || 0 : amount || 0)
		.shiftedBy(USDC_DECIMALS)
		.toString();

	const {
		data: dataFromEstimate,
		isLoading: dataFromEstimateLoading,
		isError: dataFromEstimateError,
	} = useGetEstimate({
		signer,
		accountCapId,
		coinType: activeCoinType || '',
		poolId,
		baseBalance,
		quoteBalance,
		isAsk,
	});

	const recognizedPackagesList = useRecognizedPackages();

	const txnSummary = useTransactionSummary({
		transaction: dataFromEstimate?.dryRunResponse as DryRunTransactionBlockResponse,
		recognizedPackagesList,
		currentAddress: activeAccountAddress,
	});

	const totalGas = txnSummary?.gas?.totalGas;
	const balanceChanges = dataFromEstimate?.dryRunResponse?.balanceChanges || [];

	const { mutate: handleSwap, isLoading: isSwapLoading } = useMutation({
		mutationFn: async (formData: FormValues) => {
			const txn = dataFromEstimate?.txn;
			const baseCoinAmount = balanceChanges.find(({ coinType }) => {
				return coinType === baseCoinType;
			})?.amount;

			const quoteCoinAmount = balanceChanges.find(({ coinType }) => {
				return coinType === quoteCoinType;
			})?.amount;

			const isExceedingSlippage = await isExceedingSlippageTolerance({
				slipPercentage: formData.allowedMaxSlippagePercentage,
				poolId,
				deepBookClient,
				conversionRate: USDC_DECIMALS,
				baseCoinAmount,
				quoteCoinAmount,
				isAsk,
			});

			if (isExceedingSlippage) {
				throw new Error('Slippage is not acceptable');
			}

			if (!txn || !signer) {
				throw new Error('Missing data');
			}

			return signer.signAndExecuteTransactionBlock({
				transactionBlock: txn,
				options: {
					showInput: true,
					showEffects: true,
					showEvents: true,
				},
			});
		},
		onSuccess: (response) => {
			queryClient.invalidateQueries(['get-coins']);
			queryClient.invalidateQueries(['coin-balance']);

			const receiptUrl = `/receipt?txdigest=${encodeURIComponent(
				response.digest,
			)}&from=transactions`;
			return navigate(receiptUrl);
		},
	});

	const handleOnsubmit: SubmitHandler<FormValues> = async (formData) => {
		handleSwap(formData);
	};

	return (
		<Overlay showModal title="Swap" closeOverlay={() => navigate('/')}>
			<div className="flex flex-col h-full w-full">
				<Loading loading={baseCoinBalanceDataLoading || quoteCoinBalanceDataLoading}>
					<BottomMenuLayout>
						<Content>
							<Form form={form} onSubmit={handleOnsubmit}>
								<div
									className={clsx(
										'flex flex-col border border-hero-darkest/20 rounded-xl pt-5 pb-6 px-5 gap-4 border-solid',
										isValid && 'bg-gradients-graph-cards',
									)}
								>
									{activeCoinType && (
										<AssetData
											disabled={!renderButtonToCoinsList}
											tokenBalance={isAsk ? formattedBaseTokenBalance : formattedQuoteTokenBalance}
											coinType={activeCoinType}
											symbol={isAsk ? baseCoinSymbol : quoteCoinSymbol}
											to="/swap/from-assets"
										/>
									)}

									<InputWithActionButton
										{...register('amount')}
										dark
										suffix={isAsk ? baseCoinSymbol : quoteCoinSymbol}
										value={amount}
										type="number"
										errorString={errors.amount?.message}
										actionText="Max"
										actionType="button"
										actionDisabled={isPayAll}
										prefix={isPayAll ? '~' : undefined}
										onActionClicked={() => {
											setValue(
												'amount',
												activeCoinType === SUI_TYPE_ARG
													? formattedBaseTokenBalance
													: formattedQuoteTokenBalance,
											);
											trigger('amount');
										}}
									/>

									{isValid && !!amount && (
										<div className="ml-3">
											<div className="text-bodySmall font-medium text-hero-darkest/40">
												{isPayAll ? '~ ' : ''}
												{getUSDCurrency(isAsk ? rawInputSuiUsdc : Number(amount))}
											</div>
										</div>
									)}
								</div>

								<ButtonOrLink
									className="group flex my-4 gap-3 items-center w-full bg-transparent border-none cursor-pointer"
									onClick={() => {
										navigate(
											`/swap?${new URLSearchParams({
												type: activeCoinType === SUI_TYPE_ARG ? coinsMap.USDC : SUI_TYPE_ARG,
											}).toString()}`,
										);
										reset();
									}}
								>
									<div className="bg-gray-45 h-px w-full group-hover:bg-hero-dark" />
									<div className="h-3 w-3">
										<ArrowDown12 className="text-steel group-hover:text-hero-dark" />
									</div>
									<div className="bg-gray-45 h-px w-full group-hover:bg-hero-dark" />
								</ButtonOrLink>

								<ToAssetSection activeCoinType={activeCoinType} balanceChanges={balanceChanges} />

								<div className="mt-4">
									<GasFeeSection
										totalGas={totalGas || ''}
										activeCoinType={activeCoinType}
										amount={amount}
										isValid={isValid}
									/>
								</div>
							</Form>
						</Content>

						<Menu stuckClass="sendCoin-cta" className="w-full px-0 pb-0 mx-0 gap-2.5">
							<Button
								onClick={handleSubmit(handleOnsubmit)}
								type="submit"
								variant="primary"
								loading={isSubmitting || isSwapLoading}
								disabled={
									!isValid || isSubmitting || dataFromEstimateLoading || dataFromEstimateError
								}
								size="tall"
								text={atcText}
								after={<ArrowRight16 />}
							/>
						</Menu>
					</BottomMenuLayout>
				</Loading>
			</div>
		</Overlay>
	);
}

export function SwapPage() {
	return (
		<DeepBookContextProvider>
			<SwapPageContent />
		</DeepBookContextProvider>
	);
}
