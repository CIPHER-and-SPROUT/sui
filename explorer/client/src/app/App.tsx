// Copyright (c) 2022, Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

import { Link } from 'react-router-dom';

import { ReactComponent as SuiLogo } from '../assets/Sui Logo.svg';
import Footer from '../components/footer/Footer';
import NetworkSelect from '../components/network/Network';
import Search from '../components/search/Search';
import { NetworkContext, useNetwork } from '../context';
import AppRoutes from '../pages/config/AppRoutes';

import styles from './App.module.css';

function App() {
    const [network, setNetwork] = useNetwork();
    return (
        <NetworkContext.Provider value={[network, setNetwork]}>
            <div className={styles.app}>
                <header className={styles.header}>
                    <Link className={styles.suititle} to="/">
                        <SuiLogo />
                    </Link>
                    <Search />
                    <NetworkSelect />
                </header>
                <main>
                    <AppRoutes />
                </main>
                <footer>
                    <Footer />
                </footer>
            </div>
        </NetworkContext.Provider>
    );
}

export default App;
