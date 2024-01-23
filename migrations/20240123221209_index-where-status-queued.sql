CREATE INDEX subscriber_notification_status_queued ON subscriber_notification (status)
    WHERE status = 'queued';
