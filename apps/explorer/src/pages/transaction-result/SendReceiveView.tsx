// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

import { useQuery } from '@tanstack/react-query';
import cl from 'clsx';
import { useState, useContext } from 'react';

import { ReactComponent as DoneIcon } from '../../assets/SVGIcons/16px/CheckFill.svg';
import { ReactComponent as StartIcon } from '../../assets/SVGIcons/Start.svg';
import Longtext from '../../components/longtext/Longtext';
import { NetworkContext } from '../../context';
import { DefaultRpcClient as rpc } from '../../utils/api/DefaultRpcClient';
import { parseObjectType } from '../../utils/objectUtils';

import styles from './SendReceiveView.module.css';

import { useFormatCoin, CoinFormat } from '~/hooks/useFormatCoin';
import { Heading } from '~/ui/Heading';

type TxAddress = {
    sender: string;
    recipient?: string[];
    amount?: bigint[];
    objects?: string[];
};

const getObjType = (objId: string, network: string) =>
    rpc(network)
        .getObject(objId)
        .then((obj) => parseObjectType(obj));

function MultipleRecipients({ sender, recipient, amount, objects }: TxAddress) {
    const [network] = useContext(NetworkContext);

    const [isSingleCoin, setIsSingleCoin] = useState(false);

    const { data: coinList, isSuccess } = useQuery({
        queryKey: ['get-coin-types-for-pay-tx', network, objects],
        queryFn: async () => {
            const coinList = await Promise.all(
                objects!.map((objId) => getObjType(objId, network))
            );

            if (coinList.every((val) => val === coinList[0])) {
                setIsSingleCoin(true);
            }

            return coinList;
        },
        enabled: !!objects,
    });

    return (
        <>
            {isSingleCoin && amount && (
                <div className={styles.amountbox}>
                    <div>Amount</div>
                    <SingleAmount
                        amount={amount.reduce((x, y) => x + y)}
                        objectId={objects![0]}
                    />
                </div>
            )}
            <div className={styles.txaddress} data-testid="transaction-sender">
                <div className={styles.senderbox}>
                    <Heading as="h4" variant="heading4" weight="semibold">
                        Sender
                    </Heading>
                    <div className={styles.oneaddress}>
                        <StartIcon />
                        <Longtext
                            text={sender}
                            category="addresses"
                            isLink={true}
                        />
                    </div>
                </div>
                <div
                    className={cl([
                        styles.txaddresssender,
                        recipient?.length ? styles.recipient : '',
                    ])}
                >
                    {recipient && (
                        <div className={styles.recipientbox}>
                            <div>
                                <Heading
                                    as="h4"
                                    variant="heading4"
                                    weight="semibold"
                                >
                                    Recipients
                                </Heading>
                            </div>
                            {recipient.map((add: string, idx: number) => (
                                <div key={idx}>
                                    <>
                                        <div className={styles.oneaddress}>
                                            <div className={styles.doneicon}>
                                                <DoneIcon />
                                            </div>
                                            <Longtext
                                                text={add}
                                                category="addresses"
                                                isLink={true}
                                                alttext={add}
                                            />
                                        </div>
                                        {amount?.[idx] && (
                                            <Amount
                                                amount={amount![idx]}
                                                label={
                                                    isSuccess && coinList
                                                        ? coinList[idx]
                                                        : ''
                                                }
                                            />
                                        )}
                                    </>
                                </div>
                            ))}
                        </div>
                    )}
                </div>
            </div>
        </>
    );
}

function Amount({ amount, label }: { amount: bigint; label: string }) {
    const coinBoilerPlateRemoved = /^0x2::coin::Coin<(.+)>$/.exec(label)?.[1];
    const formattedCoin = useFormatCoin(
        amount,
        coinBoilerPlateRemoved,
        CoinFormat.FULL
    );
    return (
        <div className={styles.sui}>
            <span className={styles.suiamount}>{formattedCoin[0]}</span>
            <span className={styles.suilabel}>{formattedCoin[1]}</span>
        </div>
    );
}

function SingleAmount({
    amount,
    objectId,
}: {
    amount: bigint;
    objectId: string;
}) {
    const [network] = useContext(NetworkContext);

    const { data: label } = useQuery(
        ['get-coin-type-for-pay-tx', objectId, network],
        async () => {
            const objType = await getObjType(objectId, network);

            return /^0x2::coin::Coin<(.+)>$/.exec(objType)?.[1]!;
        }
    );

    const formattedAmount = useFormatCoin(amount, label, CoinFormat.FULL);

    return (
        <div>
            {formattedAmount[0]}
            <sup>{formattedAmount[1]}</sup>
        </div>
    );
}

//TODO: Add date format function
function SendReceiveView({ sender, recipient, amount, objects }: TxAddress) {
    if (recipient && recipient.length === 1 && amount) {
        return (
            <>
                <div className={styles.amountbox}>
                    <div>Amount</div>
                    <SingleAmount amount={amount[0]} objectId={objects![0]} />
                </div>
                <div className={styles.txaddress}>
                    <div className={styles.oneheading}>
                        <Heading as="h4" variant="heading4" weight="semibold">
                            Sender &#x26; Recipient
                        </Heading>
                    </div>
                    <div
                        className={cl([styles.oneaddress, styles.senderwline])}
                    >
                        <div className="z-0">
                            <StartIcon />
                        </div>
                        <Longtext
                            text={sender}
                            category="addresses"
                            isLink={true}
                        />
                    </div>
                    <div>
                        {recipient.map((add: string, idx: number) => (
                            <div key={idx} className="flex">
                                <div
                                    className={cl([
                                        styles.oneaddress,
                                        'mt-[20px] ml-[10px] w-[90%]',
                                    ])}
                                >
                                    <div
                                        className={`${styles.doneicon} ${styles.doneiconwline}`}
                                    >
                                        <DoneIcon />
                                    </div>
                                    <Longtext
                                        text={add}
                                        category="addresses"
                                        isLink={true}
                                        alttext={add}
                                    />
                                </div>
                            </div>
                        ))}
                    </div>
                </div>
            </>
        );
    }

    return (
        <MultipleRecipients
            sender={sender}
            recipient={recipient}
            amount={amount}
            objects={objects}
        />
    );
}

export default SendReceiveView;
