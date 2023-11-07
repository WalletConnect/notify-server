CREATE TABLE notification (
    id              UUID         PRIMARY KEY DEFAULT gen_random_uuid(),
    created_at      TIMESTAMPTZ  NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ  NOT NULL DEFAULT now(),
    project         UUID         NOT NULL REFERENCES project (id) ON DELETE CASCADE,
    notification_id VARCHAR(255) NOT NULL,
    type            UUID         NOT NULL,
    title           VARCHAR(255) NOT NULL,
    body            VARCHAR(255) NOT NULL,
    icon            VARCHAR(255), -- nullable
    url             VARCHAR(255), -- nullable

    UNIQUE (project, notification_id)
);

CREATE TYPE subscriber_notification_status
  AS ENUM ('queued', 'processing', 'published', 'failed');

CREATE TABLE subscriber_notification (
    id            UUID                            PRIMARY KEY DEFAULT gen_random_uuid(),
    created_at    TIMESTAMPTZ                     NOT NULL DEFAULT now(),
    updated_at    TIMESTAMPTZ                     NOT NULL DEFAULT now(),
    notification  UUID                            NOT NULL REFERENCES notification (id) ON DELETE CASCADE,
    subscriber    UUID                            NOT NULL REFERENCES subscriber (id) ON DELETE CASCADE,
    status        subscriber_notification_status  NOT NULL,

    UNIQUE (notification, subscriber)
);
CREATE INDEX subscriber_notification_status_idx ON subscriber_notification (status);
