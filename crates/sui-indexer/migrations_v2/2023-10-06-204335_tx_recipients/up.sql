-- Your SQL goes here
CREATE TABLE tx_recipients (
    tx_sequence_number          BIGINT       NOT NULL,
    -- SuiAddress in bytes.
    recipient                   BYTEA        NOT NULL,
    PRIMARY KEY(recipient, tx_sequence_number)
);
