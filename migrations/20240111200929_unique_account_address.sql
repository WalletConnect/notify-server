CREATE FUNCTION get_address_lower(account_id text)
RETURNS text AS $$
BEGIN
    RETURN lower(split_part(account_id, ':', 3));
END;
$$ LANGUAGE plpgsql IMMUTABLE;

CREATE INDEX subscriber_address ON subscriber (get_address_lower(account));

WITH duplicates AS (
    SELECT id,
        ROW_NUMBER() OVER (
            PARTITION BY project, get_address_lower(account)
            -- ORDER BY some_criteria
        ) as rn
    FROM subscriber
)
DELETE FROM subscriber WHERE id IN (SELECT id FROM duplicates WHERE rn > 1);

ALTER TABLE subscriber DROP CONSTRAINT subscriber_project_account_key;
CREATE UNIQUE INDEX subscriber_project_account_key ON subscriber (project, get_address_lower(account));
