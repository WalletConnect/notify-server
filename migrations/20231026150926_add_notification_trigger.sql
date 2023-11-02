-- Function that sends a pg_notify with the notification id to process
CREATE FUNCTION "notification_for_delivery" ()
    RETURNS TRIGGER AS $$
    BEGIN
        PERFORM pg_notify('notification_for_delivery', NEW.id::text);
        RETURN NEW;
    END;
$$ LANGUAGE PLPGSQL;

-- Trigger to notify when a new notification to delivery is created
CREATE TRIGGER "subscriber_notification_insert" AFTER INSERT ON "subscriber_notification"
    FOR EACH ROW
    EXECUTE FUNCTION "notification_for_delivery" ();

-- Trigger to notify when a notification delivery status is updated to the `queued`
CREATE TRIGGER "subscriber_notification_update" AFTER UPDATE ON "subscriber_notification"
    FOR EACH ROW
    WHEN (OLD.status <> NEW.status AND NEW.status = 'queued')
    EXECUTE FUNCTION "notification_for_delivery" ();

-- TODO why double trigger?
