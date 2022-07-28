import {
    isMoveEvent,
    isNewObjectEvent,
    isTransferObjectEvent,
    isDeleteObjectEvent,
    isPublishEvent,
} from '@mysten/sui.js';

import { isBigIntOrNumber } from '../../utils/numberUtil';
import { getOwnerStr } from '../../utils/objectUtils';

import type { Category } from '../../pages/transaction-result/TransactionResultType';
import type {
    MoveEvent,
    NewObjectEvent,
    ObjectId,
    SuiAddress,
    SuiEvent,
    TransferObjectEvent,
    DeleteObjectEvent,
    PublishEvent,
} from '@mysten/sui.js';

export type ContentItem = {
    label: string;
    value: string;
    monotypeClass: boolean;
    link?: boolean;
    category?: Category;
};

export type EventDisplayData = {
    top: {
        title: string;
        content: (ContentItem | ContentItem[])[];
    };
    fields?: {
        title: string;
        // css class name to apply to the 'Fields' sub-header
        titleStyle?: string;
        content: (ContentItem | ContentItem[])[];
    };
};

function addressContent(label: string, addr: SuiAddress) {
    return {
        label: label,
        value: addr,
        link: true,
        category: 'addresses' as Category,
        monotypeClass: true,
    };
}

function objectContent(label: string, id: ObjectId) {
    return {
        label: label,
        value: id,
        link: true,
        category: 'objects' as Category,
        monotypeClass: true,
    };
}

function fieldsContent(fields: { [key: string]: any }) {
    return Object.keys(fields).map((k) => {
        return {
            label: k,
            value: fields[k].toString(),
            monotypeClass: true,
        };
    });
}

export function moveEventDisplay(event: MoveEvent): EventDisplayData {
    return {
        top: {
            title: 'Move Event',
            content: [
                {
                    label: 'Type',
                    value: event.type,
                    monotypeClass: true,
                },
                addressContent('Sender', event.sender as string),
                {
                    label: 'BCS',
                    value: event.bcs,
                    monotypeClass: true,
                },
            ],
        },
        fields: {
            title: 'Fields',
            titleStyle: 'itemfieldstitle',
            content: fieldsContent(event.fields),
        },
    };
}

export function newObjectEventDisplay(event: NewObjectEvent): EventDisplayData {
    return {
        top: {
            title: 'New Object',
            content: [
                {
                    label: 'Module',
                    value: `${event.packageId}::${event.transactionModule}`,
                    monotypeClass: true,
                },
                [
                    addressContent('', event.sender),
                    addressContent('', getOwnerStr(event.recipient)),
                ],
            ],
        },
    };
}

export function transferObjectEventDisplay(
    event: TransferObjectEvent
): EventDisplayData {
    return {
        top: {
            title: 'Transfer Object',
            content: [
                {
                    label: 'Type',
                    value: event.type,
                    monotypeClass: true,
                },
                objectContent('Object ID', event.objectId),
                {
                    label: 'Version',
                    value: event.version.toString(),
                    monotypeClass: false,
                },
                [
                    addressContent('', event.sender),
                    addressContent('', getOwnerStr(event.recipient)),
                ],
            ],
        },
    };
}

export function deleteObjectEventDisplay(
    event: DeleteObjectEvent
): EventDisplayData {
    return {
        top: {
            title: 'Delete Object',
            content: [
                {
                    label: 'Module',
                    value: `${event.packageId}::${event.transactionModule}`,
                    monotypeClass: true,
                },
                objectContent('Object ID', event.objectId),
                addressContent('Sender', event.sender),
            ],
        },
    };
}

export function publishEventDisplay(event: PublishEvent): EventDisplayData {
    return {
        top: {
            title: 'Publish',
            content: [
                addressContent('Sender', event.sender),
                {
                    label: 'Package',
                    value: event.packageId,
                    monotypeClass: true,
                },
            ],
        },
    };
}

export function bigintDisplay(
    title: string,
    label: string,
    value: bigint
): EventDisplayData {
    return {
        top: {
            title: title,
            content: [
                {
                    label: label,
                    value: value.toString(),
                    monotypeClass: false,
                },
            ],
        },
    };
}

export function eventToDisplay(event: SuiEvent) {
    console.log('event to display', event);

    if ('moveEvent' in event && isMoveEvent(event.moveEvent))
        return moveEventDisplay(event.moveEvent);

    if ('newObject' in event && isNewObjectEvent(event.newObject))
        return newObjectEventDisplay(event.newObject);

    if (
        'transferObject' in event &&
        isTransferObjectEvent(event.transferObject)
    )
        return transferObjectEventDisplay(event.transferObject);

    if ('deleteObject' in event && isDeleteObjectEvent(event.deleteObject))
        return deleteObjectEventDisplay(event.deleteObject);

    if ('publish' in event && isPublishEvent(event.publish))
        return publishEventDisplay(event.publish);

    // TODO - once epoch and checkpoint pages exist, make these links
    if ('epochChange' in event && isBigIntOrNumber(event.epochChange))
        return bigintDisplay('Epoch Change', 'Epoch ID', event.epochChange);

    if ('checkpoint' in event && isBigIntOrNumber(event.checkpoint))
        return bigintDisplay('Checkpoint', 'Sequence #', event.checkpoint);

    return null;
}
