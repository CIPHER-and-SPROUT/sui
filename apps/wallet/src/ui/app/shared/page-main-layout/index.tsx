// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

import cl from 'classnames';
import { Link } from 'react-router-dom';

import { useAppSelector } from '../../hooks';
import { Toaster } from '../toaster';
import DappStatus from '_app/shared/dapp-status';
import { ErrorBoundary } from '_components/error-boundary';
import Logo from '_components/logo';
import { MenuButton, MenuContent } from '_components/menu';
import Navigation from '_components/navigation';

import type { ReactNode } from 'react';

import st from './PageMainLayout.module.scss';

export type PageMainLayoutProps = {
    children: ReactNode | ReactNode[];
    bottomNavEnabled?: boolean;
    topNavMenuEnabled?: boolean;
    dappStatusEnabled?: boolean;
    centerLogo?: boolean;
    className?: string;
};

export default function PageMainLayout({
    children,
    bottomNavEnabled = false,
    topNavMenuEnabled = false,
    dappStatusEnabled = false,
    centerLogo = false,
    className,
}: PageMainLayoutProps) {
    const networkName = useAppSelector(({ app: { apiEnv } }) => apiEnv);
    return (
        <div className={st.container}>
            <div
                className={cl(st.header, {
                    [st.center]:
                        centerLogo && !topNavMenuEnabled && !dappStatusEnabled,
                })}
            >
                <Link
                    to="/tokens"
                    className="no-underline text-gray-90 mr-auto flex flex-1"
                >
                    <Logo networkName={networkName} />
                </Link>
                {dappStatusEnabled ? (
                    <div className={st.dappStatusContainer}>
                        <DappStatus />
                    </div>
                ) : null}
                {topNavMenuEnabled ? (
                    <MenuButton className={st.menuButton} />
                ) : null}
            </div>
            <div className={st.content}>
                <main
                    className={cl(
                        st.main,
                        { [st.withNav]: bottomNavEnabled },
                        className
                    )}
                >
                    <ErrorBoundary>{children}</ErrorBoundary>
                </main>
                {bottomNavEnabled ? <Navigation /> : null}
                {topNavMenuEnabled ? <MenuContent /> : null}
                <Toaster bottomNavEnabled={bottomNavEnabled} />
            </div>
        </div>
    );
}
