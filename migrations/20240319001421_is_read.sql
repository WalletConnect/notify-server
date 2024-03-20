ALTER TABLE subscriber_notification ADD COLUMN is_read BOOLEAN NOT NULL DEFAULT FALSE;
CREATE INDEX subscriber_notification_is_read_idx  ON subscriber_notification (is_read);
