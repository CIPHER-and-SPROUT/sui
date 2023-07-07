DO
$$
    BEGIN
        CREATE TYPE owner_type AS ENUM ('address_owner', 'object_owner', 'shared', 'immutable');
        CREATE TYPE object_status AS ENUM ('created', 'mutated', 'deleted', 'wrapped', 'unwrapped', 'unwrapped_then_deleted');
        CREATE TYPE bcs_bytes AS
        (
            name TEXT,
            data bytea
        );
    EXCEPTION
        WHEN duplicate_object THEN
            -- Type already exists, do nothing
            NULL;
    END
$$;

CREATE TABLE objects
(
    epoch                  BIGINT        NOT NULL,
    checkpoint             BIGINT        NOT NULL,
    object_id              address       PRIMARY KEY,
    version                BIGINT        NOT NULL,
    object_digest          base58digest  NOT NULL,
    -- owner related
    owner_type             owner_type    NOT NULL,
    -- only non-null for objects with an owner,
    -- the owner can be an account or an object. 
    owner_address          address,
    -- only non-null for shared objects
    initial_shared_version BIGINT,
    previous_transaction   base58digest  NOT NULL,
    object_type            VARCHAR       NOT NULL,
    object_status          object_status NOT NULL,
    has_public_transfer    BOOLEAN       NOT NULL,
    storage_rebate         BIGINT        NOT NULL,
    bcs                    bcs_bytes[]   NOT NULL
) PARTITION BY HASH (object_id);
CREATE INDEX objects_owner_address ON objects (owner_type, owner_address);
CREATE INDEX objects_tx_digest ON objects (previous_transaction);
CREATE TABLE objects_partition_0 PARTITION OF objects FOR VALUES WITH (MODULUS 10, REMAINDER 0);
CREATE TABLE objects_partition_1 PARTITION OF objects FOR VALUES WITH (MODULUS 10, REMAINDER 1);
CREATE TABLE objects_partition_2 PARTITION OF objects FOR VALUES WITH (MODULUS 10, REMAINDER 2);
CREATE TABLE objects_partition_3 PARTITION OF objects FOR VALUES WITH (MODULUS 10, REMAINDER 3);
CREATE TABLE objects_partition_4 PARTITION OF objects FOR VALUES WITH (MODULUS 10, REMAINDER 4);
CREATE TABLE objects_partition_5 PARTITION OF objects FOR VALUES WITH (MODULUS 10, REMAINDER 5);
CREATE TABLE objects_partition_6 PARTITION OF objects FOR VALUES WITH (MODULUS 10, REMAINDER 6);
CREATE TABLE objects_partition_7 PARTITION OF objects FOR VALUES WITH (MODULUS 10, REMAINDER 7);
CREATE TABLE objects_partition_8 PARTITION OF objects FOR VALUES WITH (MODULUS 10, REMAINDER 8);
CREATE TABLE objects_partition_9 PARTITION OF objects FOR VALUES WITH (MODULUS 10, REMAINDER 9);

CREATE TABLE objects_history
(
    epoch                  BIGINT        NOT NULL,
    checkpoint             BIGINT        NOT NULL,
    object_id              address       NOT NULL,
    version                BIGINT        NOT NULL,
    object_digest          base58digest  NOT NULL,
    owner_type             owner_type    NOT NULL,
    owner_address          address,
    old_owner_type         owner_type,
    old_owner_address      address,
    initial_shared_version BIGINT,
    previous_transaction   base58digest  NOT NULL,
    object_type            VARCHAR       NOT NULL,
    object_status          object_status NOT NULL,
    has_public_transfer    BOOLEAN       NOT NULL,
    storage_rebate         BIGINT        NOT NULL,
    bcs                    bcs_bytes[]   NOT NULL,
    CONSTRAINT objects_history_pk PRIMARY KEY (object_id, version, checkpoint)
) PARTITION BY RANGE (checkpoint);
CREATE INDEX objects_history_checkpoint_index ON objects_history (checkpoint);
CREATE INDEX objects_history_id_version_index ON objects_history (object_id, version);
CREATE INDEX objects_history_owner_index ON objects_history (owner_type, owner_address);
CREATE INDEX objects_history_old_owner_index ON objects_history (old_owner_type, old_owner_address);
-- fast-path partition for the most recent objects before checkpoint, range is half-open.
-- partition name need to match regex of '.*(_partition_)\d+'.
CREATE TABLE objects_history_fast_path_partition_0 PARTITION OF objects_history FOR VALUES FROM (-1) TO (0);
CREATE TABLE objects_history_partition_0 PARTITION OF objects_history FOR VALUES FROM (0) TO (MAXVALUE);

CREATE OR REPLACE FUNCTION objects_modified_func() RETURNS TRIGGER AS
$body$
BEGIN
    IF (TG_OP = 'INSERT') THEN
        INSERT INTO objects_history
        VALUES (NEW.epoch, NEW.checkpoint, NEW.object_id, NEW.version, NEW.object_digest, NEW.owner_type,
                NEW.owner_address, NULL, NULL,
                NEW.initial_shared_version,
                NEW.previous_transaction, NEW.object_type, NEW.object_status, NEW.has_public_transfer,
                NEW.storage_rebate, NEW.bcs);
        RETURN NEW;
    ELSEIF (TG_OP = 'UPDATE') THEN
        INSERT INTO objects_history
        VALUES (NEW.epoch, NEW.checkpoint, NEW.object_id, NEW.version, NEW.object_digest, NEW.owner_type,
                NEW.owner_address, OLD.owner_type, OLD.owner_address,
                NEW.initial_shared_version,
                NEW.previous_transaction, NEW.object_type, NEW.object_status, NEW.has_public_transfer,
                NEW.storage_rebate, NEW.bcs);
        -- MUSTFIX(gegaowp): we cannot update checkpoint in-place, b/c checkpoint is a partition key,
        -- we need to prune old data in this partition periodically, like pruning old epochs upon new epoch.
        RETURN NEW;
    ELSIF (TG_OP = 'DELETE') THEN
        -- object deleted from the main table, archive the history for that object
        DELETE FROM objects_history WHERE object_id = old.object_id;
        RETURN OLD;
    ELSE
        RAISE WARNING '[OBJECTS_MODIFIED_FUNC] - Other action occurred: %, at %',TG_OP,NOW();
        RETURN NULL;
    END IF;

EXCEPTION
    WHEN data_exception THEN
        RAISE WARNING '[OBJECTS_MODIFIED_FUNC] - UDF ERROR [DATA EXCEPTION] - SQLSTATE: %, SQLERRM: %',SQLSTATE,SQLERRM;
        RETURN NULL;
    WHEN unique_violation THEN
        RAISE WARNING '[OBJECTS_MODIFIED_FUNC] - UDF ERROR [UNIQUE] - SQLSTATE: %, SQLERRM: %',SQLSTATE,SQLERRM;
        RETURN NULL;
    WHEN OTHERS THEN
        RAISE WARNING '[OBJECTS_MODIFIED_FUNC] - UDF ERROR [OTHER] - SQLSTATE: %, SQLERRM: %',SQLSTATE,SQLERRM;
        RETURN NULL;
END;
$body$
    LANGUAGE plpgsql;

CREATE TRIGGER objects_history
    AFTER INSERT OR UPDATE OR DELETE
    ON objects
    FOR EACH ROW
EXECUTE PROCEDURE objects_modified_func();

