CREATE FUNCTION get_address_lower(account_id text)
RETURNS text AS $$
BEGIN
    RETURN lower(split_part(account_id, ':', 3));
END;
$$ LANGUAGE plpgsql IMMUTABLE;

CREATE INDEX subscriber_address ON subscriber (get_address_lower(account));

WITH duplicates AS (
    SELECT subscriber.id,
        ROW_NUMBER() OVER (
            PARTITION BY project, get_address_lower(account)
            ORDER BY count(subscriber_notification.id) DESC
        ) as rn
    FROM subscriber
    LEFT JOIN subscriber_notification ON subscriber_notification.subscriber=subscriber.id
    GROUP BY subscriber.id, project, get_address_lower(account)
)
DELETE FROM subscriber WHERE id IN (SELECT id FROM duplicates WHERE rn > 1);

ALTER TABLE subscriber DROP CONSTRAINT subscriber_project_account_key;
CREATE UNIQUE INDEX subscriber_project_account_key ON subscriber (project, get_address_lower(account));
