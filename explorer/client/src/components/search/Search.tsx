// Copyright (c) 2022, Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

import React, { useState, useCallback } from 'react';
import { useNavigate } from 'react-router-dom';

import styles from './Search.module.css';

function Search() {
    const [input, setInput] = useState('');
    const navigate = useNavigate();

    const handleSubmit = useCallback(
        (e: React.FormEvent<HTMLFormElement>) => {
            e.preventDefault();
            input.length < 60
                ? navigate(`../search/${input}`)
                : navigate(`../transactions/${input}`);
            setInput('');
        },
        [input, navigate, setInput]
    );

    const handleTextChange = useCallback(
        (e: React.ChangeEvent<HTMLInputElement>) =>
            setInput(e.currentTarget.value),
        [setInput]
    );

    return (
        <form
            className={styles.form}
            onSubmit={handleSubmit}
            aria-label="search form"
        >
            <input
                className={styles.searchtext}
                id="search"
                placeholder="Search transactions by ID"
                value={input}
                onChange={handleTextChange}
                type="text"
            />
            <input type="submit" value="Search" className={styles.searchbtn} />
        </form>
    );
}

export default Search;
