CREATE FUNCTION get_address_lower(account_id text)
RETURNS text AS $$
BEGIN
    RETURN lower(split_part(account_id, ':', 3));
END;
$$ LANGUAGE plpgsql IMMUTABLE;

ALTER TABLE subscriber DROP CONSTRAINT subscriber_project_account_key;
CREATE INDEX subscriber_address ON subscriber (get_address_lower(account));
CREATE UNIQUE INDEX subscriber_project_account_key ON subscriber (project, get_address_lower(account));
