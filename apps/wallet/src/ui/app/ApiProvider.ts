// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

import { JsonRpcProvider, LocalTxnDataSerializer } from '@mysten/sui.js';

import { BackgroundServiceSigner } from './background-client/BackgroundServiceSigner';
import { LedgerSigner } from './LedgerSigner';
import { queryClient } from './helpers/queryClient';
import { growthbook } from '_app/experimentation/feature-gating';
import { API_ENV } from '_src/shared/api-env';
import { FEATURES } from '_src/shared/experimentation/features';
import type { AccountSerialized } from '_redux/slices/account';
import type { BackgroundClient } from './background-client';
import type { SuiAddress, SignerWithProvider } from '@mysten/sui.js';
import type AppSui from 'hw-app-sui';

type EnvInfo = {
    name: string;
    env: API_ENV;
};

type ApiEndpoints = {
    fullNode: string;
    faucet?: string;
} | null;
export const API_ENV_TO_INFO: Record<API_ENV, EnvInfo> = {
    [API_ENV.local]: { name: 'Local', env: API_ENV.local },
    [API_ENV.devNet]: { name: 'Sui Devnet', env: API_ENV.devNet },
    [API_ENV.customRPC]: { name: 'Custom RPC URL', env: API_ENV.customRPC },
    [API_ENV.testNet]: { name: 'Sui Testnet', env: API_ENV.testNet },
};

export const ENV_TO_API: Record<API_ENV, ApiEndpoints> = {
    [API_ENV.local]: {
        fullNode: process.env.API_ENDPOINT_LOCAL_FULLNODE || '',
        faucet: process.env.API_ENDPOINT_LOCAL_FAUCET || '',
    },
    [API_ENV.devNet]: {
        fullNode: process.env.API_ENDPOINT_DEV_NET_FULLNODE || '',
        faucet: process.env.API_ENDPOINT_DEV_NET_FAUCET || '',
    },
    [API_ENV.customRPC]: null,
    [API_ENV.testNet]: {
        fullNode: process.env.API_ENDPOINT_TEST_NET_FULLNODE || '',
        // NOTE: Faucet is currently disabled for testnet:
        // faucet: process.env.API_ENDPOINT_TEST_NET_FAUCET || '',
    },
};

function getDefaultApiEnv() {
    const apiEnv = process.env.API_ENV;
    if (apiEnv && !Object.keys(API_ENV).includes(apiEnv)) {
        throw new Error(`Unknown environment variable API_ENV, ${apiEnv}`);
    }
    return apiEnv ? API_ENV[apiEnv as keyof typeof API_ENV] : API_ENV.devNet;
}

function getDefaultAPI(env: API_ENV) {
    const apiEndpoint = ENV_TO_API[env];
    if (
        !apiEndpoint ||
        apiEndpoint.fullNode === '' ||
        apiEndpoint.faucet === ''
    ) {
        throw new Error(`API endpoint not found for API_ENV ${env}`);
    }
    return apiEndpoint;
}

export const DEFAULT_API_ENV = getDefaultApiEnv();

type NetworkTypes = keyof typeof API_ENV;

export const generateActiveNetworkList = (): NetworkTypes[] => {
    const excludedNetworks: NetworkTypes[] = [];

    if (!growthbook.isOn(FEATURES.USE_TEST_NET_ENDPOINT)) {
        excludedNetworks.push(API_ENV.testNet);
    }

    return Object.values(API_ENV).filter(
        (env) => !excludedNetworks.includes(env as keyof typeof API_ENV)
    );
};

export default class ApiProvider {
    private _apiFullNodeProvider?: JsonRpcProvider;
    private _signerByAddress: Map<SuiAddress, SignerWithProvider> = new Map();

    public setNewJsonRpcProvider(
        apiEnv: API_ENV = DEFAULT_API_ENV,
        customRPC?: string | null
    ) {
        this._apiFullNodeProvider = new JsonRpcProvider(
            customRPC ?? getDefaultAPI(apiEnv).fullNode,
            { faucetURL: customRPC ? '' : getDefaultAPI(apiEnv).faucet }
        );
        this._signerByAddress.clear();
        // We also clear the query client whenever set set a new API provider:
        queryClient.resetQueries();
        queryClient.clear();
    }

    public get instance() {
        if (!this._apiFullNodeProvider) {
            this.setNewJsonRpcProvider();
        }
        return {
            // eslint-disable-next-line @typescript-eslint/no-non-null-assertion
            fullNode: this._apiFullNodeProvider!,
        };
    }

    public getSignerInstance(
        account: AccountSerialized,
        backgroundClient: BackgroundClient,
        initAppSui: () => Promise<AppSui>
    ): SignerWithProvider {
        if (!this._apiFullNodeProvider) {
            this.setNewJsonRpcProvider();
        }
        if (!this._signerByAddress.has(account.address)) {
            const provider = this._apiFullNodeProvider;
            const serializer = growthbook.isOn(
                FEATURES.USE_LOCAL_TXN_SERIALIZER
            )
                ? // eslint-disable-next-line @typescript-eslint/no-non-null-assertion
                  new LocalTxnDataSerializer(this._apiFullNodeProvider!)
                : undefined;
            switch (account.type) {
                case 'derived':
                case 'imported':
                    this._signerByAddress.set(
                        account.address,
                        new BackgroundServiceSigner(
                            account.address,
                            backgroundClient,
                            provider,
                            serializer
                        )
                    );
                    break;

                case 'ledger':
                    this._signerByAddress.set(
                        account.address,
                        new LedgerSigner(
                            initAppSui(),
                            account.derivationPath,
                            provider,
                            serializer
                        )
                    );
                    break;
            }
        }
        // eslint-disable-next-line @typescript-eslint/no-non-null-assertion
        return this._signerByAddress.get(account.address)!;
    }
}
